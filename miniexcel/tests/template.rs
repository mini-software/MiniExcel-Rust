use std::io::{Cursor, Read, Write};

use miniexcel::{CellValue, MiniExcel, ReadOptions, TemplateOptions};
use rust_xlsxwriter::{Format, Workbook};
use serde_json::json;
use zip::ZipArchive;
use zip::write::ZipWriter;

#[test]
fn fills_scalars_and_expands_list_rows_across_worksheets() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let template_path = temp_dir.path().join("template.xlsx");
    let output_path = temp_dir.path().join("output.xlsx");

    let mut workbook = Workbook::new();
    let report = workbook.add_worksheet();
    report.set_name("Report").unwrap();
    report.write_string(0, 0, "{{title}}").unwrap();
    report.write_string(1, 0, "Name").unwrap();
    report.write_string(1, 1, "Score").unwrap();
    report.write_string_with_format(2, 0, "{{items.name}}", &Format::new().set_bold()).unwrap();
    report.write_string(2, 1, "{{items.score}}").unwrap();
    report.write_string(2, 2, "{{active}}").unwrap();
    report.write_string(3, 0, "Footer: {{title}}").unwrap();
    let metadata = workbook.add_worksheet();
    metadata.set_name("Metadata").unwrap();
    metadata.write_string(0, 0, "{{title}}").unwrap();
    workbook.save(&template_path).expect("write template");

    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({
            "title": "Quarterly <Report>",
            "active": true,
            "items": [
                { "name": "Ada", "score": 10 },
                { "name": "Linus", "score": 20 }
            ]
        }),
        &TemplateOptions::new(),
    )
    .expect("fill template");

    assert_eq!(MiniExcel::get_sheet_names(&output_path).unwrap(), ["Report", "Metadata"]);
    let report_rows =
        MiniExcel::query_with_options(&output_path, &ReadOptions::new().with_sheet_name("Report"))
            .unwrap()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
    assert_eq!(report_rows[0]["A"], CellValue::String("Quarterly <Report>".to_owned()));
    assert_eq!(report_rows[2]["A"], CellValue::String("Ada".to_owned()));
    assert_eq!(report_rows[2]["B"], CellValue::Int(10));
    assert_eq!(report_rows[2]["C"], CellValue::Bool(true));
    assert_eq!(report_rows[3]["A"], CellValue::String("Linus".to_owned()));
    assert_eq!(report_rows[3]["B"], CellValue::Int(20));
    assert_eq!(report_rows[4]["A"], CellValue::String("Footer: Quarterly <Report>".to_owned()));

    let metadata_rows = MiniExcel::query_with_options(
        &output_path,
        &ReadOptions::new().with_sheet_name("Metadata"),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(metadata_rows[0]["A"], CellValue::String("Quarterly <Report>".to_owned()));

    let structured = MiniExcel::query_structured_with_options(
        &output_path,
        &ReadOptions::new().with_sheet_name("Report"),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_ne!(structured[2].cells()[0].style_id(), 0);
    assert_ne!(structured[3].cells()[0].style_id(), 0);
}

#[test]
fn fills_template_bytes_with_native_types_and_safe_strings() {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{count}}").unwrap();
    sheet.write_string(0, 1, "{{enabled}}").unwrap();
    sheet.write_string(0, 2, "Value: {{text}}").unwrap();
    sheet.write_string(1, 0, "{{formula_like}}").unwrap();
    sheet.write_string(2, 0, "{{missing}}").unwrap();
    let template = workbook.save_to_buffer().expect("write template bytes");

    let output = MiniExcel::save_as_template_bytes(
        &template,
        &json!({
            "count": 42,
            "enabled": true,
            "text": "A & B < C",
            "formula_like": "=1+1"
        }),
        &TemplateOptions::new(),
    )
    .expect("fill template bytes");
    let rows = MiniExcel::query_bytes(&output, &ReadOptions::new()).expect("read output bytes");

    assert_eq!(rows[0]["A"], CellValue::Int(42));
    assert_eq!(rows[0]["B"], CellValue::Bool(true));
    assert_eq!(rows[0]["C"], CellValue::String("Value: A & B < C".to_owned()));
    assert_eq!(rows[1]["A"], CellValue::String("'=1+1".to_owned()));
    assert!(rows[2]["A"].is_empty());

    let error = MiniExcel::save_as_template_bytes(
        &template,
        &json!({}),
        &TemplateOptions::new().with_ignore_missing_variables(false),
    )
    .expect_err("strict template variables");
    assert!(error.to_string().contains("count"));
}

