use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::donor::DonorWorksheet;
use super::package::{DefinedName, PackageInventory, WorksheetAllocation};
use super::style::rebase_styles;
use crate::{Error, Result};

const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
const WORKBOOK_PATH: &str = "xl/workbook.xml";
const WORKBOOK_RELS_PATH: &str = "xl/_rels/workbook.xml.rels";
const WORKSHEET_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const WORKSHEET_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PackageRewriteStage {
    Copy,
    Finish,
}

pub(crate) fn append_worksheet<R>(source: R, donor: &DonorWorksheet) -> Result<Vec<u8>>
where
    R: Read + Seek,
{
    let output = append_worksheet_to_writer(source, Cursor::new(Vec::new()), donor)?;
    Ok(output.into_inner())
}

pub(crate) fn append_worksheet_to_writer<R, W>(
    source: R,
    destination: W,
    donor: &DonorWorksheet,
) -> Result<W>
where
    R: Read + Seek,
    W: Write + Seek,
{
    append_worksheet_to_writer_with_hook(source, destination, donor, |_| Ok(()))
}

pub(super) fn append_worksheet_to_writer_with_hook<R, W, F>(
    mut source: R,
    destination: W,
    donor: &DonorWorksheet,
    mut checkpoint: F,
) -> Result<W>
where
    R: Read + Seek,
    W: Write + Seek,
    F: FnMut(PackageRewriteStage) -> Result<()>,
{
    let inventory = PackageInventory::inspect(&mut source)?;
    inventory.ensure_sheet_absent(&donor.sheet_name)?;
    let allocation = inventory.allocate_worksheet()?;
    let styles_path = styles_path(&inventory)?;

    source.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(source).map_err(|error| {
        Error::insert_package(format!("cannot reopen source workbook: {error}"))
    })?;
    let workbook_xml = read_part(&mut archive, WORKBOOK_PATH)?;
    let workbook_rels_xml = read_part(&mut archive, WORKBOOK_RELS_PATH)?;
    let content_types_xml = read_part(&mut archive, CONTENT_TYPES_PATH)?;
    let styles_xml = read_part(&mut archive, &styles_path)?;

    let style_rebase = rebase_styles(&styles_xml, donor)?;
    let workbook_xml = append_workbook(
        &workbook_xml,
        &donor.sheet_name,
        &allocation,
        inventory.sheets.len(),
        &donor.local_defined_names,
    )?;
    let workbook_rels_xml = append_workbook_relationship(&workbook_rels_xml, &allocation)?;
    let has_worksheet_override = inventory
        .content_types
        .overrides
        .iter()
        .find(|entry| entry.part_name == allocation.package_path)
        .map(|entry| entry.content_type.as_str());
    if has_worksheet_override.is_some_and(|content_type| content_type != WORKSHEET_CONTENT_TYPE) {
        return Err(Error::unsafe_package(format!(
            "worksheet target '{}' has incompatible content type",
            allocation.package_path
        )));
    }
    let content_types_xml = append_content_type_override(
        &content_types_xml,
        &allocation,
        has_worksheet_override.is_none(),
    )?;

    let replacements = BTreeMap::from([
        (CONTENT_TYPES_PATH.to_owned(), content_types_xml),
        (WORKBOOK_PATH.to_owned(), workbook_xml),
        (WORKBOOK_RELS_PATH.to_owned(), workbook_rels_xml),
        (styles_path, style_rebase.styles_xml),
    ]);
    write_package(
        archive,
        destination,
        &replacements,
        &allocation.package_path,
        &style_rebase.worksheet_xml,
        &mut checkpoint,
    )
}

