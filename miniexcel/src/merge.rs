use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[cfg(not(target_arch = "wasm32"))]
use crate::MergeSameCellsOptions;
use crate::{Error, Result};

#[derive(Clone, Debug)]
struct CellInfo {
    column: String,
    row: usize,
    value: String,
}

struct WorksheetRow {
    row: usize,
    events: Vec<Event<'static>>,
    cells: Vec<CellInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MergeRange {
    start_column: String,
    start_row: usize,
    end_column: String,
    end_row: usize,
}

pub(crate) fn merge_same_cells_bytes(workbook: &[u8]) -> Result<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(workbook))
        .map_err(|error| Error::template(format!("cannot open merge workbook: {error}")))?;
    let shared_strings = crate::template::read_shared_strings(&mut archive)?;
    let (control_replacements, removed_entries) =
        crate::template::calculation_metadata(&mut archive)?;
    let output = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(output);

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| Error::template(format!("cannot read merge entry: {error}")))?;
        let name = entry.name().to_owned();
        if removed_entries.contains(&name) {
            continue;
        }
        if let Some(replacement) = control_replacements.get(&name) {
            writer
                .start_file(name, entry.options())
                .map_err(|error| Error::template(format!("cannot replace merge entry: {error}")))?;
            writer.write_all(replacement)?;
        } else if crate::template::is_worksheet(&name) {
            let compression = entry.compression();
            let modified = entry.last_modified();
            let permissions = entry.unix_mode();
            let mut xml = Vec::new();
            entry.read_to_end(&mut xml)?;
            drop(entry);
            let merged = rewrite_worksheet(&xml, &shared_strings)?;
            let mut options = SimpleFileOptions::default().compression_method(compression);
            if let Some(modified) = modified {
                options = options.last_modified_time(modified);
            }
            if let Some(permissions) = permissions {
                options = options.unix_permissions(permissions);
            }
            writer.start_file(name, options).map_err(|error| {
                Error::template(format!("cannot create merged worksheet entry: {error}"))
            })?;
            writer.write_all(&merged)?;
        } else {
            writer
                .raw_copy_file(entry)
                .map_err(|error| Error::template(format!("cannot copy merge entry: {error}")))?;
        }
    }

    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|error| Error::template(format!("cannot finish merged workbook: {error}")))
}

fn rewrite_worksheet(xml: &[u8], shared_strings: &[String]) -> Result<Vec<u8>> {
    let (prefix, rows, suffix, element_prefix) = parse_worksheet(xml, shared_strings)?;
    let cells = rows.iter().flat_map(|row| row.cells.iter().cloned()).collect::<Vec<_>>();
    let removed_rows = rows
        .iter()
        .filter(|row| {
            row.cells.iter().any(|cell| matches!(cell.value.as_str(), "@merge" | "@endmerge"))
        })
        .map(|row| row.row)
        .collect::<BTreeSet<_>>();
    let calculated = calculate_merge_ranges(&cells)?;
    let (suffix, existing, insertion) = strip_merge_cells(suffix)?;
    let mut ranges = existing
        .into_iter()
        .filter_map(|range| shift_existing_range(range, &removed_rows))
        .collect::<Vec<_>>();
    ranges.extend(calculated.into_iter().map(|range| shift_range(range, &removed_rows)));

    let mut output = Writer::new(Vec::new());
    for event in prefix {
        output.write_event(event).map_err(merge_xml_error)?;
    }
    for row in rows {
        if removed_rows.contains(&row.row) {
            continue;
        }
        let target_row = shifted_row(row.row, &removed_rows);
        for event in rewrite_row(row.events, target_row)? {
            output.write_event(event).map_err(merge_xml_error)?;
        }
    }
    for (index, event) in suffix.into_iter().enumerate() {
        if index == insertion && !ranges.is_empty() {
            write_merge_cells(&mut output, &element_prefix, &ranges)?;
        }
        output.write_event(event).map_err(merge_xml_error)?;
    }
    Ok(output.into_inner())
}