#[test]
fn keeps_one_blank_template_row_for_an_empty_list_and_requires_explicit_overwrite() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let template_path = temp_dir.path().join("template.xlsx");
    let output_path = temp_dir.path().join("output.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{items.name}}").unwrap();
    sheet.write_string(1, 0, "Footer").unwrap();
    workbook.save(&template_path).expect("write template");

    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({ "items": [] }),
        &TemplateOptions::new(),
    )
    .expect("fill empty list");
    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({ "items": [] }),
        &TemplateOptions::new(),
    )
    .expect_err("reject overwrite by default");
    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({ "items": [] }),
        &TemplateOptions::new().with_overwrite_file(true),
    )
    .expect("overwrite explicitly");

    let rows =
        MiniExcel::query(&output_path).unwrap().collect::<miniexcel::Result<Vec<_>>>().unwrap();
    assert!(rows[0]["A"].is_empty());
    assert_eq!(rows[1]["A"], CellValue::String("Footer".to_owned()));
}

#[test]
fn renders_enumerable_cell_conditional_blocks_and_preserves_styles() {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{employees.name}}").unwrap();
    let conditional = "@if(name == Jack)\n{{employees.name}}\n@elseif(name == Neo)\nTest {{employees.name}}\n@else\n{{employees.department}}\n@endif";
    sheet.write_string_with_format(0, 1, conditional, &Format::new().set_bold()).unwrap();
    sheet
        .write_string(
            0,
            2,
            "@if(score >= 20)\nHigh\n@elseif(score > 10)\nMedium\n@else\nLow\n@endif",
        )
        .unwrap();
    sheet.write_string(0, 3, "@if(active == true)\nEnabled\n@else\nDisabled\n@endif").unwrap();
    sheet.write_string(1, 0, "Footer").unwrap();
    let template = workbook.save_to_buffer().unwrap();

    let output = MiniExcel::save_as_template_bytes(
        &template,
        &json!({
            "employees": [
                { "name": "Jack", "department": "HR", "score": 5, "active": true },
                { "name": "Neo", "department": "IT", "score": 15, "active": false },
                { "name": "Linus", "department": "Kernel", "score": 25, "active": true }
            ]
        }),
        &TemplateOptions::new(),
    )
    .unwrap();
    let rows = MiniExcel::query_bytes(&output, &ReadOptions::new()).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["B"], CellValue::String("Jack".to_owned()));
    assert_eq!(rows[1]["B"], CellValue::String("Test Neo".to_owned()));
    assert_eq!(rows[2]["B"], CellValue::String("Kernel".to_owned()));
    assert_eq!(rows[0]["C"], CellValue::String("Low".to_owned()));
    assert_eq!(rows[1]["C"], CellValue::String("Medium".to_owned()));
    assert_eq!(rows[2]["C"], CellValue::String("High".to_owned()));
    assert_eq!(rows[0]["D"], CellValue::String("Enabled".to_owned()));
    assert_eq!(rows[1]["D"], CellValue::String("Disabled".to_owned()));
    assert_eq!(rows[3]["A"], CellValue::String("Footer".to_owned()));

    let mut reader = std::io::Cursor::new(output);
    let mut styled_rows = Vec::new();
    MiniExcel::visit_structured_rows_from_reader(&mut reader, &ReadOptions::new(), |row| {
        styled_rows.push(row.clone());
        Ok(true)
    })
    .unwrap();
    assert_ne!(styled_rows[0].cells()[1].style_id(), 0);
    assert_eq!(styled_rows[0].cells()[1].style_id(), styled_rows[2].cells()[1].style_id());
}

#[test]
fn rejects_malformed_or_missing_conditional_fields() {
    for conditional in [
        "@if(name==Jack)\nJack\n@endif",
        "@if(name == Jack)\nJack",
        "@if(missing == Jack)\nJack\n@else\nOther\n@endif",
    ] {
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.write_string(0, 0, "{{employees.name}}").unwrap();
        sheet.write_string(0, 1, conditional).unwrap();
        let template = workbook.save_to_buffer().unwrap();
        let error = MiniExcel::save_as_template_bytes(
            &template,
            &json!({ "employees": [{ "name": "Jack" }] }),
            &TemplateOptions::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("conditional"), "{error}");
    }
}