fn styles_path(inventory: &PackageInventory) -> Result<String> {
    let mut paths = inventory
        .relationships
        .iter()
        .filter(|relationship| {
            relationship.source.as_deref() == Some(WORKBOOK_PATH)
                && relationship.relationship_type.rsplit('/').next() == Some("styles")
        })
        .map(|relationship| {
            relationship.normalized_target.clone().ok_or_else(|| {
                Error::insert_package("workbook styles relationship cannot be external")
            })
        });
    let path = paths
        .next()
        .transpose()?
        .ok_or_else(|| Error::insert_package("workbook has no styles relationship"))?;
    if paths.next().is_some() {
        return Err(Error::unsafe_package("workbook has multiple styles relationships"));
    }
    if !inventory.entry_names.contains(&path) {
        return Err(Error::insert_package(format!("styles part '{path}' is missing")));
    }
    Ok(path)
}

fn append_workbook(
    xml: &[u8],
    sheet_name: &str,
    allocation: &WorksheetAllocation,
    local_sheet_id: usize,
    local_defined_names: &[DefinedName],
) -> Result<Vec<u8>> {
    let has_defined_names = contains_element(xml, b"definedNames")?;
    let relationship_prefix = workbook_relationship_prefix(xml)?;
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len() + 256));
    let mut sheets_seen = 0;
    let mut defined_names_seen = 0;
    let mut depth = 0_usize;
    let mut workbook_prefix = None::<String>;
    let mut pending_defined_names = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid workbook XML: {error}")))?;
        if pending_defined_names && is_direct_child_after_defined_names(&event, depth) {
            write_defined_names(
                &mut writer,
                local_defined_names,
                local_sheet_id,
                workbook_prefix.as_deref().unwrap_or_default(),
            )?;
            pending_defined_names = false;
        }
        match event {
            Event::End(end) if local_name(end.name().as_ref()) == b"sheets" => {
                let prefix = element_prefix(end.name().as_ref())?;
                workbook_prefix = Some(prefix.clone());
                write_new_sheet(
                    &mut writer,
                    sheet_name,
                    allocation,
                    &prefix,
                    &relationship_prefix,
                )?;
                write_event(&mut writer, Event::End(end))?;
                sheets_seen += 1;
                if !has_defined_names && !local_defined_names.is_empty() {
                    pending_defined_names = true;
                }
                depth = depth.saturating_sub(1);
            }
            Event::End(end) if local_name(end.name().as_ref()) == b"definedNames" => {
                let prefix = element_prefix(end.name().as_ref())?;
                for defined_name in local_defined_names {
                    write_defined_name(&mut writer, defined_name, local_sheet_id, &prefix)?;
                }
                write_event(&mut writer, Event::End(end))?;
                defined_names_seen += 1;
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            Event::Start(start) => {
                write_event(&mut writer, Event::Start(start))?;
                depth += 1;
            }
            Event::End(end) => {
                write_event(&mut writer, Event::End(end))?;
                depth = depth.saturating_sub(1);
            }
            event => write_event(&mut writer, event)?,
        }
    }
    if sheets_seen != 1 {
        return Err(Error::insert_package(format!(
            "workbook contains {sheets_seen} sheets containers"
        )));
    }
    if has_defined_names && defined_names_seen != 1 {
        return Err(Error::insert_package(format!(
            "workbook contains {defined_names_seen} definedNames containers"
        )));
    }
    if pending_defined_names {
        return Err(Error::insert_package("workbook ended before definedNames could be inserted"));
    }
    Ok(writer.into_inner())
}

fn append_workbook_relationship(xml: &[u8], allocation: &WorksheetAllocation) -> Result<Vec<u8>> {
    append_empty_child(xml, b"Relationships", |qualified_name| {
        let mut relationship = BytesStart::new(qualified_name);
        relationship.push_attribute(("Id", allocation.relationship_id.as_str()));
        relationship.push_attribute(("Type", WORKSHEET_RELATIONSHIP_TYPE));
        relationship.push_attribute(("Target", allocation.workbook_target.as_str()));
        relationship
    })
}

