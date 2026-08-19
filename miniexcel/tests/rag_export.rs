mod common;

use miniexcel::{
    FormulaCalculationStatus, HeaderMode, MiniExcel, RagExportOptions, RagValue, ReadOptions,
};

#[test]
fn path_and_byte_exports_have_identical_chunks_and_manifests() {
    let path = common::fixture("TestIssue157.xlsx");
    let read_options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let export_options =
        RagExportOptions::new().with_chunk_rows(2).with_source_name("formula-workbook.xlsx");

    let mut export = MiniExcel::export_rag(&path, &read_options, &export_options)
        .expect("create path RAG export");
    let path_chunks =
        export.by_ref().collect::<miniexcel::Result<Vec<_>>>().expect("stream path chunks");
    let path_manifest = export.manifest().clone();

    let bytes = std::fs::read(path).expect("read workbook bytes");
    let mut byte_chunks = Vec::new();
    let byte_manifest =
        MiniExcel::visit_rag_chunks_from_bytes(&bytes, &read_options, &export_options, |chunk| {
            byte_chunks.push(chunk.clone());
            Ok(())
        })
        .expect("stream byte chunks");

    assert_eq!(byte_chunks, path_chunks);
    assert_eq!(byte_manifest, path_manifest);
    assert_eq!(path_manifest.emitted_rows(), 5);
    assert_eq!(path_manifest.emitted_chunks(), 3);
    assert_eq!(path_manifest.source_sha256().len(), 64);
    assert!(!path_manifest.truncated());
    assert!(path_chunks.iter().all(|chunk| chunk.header().is_some()));
    assert_eq!(path_chunks[0].data_range(), "A2:E3");

    let formula_cell = path_chunks[0]
        .rows()
        .iter()
        .flat_map(|row| row.cells())
        .find(|cell| cell.address() == "D2")
        .expect("formula cell D2");
    assert_eq!(formula_cell.formula(), Some("FALSE()"));
    assert_eq!(formula_cell.value(), &RagValue::Bool(false));

    let json = serde_json::to_value(&path_chunks[0]).expect("serialize RAG chunk");
    assert_eq!(json["rows"][0]["cells"][3]["calculationStatus"], "cachedOnly");
    assert_eq!(
        serde_json::from_value::<FormulaCalculationStatus>(
            json["rows"][0]["cells"][3]["calculationStatus"].clone(),
        )
        .expect("deserialize status"),
        FormulaCalculationStatus::CachedOnly
    );
}

#[test]
fn markdown_preserves_source_sheet_formula_and_style_metadata() {
    let path = common::fixture("TestIssue157.xlsx");
    let read_options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let export_options =
        RagExportOptions::new().with_chunk_rows(2).with_source_name("formula-workbook.xlsx");
    let mut export = MiniExcel::export_rag(path, &read_options, &export_options)
        .expect("create path RAG export");
    let mut markdown = Vec::new();
    export.manifest().write_markdown_stream_start(&mut markdown).expect("write stream metadata");
    export
        .next()
        .expect("first chunk")
        .expect("read first chunk")
        .write_markdown(&mut markdown)
        .expect("write first chunk");
    let markdown = String::from_utf8(markdown).expect("Markdown is UTF-8");

    assert!(markdown.contains("<!-- miniexcel:stream-start -->"));
    assert!(markdown.contains("# formula-workbook.xlsx"));
    assert!(markdown.contains("| Source SHA-256 | "));
    assert!(markdown.contains("| Worksheet | Sheet1 |"));
    assert!(markdown.contains("| Worksheet visibility | visible |"));
    assert!(markdown.contains("| Selected range | A1:worksheet end |"));
    assert!(markdown.contains("### Cell metadata"));
    assert!(markdown.contains("| D2 | bool | =FALSE() (cached value) | 2 | General |"));
}

#[test]
fn max_rows_truncates_without_overfilling_a_chunk() {
    let path = common::fixture("TestTypeMapping.xlsx");
    let read_options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let export_options =
        RagExportOptions::new().with_chunk_rows(25).with_max_rows(3).with_source_name("types.xlsx");

    let mut export = MiniExcel::export_rag(&path, &read_options, &export_options)
        .expect("create truncated path export");
    let chunks = export
        .by_ref()
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("stream truncated path export");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].rows().len(), 3);
    assert!(export.manifest().truncated());

    let bytes = std::fs::read(path).expect("read workbook bytes");
    let mut byte_chunks = Vec::new();
    let manifest =
        MiniExcel::visit_rag_chunks_from_bytes(&bytes, &read_options, &export_options, |chunk| {
            byte_chunks.push(chunk.clone());
            Ok(())
        })
        .expect("stream truncated byte export");
    assert_eq!(byte_chunks, chunks);
    assert_eq!(manifest.emitted_rows(), 3);
    assert!(manifest.truncated());
}

#[test]
fn exact_row_limit_is_not_reported_as_truncated() {
    let path = common::fixture("TestIssue157.xlsx");
    let read_options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let export_options = RagExportOptions::new().with_chunk_rows(2).with_max_rows(5);

    let mut export = MiniExcel::export_rag(&path, &read_options, &export_options)
        .expect("create exact-limit export");
    export.by_ref().collect::<miniexcel::Result<Vec<_>>>().expect("consume exact-limit export");
    assert_eq!(export.manifest().emitted_rows(), 5);
    assert!(!export.manifest().truncated());

    let bytes = std::fs::read(path).expect("read workbook bytes");
    let manifest =
        MiniExcel::visit_rag_chunks_from_bytes(&bytes, &read_options, &export_options, |_| Ok(()))
            .expect("consume exact-limit byte export");
    assert_eq!(manifest.emitted_rows(), 5);
    assert!(!manifest.truncated());
}

#[test]
fn hidden_sheets_require_explicit_opt_in() {
    let path = common::fixture("TestMultiSheetWithHiddenSheet.xlsx");
    let read_options = ReadOptions::new().with_sheet_name("HiddenSheet4");
    let error = match MiniExcel::export_rag(&path, &read_options, &RagExportOptions::new()) {
        Ok(_) => panic!("hidden sheet should be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("requires explicit opt-in"));

    MiniExcel::export_rag(
        path,
        &read_options,
        &RagExportOptions::new().with_allow_hidden_sheets(true),
    )
    .expect("explicitly allow hidden sheet");
}