type WorksheetParts = (Vec<Event<'static>>, Vec<WorksheetRow>, Vec<Event<'static>>, String);

fn parse_worksheet(xml: &[u8], shared_strings: &[String]) -> Result<WorksheetParts> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut prefix = Vec::new();
    let mut rows = Vec::new();
    let mut suffix = Vec::new();
    let mut current_row = None::<Vec<Event<'static>>>;
    let mut seen_sheet_data = false;
    let mut element_prefix = String::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|error| Error::template(format!("invalid merge worksheet XML: {error}")))?;
        if matches!(event, Event::Eof) {
            break;
        }
        if let Some(row) = current_row.as_mut() {
            let is_end =
                matches!(&event, Event::End(end) if local_name(end.name().as_ref()) == b"row");
            row.push(event.into_owned());
            if is_end {
                let events = current_row.take().expect("row is present");
                rows.push(parse_row(events, shared_strings)?);
            }
        } else if matches!(&event, Event::Start(start) if local_name(start.name().as_ref()) == b"row")
        {
            current_row = Some(vec![event.into_owned()]);
        } else if !seen_sheet_data {
            if let Event::Start(start) = &event {
                if local_name(start.name().as_ref()) == b"sheetData" {
                    seen_sheet_data = true;
                    element_prefix = xml_prefix(start.name().as_ref());
                }
            }
            prefix.push(event.into_owned());
        } else {
            suffix.push(event.into_owned());
        }
    }

    if !seen_sheet_data {
        return Err(Error::template("worksheet does not contain sheetData"));
    }
    Ok((prefix, rows, suffix, element_prefix))
}

fn parse_row(events: Vec<Event<'static>>, shared_strings: &[String]) -> Result<WorksheetRow> {
    let row = events
        .first()
        .and_then(|event| match event {
            Event::Start(start) => attribute(start, b"r"),
            _ => None,
        })
        .ok_or_else(|| Error::template("worksheet row is missing its r attribute"))?
        .parse::<usize>()
        .map_err(|_| Error::template("worksheet row has an invalid r attribute"))?;
    let mut cells = Vec::new();
    let mut index = 0;
    while index < events.len() {
        let Event::Start(start) = &events[index] else {
            index += 1;
            continue;
        };
        if local_name(start.name().as_ref()) != b"c" {
            index += 1;
            continue;
        }
        let end = cell_end(&events, index)?;
        let address = attribute(start, b"r")
            .ok_or_else(|| Error::template("worksheet cell is missing its r attribute"))?;
        let (column, address_row) = split_cell_address(&address)?;
        if address_row != row {
            return Err(Error::template(format!(
                "cell '{address}' does not belong to worksheet row {row}"
            )));
        }
        if let Some(value) = cell_value(&events[index..=end], shared_strings)? {
            if !value.is_empty() {
                cells.push(CellInfo { column, row, value });
            }
        }
        index = end + 1;
    }
    Ok(WorksheetRow { row, events, cells })
}