fn append_content_type_override(
    xml: &[u8],
    allocation: &WorksheetAllocation,
    should_append: bool,
) -> Result<Vec<u8>> {
    if !should_append {
        return Ok(xml.to_vec());
    }
    append_empty_child(xml, b"Types", |qualified_name| {
        let mut override_element = BytesStart::new(qualified_name);
        let part_name = format!("/{}", allocation.package_path);
        override_element.push_attribute(("PartName", part_name.as_str()));
        override_element.push_attribute(("ContentType", WORKSHEET_CONTENT_TYPE));
        override_element
    })
}

fn append_empty_child<F>(xml: &[u8], parent_name: &[u8], child: F) -> Result<Vec<u8>>
where
    F: FnOnce(String) -> BytesStart<'static>,
{
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len() + 192));
    let mut child = Some(child);
    let mut parents_seen = 0;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid control XML: {error}")))?;
        match event {
            Event::End(end) if local_name(end.name().as_ref()) == parent_name => {
                let child_name = qualified_child_name(
                    end.name().as_ref(),
                    match parent_name {
                        b"Relationships" => "Relationship",
                        b"Types" => "Override",
                        _ => return Err(Error::insert_package("unsupported control XML parent")),
                    },
                )?;
                let child = child
                    .take()
                    .ok_or_else(|| Error::insert_package("duplicate control XML root"))?(
                    child_name,
                );
                write_event(&mut writer, Event::Empty(child))?;
                write_event(&mut writer, Event::End(end))?;
                parents_seen += 1;
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
    }
    if parents_seen != 1 {
        return Err(Error::insert_package(format!(
            "control XML contains {parents_seen} '{}' roots",
            String::from_utf8_lossy(parent_name)
        )));
    }
    Ok(writer.into_inner())
}

fn write_new_sheet(
    writer: &mut Writer<Vec<u8>>,
    sheet_name: &str,
    allocation: &WorksheetAllocation,
    element_prefix: &str,
    relationship_prefix: &str,
) -> Result<()> {
    let mut sheet = BytesStart::new(qualify(element_prefix, "sheet"));
    let sheet_id = allocation.sheet_id.to_string();
    let relationship_id_name = qualify(relationship_prefix, "id");
    sheet.push_attribute(("name", sheet_name));
    sheet.push_attribute(("sheetId", sheet_id.as_str()));
    sheet.push_attribute((relationship_id_name.as_str(), allocation.relationship_id.as_str()));
    write_event(writer, Event::Empty(sheet))
}

fn write_defined_names(
    writer: &mut Writer<Vec<u8>>,
    names: &[DefinedName],
    local_sheet_id: usize,
    element_prefix: &str,
) -> Result<()> {
    write_event(writer, Event::Start(BytesStart::new(qualify(element_prefix, "definedNames"))))?;
    for name in names {
        write_defined_name(writer, name, local_sheet_id, element_prefix)?;
    }
    write_event(writer, Event::End(BytesEnd::new(qualify(element_prefix, "definedNames"))))
}

fn write_defined_name(
    writer: &mut Writer<Vec<u8>>,
    defined_name: &DefinedName,
    local_sheet_id: usize,
    element_prefix: &str,
) -> Result<()> {
    let mut element = BytesStart::new(qualify(element_prefix, "definedName"));
    let local_sheet_id = local_sheet_id.to_string();
    element.push_attribute(("name", defined_name.name.as_str()));
    element.push_attribute(("localSheetId", local_sheet_id.as_str()));
    if defined_name.hidden {
        element.push_attribute(("hidden", "1"));
    }
    write_event(writer, Event::Start(element))?;
    write_event(writer, Event::Text(BytesText::new(&defined_name.formula)))?;
    write_event(writer, Event::End(BytesEnd::new(qualify(element_prefix, "definedName"))))
}