#[test]
fn repeats_group_blocks_and_suppresses_only_adjacent_duplicate_headers() {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "Name").unwrap();
    sheet.write_string(0, 1, "Department").unwrap();
    sheet.write_string(1, 0, "@group").unwrap();
    sheet
        .write_string_with_format(2, 0, "@header{{employees.name}}", &Format::new().set_bold())
        .unwrap();
    sheet.write_string(3, 0, "{{employees.name}}").unwrap();
    sheet.write_string(3, 1, "{{employees.department}}").unwrap();
    sheet.write_string(3, 2, "@if(department == IT)\nTechnical\n@else\nPeople\n@endif").unwrap();
    sheet.write_string(4, 0, "@endgroup").unwrap();
    sheet.write_string(5, 0, "Footer").unwrap();
    let template = workbook.save_to_buffer().unwrap();

    let output = MiniExcel::save_as_template_bytes(
        &template,
        &json!({
            "employees": [
                { "name": "Jack", "department": "HR" },
                { "name": "Jack", "department": "IT" },
                { "name": "Loan", "department": "IT" },
                { "name": "Jack", "department": "HR" }
            ]
        }),
        &TemplateOptions::new(),
    )
    .unwrap();
    let rows = MiniExcel::query_bytes(&output, &ReadOptions::new()).unwrap();
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[1]["A"], CellValue::String("Jack".to_owned()));
    assert_eq!(rows[2]["A"], CellValue::String("Jack".to_owned()));
    assert_eq!(rows[2]["C"], CellValue::String("People".to_owned()));
    assert_eq!(rows[3]["A"], CellValue::String("Jack".to_owned()));
    assert_eq!(rows[3]["C"], CellValue::String("Technical".to_owned()));
    assert_eq!(rows[4]["A"], CellValue::String("Loan".to_owned()));
    assert_eq!(rows[5]["A"], CellValue::String("Loan".to_owned()));
    assert_eq!(rows[6]["A"], CellValue::String("Jack".to_owned()));
    assert_eq!(rows[7]["A"], CellValue::String("Jack".to_owned()));
    assert_eq!(rows[8]["A"], CellValue::String("Footer".to_owned()));

    let mut reader = std::io::Cursor::new(output);
    let mut styled_rows = Vec::new();
    MiniExcel::visit_structured_rows_from_reader(&mut reader, &ReadOptions::new(), |row| {
        styled_rows.push(row.clone());
        Ok(true)
    })
    .unwrap();
    assert_ne!(styled_rows[1].cells()[0].style_id(), 0);
    assert_eq!(styled_rows[1].cells()[0].style_id(), styled_rows[6].cells()[0].style_id());
}

#[test]
fn rejects_invalid_or_unsupported_group_blocks() {
    let cases = [
        (vec!["@group", "{{items.name}}"], json!({ "items": [{ "name": "A" }] })),
        (
            vec!["@group", "@group", "{{items.name}}", "@endgroup", "@endgroup"],
            json!({ "items": [{ "name": "A" }] }),
        ),
        (vec!["@group", "{{items.name}}", "@endgroup"], json!({ "items": [] })),
        (
            vec!["@group", "{{items.name}} {{others.name}}", "@endgroup"],
            json!({ "items": [{ "name": "A" }], "others": [{ "name": "B" }] }),
        ),
        (vec!["@endgroup"], json!({ "items": [{ "name": "A" }] })),
    ];
    for (rows, value) in cases {
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        for (index, value) in rows.into_iter().enumerate() {
            sheet.write_string(index as u32, 0, value).unwrap();
        }
        let template = workbook.save_to_buffer().unwrap();
        let error = MiniExcel::save_as_template_bytes(&template, &value, &TemplateOptions::new())
            .unwrap_err();
        assert!(error.to_string().contains("group"), "{error}");
    }

    let mut formula_workbook = Workbook::new();
    let sheet = formula_workbook.add_worksheet();
    sheet.write_string(0, 0, "@group").unwrap();
    sheet.write_string(1, 0, "{{items.name}}").unwrap();
    sheet.write_formula(1, 1, "=1+1").unwrap();
    sheet.write_string(2, 0, "@endgroup").unwrap();
    let template = formula_workbook.save_to_buffer().unwrap();
    let error = MiniExcel::save_as_template_bytes(
        &template,
        &json!({ "items": [{ "name": "A" }] }),
        &TemplateOptions::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("formula"), "{error}");

    let mut merged_workbook = Workbook::new();
    let sheet = merged_workbook.add_worksheet();
    sheet.write_string(0, 0, "@group").unwrap();
    sheet.write_string(1, 0, "{{items.name}}").unwrap();
    sheet.write_string(2, 0, "@endgroup").unwrap();
    sheet.merge_range(3, 0, 3, 1, "Merged", &Format::new()).unwrap();
    let template = merged_workbook.save_to_buffer().unwrap();
    let error = MiniExcel::save_as_template_bytes(
        &template,
        &json!({ "items": [{ "name": "A" }] }),
        &TemplateOptions::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("merged"), "{error}");
}