fn calculate_merge_ranges(cells: &[CellInfo]) -> Result<Vec<MergeRange>> {
    let starts = cells.iter().filter(|cell| cell.value.contains("@merge")).collect::<Vec<_>>();
    let ends = cells.iter().filter(|cell| cell.value.contains("@endmerge")).collect::<Vec<_>>();
    let limit = starts.iter().find(|cell| cell.value.contains("@mergelimit")).copied();
    let mut candidates = Vec::new();
    for start in &starts {
        let Some(end) = ends.iter().find(|end| end.column == start.column && end.row > start.row)
        else {
            continue;
        };
        candidates.extend(
            cells
                .iter()
                .filter(|cell| {
                    cell.column == start.column && cell.row > start.row && cell.row < end.row
                })
                .cloned(),
        );
    }

    let mut ranges = Vec::new();
    let mut last_by_column = BTreeMap::<String, (usize, usize)>::new();
    for cell in cells {
        let mut equal = candidates
            .iter()
            .filter(|candidate| candidate.column == cell.column && candidate.value == cell.value)
            .collect::<Vec<_>>();
        equal.sort_by_key(|candidate| candidate.row);
        if equal.len() <= 1 {
            continue;
        }
        if let Some(limit) = limit {
            let limit_cell = candidates
                .iter()
                .find(|candidate| candidate.column == limit.column && candidate.row == cell.row)
                .ok_or_else(|| {
                    Error::template(format!(
                        "@mergelimit column '{}' has no value at row {}",
                        limit.column, cell.row
                    ))
                })?;
            let limit_end = candidates
                .iter()
                .rev()
                .find(|candidate| {
                    candidate.column == limit.column && candidate.value == limit_cell.value
                })
                .expect("limit cell is a candidate");
            equal.retain(|candidate| {
                candidate.row >= limit_cell.row && candidate.row <= limit_end.row
            });
        }
        if equal.len() <= 1 {
            continue;
        }
        let first = equal[0];
        let Some(last) = equal.iter().rev().find(|candidate| {
            candidate.row <= first.row + equal.len() && candidate.row != first.row
        }) else {
            continue;
        };
        let range = MergeRange {
            start_column: first.column.clone(),
            start_row: first.row,
            end_column: last.column.clone(),
            end_row: last.row,
        };
        let should_add = last_by_column
            .get(&range.start_column)
            .is_none_or(|previous| range.start_row < previous.0 || range.end_row > previous.1);
        if should_add {
            last_by_column.insert(range.start_column.clone(), (range.start_row, range.end_row));
            ranges.push(range);
        }
    }
    Ok(ranges)
}

fn rewrite_row(events: Vec<Event<'static>>, target_row: usize) -> Result<Vec<Event<'static>>> {
    events
        .into_iter()
        .map(|event| match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"row" => {
                Ok(Event::Start(replace_attribute(&start, b"r", &target_row.to_string())))
            }
            Event::Empty(start) if local_name(start.name().as_ref()) == b"row" => {
                Ok(Event::Empty(replace_attribute(&start, b"r", &target_row.to_string())))
            }
            Event::Start(start) if local_name(start.name().as_ref()) == b"c" => {
                Ok(Event::Start(rewrite_cell_address(&start, target_row)?))
            }
            Event::Empty(start) if local_name(start.name().as_ref()) == b"c" => {
                Ok(Event::Empty(rewrite_cell_address(&start, target_row)?))
            }
            event => Ok(event),
        })
        .collect()
}

fn rewrite_cell_address(start: &BytesStart<'_>, target_row: usize) -> Result<BytesStart<'static>> {
    let address = attribute(start, b"r")
        .ok_or_else(|| Error::template("worksheet cell is missing its r attribute"))?;
    let (column, _) = split_cell_address(&address)?;
    Ok(replace_attribute(start, b"r", &format!("{column}{target_row}")))
}

fn strip_merge_cells(
    suffix: Vec<Event<'static>>,
) -> Result<(Vec<Event<'static>>, Vec<MergeRange>, usize)> {
    let mut output = Vec::new();
    let mut ranges = Vec::new();
    let mut skip_depth = 0_usize;
    let mut existing_position = None;
    for event in suffix {
        if skip_depth > 0 {
            if let Event::Start(start) | Event::Empty(start) = &event {
                if local_name(start.name().as_ref()) == b"mergeCell" {
                    if let Some(reference) = attribute(start, b"ref") {
                        ranges.push(parse_merge_range(&reference)?);
                    }
                }
            }
            match event {
                Event::Start(_) => skip_depth += 1,
                Event::End(_) => skip_depth = skip_depth.saturating_sub(1),
                _ => {}
            }
            continue;
        }
        match &event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"mergeCells" => {
                existing_position = Some(output.len());
                skip_depth = 1;
            }
            Event::Empty(start) if local_name(start.name().as_ref()) == b"mergeCells" => {
                existing_position = Some(output.len());
            }
            _ => output.push(event),
        }
    }
    let insertion = existing_position.unwrap_or_else(|| merge_insertion_point(&output));
    Ok((output, ranges, insertion))
}