fn is_direct_child_after_defined_names(event: &Event<'_>, depth: usize) -> bool {
    if depth != 1 {
        return false;
    }
    match event {
        Event::Start(start) | Event::Empty(start) => {
            !matches!(local_name(start.name().as_ref()), b"functionGroups" | b"externalReferences")
        }
        Event::End(end) => local_name(end.name().as_ref()) == b"workbook",
        _ => false,
    }
}

fn workbook_relationship_prefix(xml: &[u8]) -> Result<String> {
    const RELATIONSHIP_NAMESPACE: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid workbook XML: {error}")))?
        {
            Event::Start(root) if local_name(root.name().as_ref()) == b"workbook" => {
                for attribute in root.attributes().with_checks(false) {
                    let attribute = attribute.map_err(|error| {
                        Error::insert_package(format!("invalid workbook namespace: {error}"))
                    })?;
                    let key = attribute.key.as_ref();
                    if let Some(prefix) = key.strip_prefix(b"xmlns:") {
                        let value = attribute.decode_and_unescape_value(root.decoder()).map_err(
                            |error| {
                                Error::insert_package(format!(
                                    "invalid workbook namespace value: {error}"
                                ))
                            },
                        )?;
                        if value == RELATIONSHIP_NAMESPACE {
                            return String::from_utf8(prefix.to_vec()).map_err(|_| {
                                Error::insert_package("workbook relationship prefix is not UTF-8")
                            });
                        }
                    }
                }
                return Err(Error::insert_package("workbook has no relationship namespace prefix"));
            }
            Event::Eof => return Err(Error::insert_package("workbook root is missing")),
            _ => {}
        }
    }
}

fn qualified_child_name(parent_name: &[u8], child_local_name: &str) -> Result<String> {
    Ok(qualify(&element_prefix(parent_name)?, child_local_name))
}

fn element_prefix(name: &[u8]) -> Result<String> {
    let name = std::str::from_utf8(name)
        .map_err(|_| Error::insert_package("XML element name is not UTF-8"))?;
    Ok(name.rsplit_once(':').map_or("", |(prefix, _)| prefix).to_owned())
}

fn qualify(prefix: &str, local_name: &str) -> String {
    if prefix.is_empty() { local_name.to_owned() } else { format!("{prefix}:{local_name}") }
}

