use std::io::{BufReader, Cursor, Read, Seek, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};
use serde::Serialize;
use zip::ZipArchive;

use super::package::{DefinedName, PackageInventory};
use crate::writer::XlsxWriter;
#[cfg(not(target_arch = "wasm32"))]
use crate::writer::validate_dimensions;
use crate::{DynamicRow, Error, Result, SheetVisibility, WriteOptions};

const STYLES_PATH: &str = "xl/styles.xml";
const SHARED_STRINGS_PATH: &str = "xl/sharedStrings.xml";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DonorStyleModel {
    pub(crate) xml: Vec<u8>,
    pub(crate) number_formats: usize,
    pub(crate) fonts: usize,
    pub(crate) fills: usize,
    pub(crate) borders: usize,
    pub(crate) cell_style_xfs: usize,
    pub(crate) cell_xfs: usize,
    pub(crate) cell_styles: usize,
    pub(crate) differential_formats: usize,
}

#[derive(Debug)]
pub(crate) struct DonorWorksheet {
    pub(crate) sheet_name: String,
    pub(crate) visibility: SheetVisibility,
    worksheet_xml: tempfile::NamedTempFile,
    pub(crate) data_row_count: usize,
    pub(crate) styles: DonorStyleModel,
    pub(crate) local_defined_names: Vec<DefinedName>,
}

impl DonorWorksheet {
    pub(crate) fn worksheet_reader(&self) -> Result<BufReader<std::fs::File>> {
        Ok(BufReader::new(self.worksheet_xml.reopen()?))
    }

    #[cfg(test)]
    pub(crate) fn worksheet_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.worksheet_reader().unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    }
}

pub(crate) struct DonorBuilder;

impl DonorBuilder {
    pub(crate) fn from_dynamic(
        rows: &[DynamicRow],
        options: &WriteOptions,
    ) -> Result<DonorWorksheet> {
        let mut writer = XlsxWriter::new();
        writer.add_rows(rows, options)?;
        extract_donor(
            writer.save_insert_donor_to_bytes()?,
            rows.len(),
            options.sheet_visibility(options.sheet_name()),
        )
    }

    pub(crate) fn from_dynamic_with_schema(
        schema: &[String],
        rows: &[DynamicRow],
        options: &WriteOptions,
    ) -> Result<DonorWorksheet> {
        let mut writer = XlsxWriter::new();
        writer.add_rows_with_schema(schema, rows, options)?;
        extract_donor(
            writer.save_insert_donor_to_bytes()?,
            rows.len(),
            options.sheet_visibility(options.sheet_name()),
        )
    }