#[test]
fn renders_formula_templates_with_final_row_ranges_and_recalculation_metadata() {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_formula(0, 5, "=1+1").unwrap();
    sheet.write_string(2, 0, "{{items.name}}").unwrap();
    sheet.write_string(2, 1, "{{items.qty}}").unwrap();
    sheet.write_string_with_format(2, 2, "$=B{{$rowindex}}*2", &Format::new().set_bold()).unwrap();
    sheet.write_string(4, 0, "Total").unwrap();
    sheet.write_string(4, 3, "$=SUM(B{{$enumrowstart}}:B{{$enumrowend}})").unwrap();
    let template = add_calc_chain(workbook.save_to_buffer().unwrap());

    let output = MiniExcel::save_as_template_bytes(
        &template,
        &json!({
            "items": [
                { "name": "A", "qty": 2 },
                { "name": "B", "qty": 3 }
            ]
        }),
        &TemplateOptions::new(),
    )
    .unwrap();

    let mut reader = Cursor::new(&output);
    let mut cells = std::collections::BTreeMap::new();
    MiniExcel::visit_structured_rows_from_reader(&mut reader, &ReadOptions::new(), |row| {
        for cell in row.cells() {
            cells.insert(cell.address(), cell.clone());
        }
        Ok(true)
    })
    .unwrap();
    assert_eq!(cells["C3"].formula(), Some("B3*2"));
    assert_eq!(cells["C4"].formula(), Some("B4*2"));
    assert_eq!(cells["D6"].formula(), Some("SUM(B3:B4)"));
    assert_eq!(cells["F1"].formula(), Some("1+1"));
    assert!(cells["C3"].value().is_empty());
    assert_ne!(cells["C3"].style_id(), 0);
    assert_eq!(cells["C3"].style_id(), cells["C4"].style_id());

    let entries = package_entries(&output);
    assert!(!entries.contains_key("xl/calcChain.xml"));
    let relationships = std::str::from_utf8(&entries["xl/_rels/workbook.xml.rels"]).unwrap();
    assert!(!relationships.contains("calcChain"));
    let content_types = std::str::from_utf8(&entries["[Content_Types].xml"]).unwrap();
    assert!(!content_types.contains("calcChain"));
    let workbook = std::str::from_utf8(&entries["xl/workbook.xml"]).unwrap();
    assert!(workbook.contains("fullCalcOnLoad=\"1\""));
    assert!(workbook.contains("forceFullCalc=\"1\""));
}

#[test]
fn formula_templates_reject_invalid_ranges_and_do_not_promote_data_strings() {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{formula_like}}").unwrap();
    sheet.write_string(0, 1, " =$=1+1").unwrap();
    let template = workbook.save_to_buffer().unwrap();
    let output = MiniExcel::save_as_template_bytes(
        &template,
        &json!({ "formula_like": "$=1+1" }),
        &TemplateOptions::new(),
    )
    .unwrap();
    let rows = MiniExcel::query_bytes(&output, &ReadOptions::new()).unwrap();
    assert_eq!(rows[0]["A"], CellValue::String("'$=1+1".to_owned()));
    assert_eq!(rows[0]["B"], CellValue::String(" =$=1+1".to_owned()));

    for formula in ["$=", "$=SUM(B{{$enumrowstart}}:B{{$enumrowend}})"] {
        let mut workbook = Workbook::new();
        workbook.add_worksheet().write_string(0, 0, formula).unwrap();
        let template = workbook.save_to_buffer().unwrap();
        let error =
            MiniExcel::save_as_template_bytes(&template, &json!({}), &TemplateOptions::new())
                .unwrap_err();
        assert!(error.to_string().contains("formula") || error.to_string().contains("enumrow"));
    }
}

fn add_calc_chain(source: Vec<u8>) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(source)).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_owned();
        let options = entry.options();
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).unwrap();
        if name == "xl/_rels/workbook.xml.rels" {
            let xml = String::from_utf8(payload).unwrap().replace(
                "</Relationships>",
                "<Relationship Id=\"rIdCalc\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain\" Target=\"calcChain.xml\"/></Relationships>",
            );
            payload = xml.into_bytes();
        } else if name == "[Content_Types].xml" {
            let xml = String::from_utf8(payload).unwrap().replace(
                "</Types>",
                "<Override PartName=\"/xl/calcChain.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml\"/></Types>",
            );
            payload = xml.into_bytes();
        }
        writer.start_file(name, options).unwrap();
        writer.write_all(&payload).unwrap();
    }
    writer
        .start_file(
            "xl/calcChain.xml",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer
        .write_all(br#"<calcChain xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><c r="F1" i="1"/></calcChain>"#)
        .unwrap();
    writer.finish().unwrap().into_inner()
}

fn package_entries(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = std::collections::BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).unwrap();
        entries.insert(entry.name().to_owned(), payload);
    }
    entries
}