fn contains_element(xml: &[u8], name: &[u8]) -> Result<bool> {
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid control XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == name =>
            {
                return Ok(true);
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn read_part<R>(archive: &mut ZipArchive<R>, path: &str) -> Result<Vec<u8>>
where
    R: Read + Seek,
{
    let mut entry = archive.by_name(path).map_err(|error| {
        Error::insert_package(format!("cannot read source part '{path}': {error}"))
    })?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn write_package<R, W, F>(
    mut archive: ZipArchive<R>,
    destination: W,
    replacements: &BTreeMap<String, Vec<u8>>,
    worksheet_path: &str,
    worksheet_xml: &[u8],
    checkpoint: &mut F,
) -> Result<W>
where
    R: Read + Seek,
    W: Write + Seek,
    F: FnMut(PackageRewriteStage) -> Result<()>,
{
    let comment = archive.comment().to_vec();
    let zip64_comment = archive.zip64_comment().map(<[u8]>::to_vec);
    let mut writer = ZipWriter::new(destination);
    writer.set_raw_comment(comment.into_boxed_slice());
    writer.set_raw_zip64_comment(zip64_comment.map(Vec::into_boxed_slice));

    checkpoint(PackageRewriteStage::Copy)?;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|error| {
            Error::insert_package(format!("cannot copy source ZIP entry: {error}"))
        })?;
        let name = entry.name().to_owned();
        if let Some(replacement) = replacements.get(&name) {
            let options = entry.options();
            writer.start_file(&name, options).map_err(|error| {
                Error::insert_package(format!("cannot replace ZIP entry '{name}': {error}"))
            })?;
            writer.write_all(replacement)?;
        } else {
            writer.raw_copy_file(entry).map_err(|error| {
                Error::insert_package(format!("cannot raw-copy ZIP entry '{name}': {error}"))
            })?;
        }
    }

    writer
        .start_file(
            worksheet_path,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| {
            Error::insert_package(format!("cannot create worksheet '{worksheet_path}': {error}"))
        })?;
    writer.write_all(worksheet_xml)?;
    checkpoint(PackageRewriteStage::Finish)?;
    writer
        .finish()
        .map_err(|error| Error::insert_package(format!("cannot finish rewritten package: {error}")))
}

fn write_event(writer: &mut Writer<Vec<u8>>, event: Event<'_>) -> Result<()> {
    writer.write_event(event).map_err(|error| {
        Error::insert_package(format!("cannot write package control XML: {error}"))
    })
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use quick_xml::Reader;
    use quick_xml::events::Event;
    use zip::{CompressionMethod, DateTime};

    use super::*;
    use crate::insert::donor::DonorBuilder;
    use crate::writer::XlsxWriter;
    use crate::{
        CellValue, DynamicRow, HeaderMode, MiniExcel, ReadOptions, SheetVisibility, WriteOptions,
    };

    #[derive(Debug, Eq, PartialEq)]
    struct EntrySnapshot {
        payload: Vec<u8>,
        compressed_payload: Vec<u8>,
        crc32: u32,
        compression: CompressionMethod,
        modified: Option<DateTime>,
        unix_mode: Option<u32>,
        comment: String,
        extra_data: Option<Vec<u8>>,
    }

    #[test]
    fn append_package_preserves_existing_parts_and_appends_visible_sheet() {
        let source = source_package();
        let before = package_snapshot(&source);
        let source_inventory = PackageInventory::inspect(Cursor::new(&source)).unwrap();
        let source_relationship_ids = source_inventory
            .relationships
            .iter()
            .filter(|relationship| relationship.source.as_deref() == Some(WORKBOOK_PATH))
            .map(|relationship| relationship.id.clone())
            .collect::<Vec<_>>();
        let mut inserted = DynamicRow::new();
        inserted.insert("Label".to_owned(), CellValue::String("Inserted".to_owned()));
        inserted.insert("Value".to_owned(), CellValue::Int(7));
        let donor = DonorBuilder::from_dynamic(
            &[inserted],
            &WriteOptions::new().with_sheet_name("New Data"),
        )
        .unwrap();

        let output = append_worksheet(Cursor::new(&source), &donor).unwrap();
        let after = package_snapshot(&output);
        let before_order = package_entry_order(&source);
        let after_order = package_entry_order(&output);
        assert_eq!(after_order[..before_order.len()], before_order);
        assert_eq!(after_order.last().map(String::as_str), Some("xl/worksheets/sheet3.xml"));
        let inventory = PackageInventory::inspect(Cursor::new(&output)).unwrap();
        assert_eq!(
            inventory.sheets.iter().map(|sheet| sheet.name.as_str()).collect::<Vec<_>>(),
            ["Data", "Hidden", "New Data"]
        );
        assert_eq!(inventory.views[0].active_tab, 0);
        assert_eq!(inventory.sheets[..source_inventory.sheets.len()], source_inventory.sheets);
        assert_eq!(
            inventory.defined_names[..source_inventory.defined_names.len()],
            source_inventory.defined_names
        );
        assert_eq!(inventory.sheets[2].visibility, SheetVisibility::Visible);
        assert_eq!(inventory.sheets[2].sheet_id, 3);
        assert!(!source_relationship_ids.contains(&inventory.sheets[2].relationship_id));
        assert_eq!(inventory.sheets[2].target, "xl/worksheets/sheet3.xml");
        assert!(
            inventory.defined_names.iter().any(|name| {
                name.name == "_xlnm._FilterDatabase"
                    && name.local_sheet_id == Some(2)
                    && name.formula == "'New Data'!$A$1:$B$2"
            }),
            "donor={:?}, rewritten={:?}",
            donor.local_defined_names,
            inventory.defined_names
        );

        let changed = [CONTENT_TYPES_PATH, WORKBOOK_PATH, WORKBOOK_RELS_PATH, "xl/styles.xml"];
        for (name, snapshot) in &before {
            if !changed.contains(&name.as_str()) {
                assert_eq!(after.get(name), Some(snapshot), "entry '{name}' changed");
            }
        }
        assert!(after.contains_key("xl/worksheets/sheet3.xml"));
        assert_eq!(archive_comment(&output), b"preserve archive comment");

        assert_eq!(
            xml_element(&before[WORKBOOK_PATH].payload, b"bookViews"),
            xml_element(&after[WORKBOOK_PATH].payload, b"bookViews")
        );
        assert_eq!(
            xml_element(&before[WORKBOOK_PATH].payload, b"calcPr"),
            xml_element(&after[WORKBOOK_PATH].payload, b"calcPr")
        );
        let content_types = String::from_utf8(after[CONTENT_TYPES_PATH].payload.clone()).unwrap();
        assert_eq!(content_types.matches("PartName=\"/xl/worksheets/sheet3.xml\"").count(), 1);

        let mut existing_rows = Vec::new();
        MiniExcel::visit_structured_rows_from_reader(
            &mut Cursor::new(&output),
            &ReadOptions::new().with_sheet_name("Data"),
            |row| {
                existing_rows.push(row.clone());
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(existing_rows[1].cells()[1].formula(), Some("20+21"));
        assert_eq!(existing_rows[1].cells()[1].value(), &CellValue::Int(41));

        let inserted_rows = MiniExcel::query_bytes(
            &output,
            &ReadOptions::new().with_sheet_name("New Data").with_header_mode(HeaderMode::FirstRow),
        )
        .unwrap();
        assert_eq!(
            inserted_rows[0].get("Label"),
            Some(&CellValue::String("Inserted".to_owned())),
            "inserted rows: {inserted_rows:?}"
        );
        assert_eq!(inserted_rows[0]["Value"], CellValue::Int(7));
    }

    #[test]
    fn append_package_rejects_duplicate_sheet_names_without_output() {
        let source = source_package();
        let donor = DonorBuilder::from_dynamic(
            &[single_value_row("duplicate")],
            &WriteOptions::new().with_sheet_name("dAtA"),
        )
        .unwrap();
        assert!(append_worksheet(Cursor::new(source), &donor).is_err());

        let mut writer = XlsxWriter::new();
        writer
            .add_rows(&[single_value_row("unicode")], &WriteOptions::new().with_sheet_name("É"))
            .unwrap();
        let unicode_source = writer.save_to_bytes().unwrap();
        let unicode_donor = DonorBuilder::from_dynamic(
            &[single_value_row("duplicate")],
            &WriteOptions::new().with_sheet_name("é"),
        )
        .unwrap();
        assert!(append_worksheet(Cursor::new(unicode_source), &unicode_donor).is_err());
    }

    #[test]
    fn control_part_patch_handles_missing_defined_names_and_existing_override() {
        let allocation = WorksheetAllocation {
            sheet_id: 5,
            relationship_id: "rId8".to_owned(),
            workbook_target: "worksheets/new.xml".to_owned(),
            package_path: "xl/worksheets/new.xml".to_owned(),
        };
        let defined_name = DefinedName {
            name: "_xlnm._FilterDatabase".to_owned(),
            local_sheet_id: Some(0),
            hidden: true,
            formula: "'New Data'!$A$1:$A$2".to_owned(),
        };
        let workbook = br#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets><externalReferences><externalReference r:id="rId4"/></externalReferences><calcPr calcId="7"/></workbook>"#;
        let patched =
            append_workbook(workbook, "New Data", &allocation, 1, &[defined_name]).unwrap();
        let text = String::from_utf8(patched).unwrap();
        assert!(text.contains("<definedNames><definedName"));
        assert!(text.find("</sheets>").unwrap() < text.find("<definedNames>").unwrap());
        assert!(text.find("</externalReferences>").unwrap() < text.find("<definedNames>").unwrap());
        assert!(text.find("</definedNames>").unwrap() < text.find("<calcPr").unwrap());

        let types = br#"<Types><Override PartName="/xl/worksheets/new.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
        assert_eq!(append_content_type_override(types, &allocation, false).unwrap(), types);

        let prefixed_workbook = br#"<x:workbook xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x:sheets><x:sheet name="A" sheetId="1" q:id="rId1"/></x:sheets><x:calcPr calcId="7"/></x:workbook>"#;
        let prefixed = String::from_utf8(
            append_workbook(prefixed_workbook, "New Data", &allocation, 1, &[]).unwrap(),
        )
        .unwrap();
        assert!(prefixed.contains("<x:sheet name=\"New Data\" sheetId=\"5\" q:id=\"rId8\"/>"));

        let prefixed_rels = br#"<p:Relationships xmlns:p="http://schemas.openxmlformats.org/package/2006/relationships"></p:Relationships>"#;
        let rels =
            String::from_utf8(append_workbook_relationship(prefixed_rels, &allocation).unwrap())
                .unwrap();
        assert!(rels.contains("<p:Relationship "));

        let prefixed_types = br#"<c:Types xmlns:c="http://schemas.openxmlformats.org/package/2006/content-types"></c:Types>"#;
        let types = String::from_utf8(
            append_content_type_override(prefixed_types, &allocation, true).unwrap(),
        )
        .unwrap();
        assert!(types.contains("<c:Override "));
    }

    #[test]
    fn append_package_escapes_and_roundtrips_autofilter_sheet_names() {
        let source = source_package();
        let donor = DonorBuilder::from_dynamic(
            &[single_value_row("escaped")],
            &WriteOptions::new().with_sheet_name("O'Brien & Sons"),
        )
        .unwrap();
        let output = append_worksheet(Cursor::new(source), &donor).unwrap();
        let inventory = PackageInventory::inspect(Cursor::new(&output)).unwrap();
        assert!(inventory.defined_names.iter().any(|name| {
            name.local_sheet_id == Some(2) && name.formula == "'O''Brien & Sons'!$A$1:$A$2"
        }));
        let workbook = package_snapshot(&output)[WORKBOOK_PATH].payload.clone();
        let workbook = String::from_utf8(workbook).unwrap();
        assert!(workbook.contains("&amp;"));
    }

    fn source_package() -> Vec<u8> {
        let mut writer = XlsxWriter::new();
        let first = [formula_source_row()];
        writer.add_rows(&first, &WriteOptions::new().with_sheet_name("Data")).unwrap();
        let second = [single_value_row("hidden")];
        writer
            .add_rows(
                &second,
                &WriteOptions::new()
                    .with_sheet_name("Hidden")
                    .with_sheet_visibility("Hidden", SheetVisibility::Hidden),
            )
            .unwrap();
        let package = writer.save_to_bytes().unwrap();
        enrich_source_package(&package)
    }

    fn formula_source_row() -> DynamicRow {
        let mut row = DynamicRow::new();
        row.insert("Name".to_owned(), CellValue::String("Existing".to_owned()));
        row.insert("Value".to_owned(), CellValue::Int(41));
        row
    }

    fn single_value_row(value: &str) -> DynamicRow {
        let mut row = DynamicRow::new();
        row.insert("Value".to_owned(), CellValue::String(value.to_owned()));
        row
    }

    fn enrich_source_package(package: &[u8]) -> Vec<u8> {
        let extras = [
            ("xl/tables/table1.xml", b"table".as_slice()),
            ("xl/drawings/drawing1.xml", b"drawing".as_slice()),
            ("xl/comments1.xml", b"comments".as_slice()),
            ("xl/externalLinks/externalLink1.xml", b"external".as_slice()),
            ("customXml/item1.xml", b"custom".as_slice()),
        ];
        let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        output.set_comment("preserve archive comment");
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let name = entry.name().to_owned();
            if name == "xl/worksheets/sheet1.xml" {
                let options = entry.options();
                let mut xml = String::new();
                entry.read_to_string(&mut xml).unwrap();
                let xml = xml.replace("<v>41</v>", "<f>20+21</f><v>41</v>");
                output.start_file(name, options).unwrap();
                output.write_all(xml.as_bytes()).unwrap();
            } else {
                output.raw_copy_file(entry).unwrap();
            }
        }
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(DateTime::default());
        for (name, payload) in extras {
            output.start_file(name, options).unwrap();
            output.write_all(payload).unwrap();
        }
        output.finish().unwrap().into_inner()
    }

    fn package_snapshot(package: &[u8]) -> BTreeMap<String, EntrySnapshot> {
        let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
        let mut entries = BTreeMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let data_start = entry.data_start() as usize;
            let compressed_end = data_start + entry.compressed_size() as usize;
            let snapshot = EntrySnapshot {
                compressed_payload: package[data_start..compressed_end].to_vec(),
                crc32: entry.crc32(),
                compression: entry.compression(),
                modified: entry.last_modified(),
                unix_mode: entry.unix_mode(),
                comment: entry.comment().to_owned(),
                extra_data: entry.extra_data().map(<[u8]>::to_vec),
                payload: {
                    let mut payload = Vec::new();
                    entry.read_to_end(&mut payload).unwrap();
                    payload
                },
            };
            entries.insert(name, snapshot);
        }
        entries
    }

    fn package_entry_order(package: &[u8]) -> Vec<String> {
        let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
        (0..archive.len()).map(|index| archive.by_index(index).unwrap().name().to_owned()).collect()
    }

    fn archive_comment(package: &[u8]) -> Vec<u8> {
        ZipArchive::new(Cursor::new(package)).unwrap().comment().to_vec()
    }

    fn xml_element(xml: &[u8], name: &[u8]) -> Vec<u8> {
        let mut reader = Reader::from_reader(xml);
        let mut writer = Writer::new(Vec::new());
        let mut depth = 0;
        loop {
            let event = reader.read_event().unwrap();
            if depth == 0 {
                match &event {
                    Event::Start(start) if local_name(start.name().as_ref()) == name => depth = 1,
                    Event::Empty(empty) if local_name(empty.name().as_ref()) == name => {
                        writer.write_event(event).unwrap();
                        break;
                    }
                    Event::Eof => {
                        panic!("XML element '{}' not found", String::from_utf8_lossy(name))
                    }
                    _ => continue,
                }
            } else {
                match &event {
                    Event::Start(_) => depth += 1,
                    Event::End(_) => depth -= 1,
                    Event::Eof => {
                        panic!("XML element '{}' is incomplete", String::from_utf8_lossy(name))
                    }
                    _ => {}
                }
            }
            writer.write_event(event).unwrap();
            if depth == 0 {
                break;
            }
        }
        writer.into_inner()
    }

    #[test]
    fn patched_control_parts_remain_well_formed_xml() {
        let source = source_package();
        let donor = DonorBuilder::from_dynamic(
            &[single_value_row("new")],
            &WriteOptions::new().with_sheet_name("New"),
        )
        .unwrap();
        let output = append_worksheet(Cursor::new(source), &donor).unwrap();
        let snapshot = package_snapshot(&output);
        for path in [CONTENT_TYPES_PATH, WORKBOOK_PATH, WORKBOOK_RELS_PATH, "xl/styles.xml"] {
            let mut reader = Reader::from_reader(snapshot[path].payload.as_slice());
            loop {
                if reader.read_event().unwrap() == Event::Eof {
                    break;
                }
            }
        }
    }
}