fn merge_insertion_point(events: &[Event<'static>]) -> usize {
    const BEFORE_MERGES: &[&[u8]] = &[
        b"sheetCalcPr",
        b"sheetProtection",
        b"protectedRanges",
        b"scenarios",
        b"autoFilter",
        b"sortState",
        b"dataConsolidate",
        b"customSheetViews",
    ];
    let mut depth = 0_usize;
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Start(start) if depth == 0 => {
                if !BEFORE_MERGES.contains(&local_name(start.name().as_ref())) {
                    return index;
                }
                depth = 1;
            }
            Event::Start(_) => depth += 1,
            Event::End(end) if depth == 0 && local_name(end.name().as_ref()) == b"worksheet" => {
                return index;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Empty(start) if depth == 0 => {
                if !BEFORE_MERGES.contains(&local_name(start.name().as_ref())) {
                    return index;
                }
            }
            _ => {}
        }
    }
    events.len()
}

fn write_merge_cells(
    writer: &mut Writer<Vec<u8>>,
    prefix: &str,
    ranges: &[MergeRange],
) -> Result<()> {
    let container_name = format!("{prefix}mergeCells");
    let cell_name = format!("{prefix}mergeCell");
    let mut container = BytesStart::new(container_name.as_str());
    let count = ranges.len().to_string();
    container.push_attribute(("count", count.as_str()));
    writer.write_event(Event::Start(container)).map_err(merge_xml_error)?;
    for range in ranges {
        let mut cell = BytesStart::new(cell_name.as_str());
        let reference = format_merge_range(range);
        cell.push_attribute(("ref", reference.as_str()));
        writer.write_event(Event::Empty(cell)).map_err(merge_xml_error)?;
    }
    writer.write_event(Event::End(BytesEnd::new(container_name.as_str()))).map_err(merge_xml_error)
}

fn shift_range(range: MergeRange, removed: &BTreeSet<usize>) -> MergeRange {
    MergeRange {
        start_row: shifted_row(range.start_row, removed),
        end_row: shifted_row(range.end_row, removed),
        ..range
    }
}

fn shift_existing_range(range: MergeRange, removed: &BTreeSet<usize>) -> Option<MergeRange> {
    let start = (range.start_row..=range.end_row).find(|row| !removed.contains(row))?;
    let end = (range.start_row..=range.end_row).rev().find(|row| !removed.contains(row))?;
    let shifted = MergeRange {
        start_column: range.start_column,
        start_row: shifted_row(start, removed),
        end_column: range.end_column,
        end_row: shifted_row(end, removed),
    };
    (shifted.start_column != shifted.end_column || shifted.start_row != shifted.end_row)
        .then_some(shifted)
}

fn shifted_row(row: usize, removed: &BTreeSet<usize>) -> usize {
    row - removed.range(..row).count()
}