    pub(crate) fn from_serialized<T>(rows: &[T], options: &WriteOptions) -> Result<DonorWorksheet>
    where
        T: Serialize,
    {
        let mut writer = XlsxWriter::new();
        writer.add_serialized(rows, options)?;
        extract_donor(
            writer.save_insert_donor_to_bytes()?,
            rows.len(),
            options.sheet_visibility(options.sheet_name()),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_dynamic_iter<I>(
        schema: &[String],
        rows: I,
        options: &WriteOptions,
    ) -> Result<DonorWorksheet>
    where
        I: IntoIterator<Item = Result<DynamicRow>>,
    {
        build_from_dynamic_iter(schema, rows, options, None)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn build_from_dynamic_iter<I>(
    schema: &[String],
    rows: I,
    options: &WriteOptions,
    spool_directory: Option<&std::path::Path>,
) -> Result<DonorWorksheet>
where
    I: IntoIterator<Item = Result<DynamicRow>>,
{
    let (spool, row_count) =
        spool_dynamic_rows(rows, spool_directory, schema.len(), options.print_header())?;
    let rows = spooled_rows(&spool)?;
    let mut writer = XlsxWriter::new();
    writer.add_rows_iter_with_schema(schema, rows, row_count, options)?;
    let mut package = tempfile::NamedTempFile::new()?;
    writer.save_insert_donor_to_writer(package.as_file_mut())?;
    extract_donor_from_reader(
        package.reopen()?,
        row_count,
        options.sheet_visibility(options.sheet_name()),
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn save_dynamic_iter_to_path<I>(
    path: &Path,
    schema: &[String],
    rows: I,
    options: &WriteOptions,
) -> Result<usize>
where
    I: IntoIterator<Item = Result<DynamicRow>>,
{
    let (spool, row_count) = spool_dynamic_rows(rows, None, schema.len(), options.print_header())?;
    let rows = spooled_rows(&spool)?;
    let mut writer = XlsxWriter::new();
    writer.add_rows_iter_with_schema(schema, rows, row_count, options)?;
    writer.save(path, false)?;
    Ok(row_count)
}

#[cfg(not(target_arch = "wasm32"))]
fn spool_dynamic_rows<I>(
    rows: I,
    spool_directory: Option<&Path>,
    columns: usize,
    print_header: bool,
) -> Result<(tempfile::NamedTempFile, usize)>
where
    I: IntoIterator<Item = Result<DynamicRow>>,
{
    use std::io::Write;

    validate_dimensions(0, columns, print_header)?;
    let mut spool = match spool_directory {
        Some(directory) => tempfile::NamedTempFile::new_in(directory)?,
        None => tempfile::NamedTempFile::new()?,
    };
    let mut row_count = 0;
    for row in rows {
        let row = row?;
        validate_dimensions(row_count + 1, columns, print_header)?;
        serde_json::to_writer(spool.as_file_mut(), &row).map_err(|error| {
            Error::insert_package(format!("cannot spool donor worksheet row: {error}"))
        })?;
        spool.as_file_mut().write_all(b"\n")?;
        row_count += 1;
    }
    spool.as_file_mut().flush()?;
    Ok((spool, row_count))
}

#[cfg(not(target_arch = "wasm32"))]
fn spooled_rows(
    spool: &tempfile::NamedTempFile,
) -> Result<impl Iterator<Item = Result<DynamicRow>>> {
    use std::io::BufReader;

    Ok(serde_json::Deserializer::from_reader(BufReader::new(spool.reopen()?))
        .into_iter::<DynamicRow>()
        .map(|row| {
            row.map_err(|error| {
                Error::insert_package(format!("cannot read donor worksheet spool: {error}"))
            })
        }))
}

pub(super) fn extract_donor(
    bytes: Vec<u8>,
    data_row_count: usize,
    visibility: SheetVisibility,
) -> Result<DonorWorksheet> {
    extract_donor_from_reader(Cursor::new(bytes), data_row_count, visibility)
}

fn extract_donor_from_reader<R>(
    mut source: R,
    data_row_count: usize,
    visibility: SheetVisibility,
) -> Result<DonorWorksheet>
where
    R: Read + std::io::Seek,
{
    let inventory = PackageInventory::inspect(&mut source)?;
    if inventory.sheets.len() != 1 {
        return Err(Error::insert_package(format!(
            "donor workbook contains {} worksheets instead of one",
            inventory.sheets.len()
        )));
    }
    let sheet_name = inventory.sheets[0].name.clone();
    let worksheet_path = inventory.sheets[0].target.clone();
    let local_defined_names =
        inventory.defined_names.into_iter().filter(|name| name.local_sheet_id == Some(0)).collect();

    source.rewind()?;
    let mut archive = ZipArchive::new(source)
        .map_err(|error| Error::insert_package(format!("cannot open donor workbook: {error}")))?;
    let styles = parse_style_model(read_part(&mut archive, STYLES_PATH)?)?;
    let shared_strings = match archive.by_name(SHARED_STRINGS_PATH) {
        Ok(mut entry) => {
            let mut xml = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut xml)?;
            parse_shared_strings(&xml)?
        }
        Err(zip::result::ZipError::FileNotFound) => Vec::new(),
        Err(error) => {
            return Err(Error::insert_package(format!(
                "cannot read donor shared strings: {error}"
            )));
        }
    };
    let mut worksheet_xml = tempfile::NamedTempFile::new()?;
    let mut worksheet_entry = archive.by_name(&worksheet_path).map_err(|error| {
        Error::insert_package(format!("cannot read donor part '{worksheet_path}': {error}"))
    })?;
    if shared_strings.is_empty() {
        std::io::copy(&mut worksheet_entry, worksheet_xml.as_file_mut())?;
    } else {
        inline_shared_strings(
            BufReader::new(&mut worksheet_entry),
            worksheet_xml.as_file_mut(),
            &shared_strings,
        )?;
    }
    worksheet_xml.as_file_mut().flush()?;
    worksheet_xml.as_file_mut().rewind()?;

    Ok(DonorWorksheet {
        sheet_name,
        visibility,
        worksheet_xml,
        data_row_count,
        styles,
        local_defined_names,
    })
}

fn read_part<R>(archive: &mut ZipArchive<R>, path: &str) -> Result<Vec<u8>>
where
    R: Read + std::io::Seek,
{
    let mut entry = archive.by_name(path).map_err(|error| {
        Error::insert_package(format!("cannot read donor part '{path}': {error}"))
    })?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn parse_style_model(xml: Vec<u8>) -> Result<DonorStyleModel> {
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut counts = [0; 8];
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid donor styles XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event) => {
                let count =
                    attribute(&event, b"count")?.and_then(|value| value.parse().ok()).unwrap_or(0);
                match local_name(event.name().as_ref()) {
                    b"numFmts" => counts[0] = count,
                    b"fonts" => counts[1] = count,
                    b"fills" => counts[2] = count,
                    b"borders" => counts[3] = count,
                    b"cellStyleXfs" => counts[4] = count,
                    b"cellXfs" => counts[5] = count,
                    b"cellStyles" => counts[6] = count,
                    b"dxfs" => counts[7] = count,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(DonorStyleModel {
        xml,
        number_formats: counts[0],
        fonts: counts[1],
        fills: counts[2],
        borders: counts[3],
        cell_style_xfs: counts[4],
        cell_xfs: counts[5],
        cell_styles: counts[6],
        differential_formats: counts[7],
    })
}

fn parse_shared_strings(xml: &[u8]) -> Result<Vec<Vec<Event<'static>>>> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut strings = Vec::new();
    let mut current = None::<Vec<Event<'static>>>;
    loop {
        let event = reader.read_event().map_err(|error| {
            Error::insert_package(format!("invalid donor shared strings XML: {error}"))
        })?;
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"si" => {
                if current.is_some() {
                    return Err(Error::insert_package("nested shared string item"));
                }
                current = Some(Vec::new());
            }
            Event::Empty(empty) if local_name(empty.name().as_ref()) == b"si" => {
                strings.push(Vec::new());
            }
            Event::End(end) if local_name(end.name().as_ref()) == b"si" => {
                strings.push(
                    current
                        .take()
                        .ok_or_else(|| Error::insert_package("shared string end without start"))?,
                );
            }
            Event::Eof => break,
            event if current.is_some() => {
                current.as_mut().expect("shared string state").push(event.into_owned());
            }
            _ => {}
        }
    }
    if current.is_some() {
        return Err(Error::insert_package("unterminated shared string item"));
    }
    Ok(strings)
}

fn inline_shared_strings<R, W>(
    xml: R,
    output: W,
    shared_strings: &[Vec<Event<'static>>],
) -> Result<()>
where
    R: std::io::BufRead,
    W: Write,
{
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(output);
    let mut buffer = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            Error::insert_package(format!("invalid donor worksheet XML: {error}"))
        })?;
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"c" => {
                let mut cell = vec![Event::Start(start.into_owned())];
                let mut depth = 1;
                while depth > 0 {
                    buffer.clear();
                    let event = reader.read_event_into(&mut buffer).map_err(|error| {
                        Error::insert_package(format!("invalid donor cell XML: {error}"))
                    })?;
                    match &event {
                        Event::Start(_) => depth += 1,
                        Event::End(_) => depth -= 1,
                        Event::Eof => {
                            return Err(Error::insert_package("unterminated donor cell"));
                        }
                        _ => {}
                    }
                    cell.push(event.into_owned());
                }
                write_cell(&mut writer, &cell, shared_strings)?;
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
        buffer.clear();
    }
    Ok(())
}

fn write_cell<W>(
    writer: &mut Writer<W>,
    events: &[Event<'static>],
    shared_strings: &[Vec<Event<'static>>],
) -> Result<()>
where
    W: Write,
{
    let Event::Start(start) = &events[0] else {
        return Err(Error::insert_package("donor cell has no start event"));
    };
    if attribute(start, b"t")?.as_deref() != Some("s") {
        for event in events {
            write_event(writer, event.clone())?;
        }
        return Ok(());
    }

    let (value_start, value_end, index) = shared_string_reference(events)?;
    let content = shared_strings.get(index).ok_or_else(|| {
        Error::insert_package(format!("donor shared string index {index} is out of range"))
    })?;
    write_event(writer, Event::Start(replace_type_attribute(start, "inlineStr")?))?;
    for (position, event) in events.iter().enumerate().skip(1) {
        if position == value_start {
            write_event(writer, Event::Start(BytesStart::new("is")))?;
            for content_event in content {
                write_event(writer, content_event.clone())?;
            }
            write_event(writer, Event::End(BytesEnd::new("is")))?;
        }
        if position < value_start || position > value_end {
            write_event(writer, event.clone())?;
        }
    }
    Ok(())
}

fn shared_string_reference(events: &[Event<'_>]) -> Result<(usize, usize, usize)> {
    let mut value_start = None;
    let mut value_end = None;
    let mut value = String::new();
    let mut depth = 0;
    for (position, event) in events.iter().enumerate().skip(1) {
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"v" && depth == 0 => {
                value_start = Some(position);
                depth = 1;
            }
            Event::Start(_) if depth > 0 => depth += 1,
            Event::Text(text) if depth > 0 => value.push_str(&text.decode().map_err(|error| {
                Error::insert_package(format!("invalid donor shared string index: {error}"))
            })?),
            Event::End(end) if local_name(end.name().as_ref()) == b"v" && depth == 1 => {
                value_end = Some(position);
                break;
            }
            Event::End(_) if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    let start =
        value_start.ok_or_else(|| Error::insert_package("shared string cell has no value"))?;
    let end =
        value_end.ok_or_else(|| Error::insert_package("shared string value is incomplete"))?;
    let index = value
        .parse()
        .map_err(|_| Error::insert_package(format!("invalid shared string index '{value}'")))?;
    Ok((start, end, index))
}

fn replace_type_attribute(start: &BytesStart<'_>, value: &str) -> Result<BytesStart<'static>> {
    let qualified_name = start.name();
    let name = std::str::from_utf8(qualified_name.as_ref())
        .map_err(|_| Error::insert_package("donor cell name is not UTF-8"))?;
    let mut output = BytesStart::new(name.to_owned());
    for attribute in start.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| {
            Error::insert_package(format!("invalid donor cell attribute: {error}"))
        })?;
        if local_name(attribute.key.as_ref()) == b"t" {
            continue;
        }
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|_| Error::insert_package("donor cell attribute name is not UTF-8"))?;
        let attribute_value = attribute
            .decode_and_unescape_value(start.decoder())
            .map_err(|error| Error::insert_package(format!("invalid donor cell value: {error}")))?;
        output.push_attribute((key, attribute_value.as_ref()));
    }
    output.push_attribute(("t", value));
    Ok(output)
}

fn attribute(event: &BytesStart<'_>, key: &[u8]) -> Result<Option<String>> {
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| Error::insert_package(format!("invalid XML attribute: {error}")))?;
        if local_name(attribute.key.as_ref()) == key {
            return attribute
                .decode_and_unescape_value(event.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| Error::insert_package(format!("invalid XML value: {error}")));
        }
    }
    Ok(None)
}

fn write_event<W>(writer: &mut Writer<W>, event: Event<'_>) -> Result<()>
where
    W: Write,
{
    writer.write_event(event).map_err(|error| {
        Error::insert_package(format!("cannot write donor worksheet XML: {error}"))
    })
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    use serde::Serialize;

    use super::*;
    use crate::CellValue;

    #[derive(Serialize)]
    struct Release {
        name: String,
        version: u32,
    }

    #[test]
    fn donor_sheet_inlines_strings_and_extracts_styles_and_filter_name() {
        let rows = [row("MiniExcel", 2), row("Rust", 1)];
        let options = WriteOptions::new()
            .with_sheet_name("Data")
            .with_column_format("Version", "0.00")
            .with_right_to_left(true)
            .with_freeze_row_count(2);
        let donor = DonorBuilder::from_dynamic(&rows, &options).unwrap();
        let xml = String::from_utf8(donor.worksheet_bytes()).unwrap();

        assert_eq!(donor.data_row_count, 2);
        assert!(xml.contains("t=\"inlineStr\""));
        assert!(xml.contains(">MiniExcel</t>"));
        assert!(!xml.contains("t=\"s\""));
        assert!(xml.contains("rightToLeft=\"1\""));
        assert!(xml.contains("ySplit=\"2\""));
        assert!(donor.styles.cell_xfs >= 2);
        assert!(!donor.styles.xml.is_empty());
        assert_eq!(donor.local_defined_names.len(), 1);
        assert_eq!(donor.local_defined_names[0].name, "_xlnm._FilterDatabase");
        assert_eq!(donor.local_defined_names[0].formula, "Data!$A$1:$B$3");
    }

    #[test]
    fn donor_sheet_supports_explicit_schema_header_only_and_empty_no_header() {
        let schema = vec!["Name".to_owned(), "Version".to_owned()];
        let header_only =
            DonorBuilder::from_dynamic_with_schema(&schema, &[], &WriteOptions::new()).unwrap();
        let xml = String::from_utf8(header_only.worksheet_bytes()).unwrap();
        assert_eq!(header_only.data_row_count, 0);
        assert!(xml.contains(">Name</t>"));
        assert!(xml.contains("<autoFilter ref=\"A1:B1\""));

        let empty = DonorBuilder::from_dynamic_with_schema(
            &[],
            &[],
            &WriteOptions::new().with_print_header(false),
        )
        .unwrap();
        let xml = String::from_utf8(empty.worksheet_bytes()).unwrap();
        assert_eq!(empty.data_row_count, 0);
        assert!(!xml.contains("<row"));
        assert!(empty.local_defined_names.is_empty());
    }

    #[test]
    fn donor_sheet_supports_serde_rows() {
        let rows = [Release { name: "MiniExcel".to_owned(), version: 2 }];
        let donor = DonorBuilder::from_serialized(&rows, &WriteOptions::new()).unwrap();
        let xml = String::from_utf8(donor.worksheet_bytes()).unwrap();
        assert_eq!(donor.data_row_count, 1);
        assert!(xml.contains(">name</t>"));
        assert!(xml.contains(">MiniExcel</t>"));
        assert!(xml.contains(">2</v>"));
    }

    #[test]
    fn shared_string_conversion_preserves_formula_events() {
        let xml = br#"<worksheet><sheetData><row><c r="A1"><f>SUM(A2:A3)</f><v>3</v></c><c r="B1" s="2" t="s"><v>0</v></c></row></sheetData></worksheet>"#;
        let shared =
            parse_shared_strings(br#"<sst><si><t xml:space="preserve"> label </t></si></sst>"#)
                .unwrap();
        let mut output = Vec::new();
        inline_shared_strings(xml.as_slice(), &mut output, &shared).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("<f>SUM(A2:A3)</f><v>3</v>"));
        assert!(output.contains("r=\"B1\" s=\"2\" t=\"inlineStr\""));
        assert!(output.contains("<is><t xml:space=\"preserve\"> label </t></is>"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dynamic_spool_is_one_pass_and_cleans_up_on_all_exit_paths() {
        let schema = vec!["Name".to_owned(), "Version".to_owned()];
        let directory = tempfile::tempdir().unwrap();
        let calls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&calls);
        let rows = (0..3).map(move |version| {
            observed.set(observed.get() + 1);
            Ok(row("MiniExcel", version))
        });
        let donor =
            build_from_dynamic_iter(&schema, rows, &WriteOptions::new(), Some(directory.path()))
                .unwrap();
        assert_eq!(donor.data_row_count, 3);
        assert_eq!(calls.get(), 3);
        assert_directory_empty(directory.path());

        let error_rows = [Ok(row("ok", 1)), Err(std::io::Error::other("producer").into())];
        assert!(
            build_from_dynamic_iter(
                &schema,
                error_rows,
                &WriteOptions::new(),
                Some(directory.path())
            )
            .is_err()
        );
        assert_directory_empty(directory.path());

        let panic_rows = std::iter::once_with(|| -> Result<DynamicRow> { panic!("producer") });
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _ = build_from_dynamic_iter(
                    &schema,
                    panic_rows,
                    &WriteOptions::new(),
                    Some(directory.path()),
                );
            }))
            .is_err()
        );
        assert_directory_empty(directory.path());
    }

    fn row(name: &str, version: u32) -> DynamicRow {
        let mut row = DynamicRow::new();
        row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
        row.insert("Version".to_owned(), CellValue::Int(i64::from(version)));
        row
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn assert_directory_empty(path: &std::path::Path) {
        assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
    }
}