fn cell_end(events: &[Event<'_>], start: usize) -> Result<usize> {
    let mut depth = 0_usize;
    for (index, event) in events.iter().enumerate().skip(start) {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err(Error::template("worksheet cell is not closed"))
}

fn cell_value(events: &[Event<'_>], shared_strings: &[String]) -> Result<Option<String>> {
    let Event::Start(cell) = &events[0] else {
        return Ok(None);
    };
    let cell_type = attribute(cell, b"t").unwrap_or_default();
    let mut value = String::new();
    let mut in_value = false;
    let mut in_inline_text = false;
    for event in &events[1..] {
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"v" => in_value = true,
            Event::End(end) if local_name(end.name().as_ref()) == b"v" => in_value = false,
            Event::Start(start) if local_name(start.name().as_ref()) == b"t" => {
                in_inline_text = true;
            }
            Event::End(end) if local_name(end.name().as_ref()) == b"t" => {
                in_inline_text = false;
            }
            Event::Text(text) if in_value || in_inline_text => {
                value.push_str(&text.xml10_content().map_err(|error| {
                    Error::template(format!("invalid merge cell text: {error}"))
                })?)
            }
            Event::GeneralRef(reference) if in_value || in_inline_text => {
                append_reference(reference, &mut value)?;
            }
            _ => {}
        }
    }
    if value.is_empty() {
        return Ok(None);
    }
    if cell_type == "s" {
        let index = value
            .parse::<usize>()
            .map_err(|_| Error::template("invalid shared string index in merge worksheet"))?;
        Ok(shared_strings.get(index).cloned())
    } else {
        Ok(Some(value))
    }
}

fn append_reference(
    reference: &quick_xml::events::BytesRef<'_>,
    target: &mut String,
) -> Result<()> {
    let decoded = reference
        .decode()
        .map_err(|error| Error::template(format!("invalid XML reference: {error}")))?;
    match decoded.as_ref() {
        "lt" => target.push('<'),
        "gt" => target.push('>'),
        "amp" => target.push('&'),
        "quot" => target.push('"'),
        "apos" => target.push('\''),
        _ => {
            if let Some(value) = reference
                .resolve_char_ref()
                .map_err(|error| Error::template(format!("invalid XML reference: {error}")))?
            {
                target.push(value);
            }
        }
    }
    Ok(())
}

fn parse_merge_range(reference: &str) -> Result<MergeRange> {
    let (start, end) = reference.split_once(':').unwrap_or((reference, reference));
    let (start_column, start_row) = split_cell_address(start)?;
    let (end_column, end_row) = split_cell_address(end)?;
    Ok(MergeRange { start_column, start_row, end_column, end_row })
}

fn split_cell_address(address: &str) -> Result<(String, usize)> {
    let split = address
        .find(|character: char| character.is_ascii_digit())
        .ok_or_else(|| Error::template(format!("invalid worksheet cell reference '{address}'")))?;
    let column = &address[..split];
    let row = address[split..]
        .parse::<usize>()
        .map_err(|_| Error::template(format!("invalid worksheet cell reference '{address}'")))?;
    if column.is_empty()
        || row == 0
        || !column.chars().all(|character| character.is_ascii_alphabetic())
    {
        return Err(Error::template(format!("invalid worksheet cell reference '{address}'")));
    }
    Ok((column.to_owned(), row))
}

fn format_merge_range(range: &MergeRange) -> String {
    format!("{}{}:{}{}", range.start_column, range.start_row, range.end_column, range.end_row)
}

fn replace_attribute(start: &BytesStart<'_>, key: &[u8], value: &str) -> BytesStart<'static> {
    let mut output = BytesStart::new(String::from_utf8_lossy(start.name().as_ref()).into_owned());
    for attribute in start.attributes().with_checks(false).flatten() {
        if attribute.key.as_ref() != key {
            output.push_attribute((attribute.key.as_ref(), attribute.value.as_ref()));
        }
    }
    output.push_attribute((key, value.as_bytes()));
    output.into_owned()
}

fn attribute(start: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    start
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| attribute.key.as_ref() == key)
        .map(|attribute| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn xml_prefix(name: &[u8]) -> String {
    name.iter()
        .rposition(|byte| *byte == b':')
        .map_or_else(String::new, |index| String::from_utf8_lossy(&name[..=index]).into_owned())
}

fn merge_xml_error(error: std::io::Error) -> Error {
    Error::template(format!("cannot write merged worksheet XML: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn merge_same_cells_path(
    source: &std::path::Path,
    destination: &std::path::Path,
    options: &MergeSameCellsOptions,
) -> Result<()> {
    crate::insert::atomic::transform_to_path(
        source,
        destination,
        options.overwrite_file(),
        "merge-same-cells",
        |source, destination| {
            let mut workbook = Vec::new();
            source.read_to_end(&mut workbook)?;
            let merged = merge_same_cells_bytes(&workbook)?;
            destination.write_all(&merged)?;
            Ok(())
        },
    )
}
