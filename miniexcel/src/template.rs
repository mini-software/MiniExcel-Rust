use std::fs::OpenOptions;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::Serialize;
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::{Error, Result, TemplateOptions};

pub(crate) fn fill_path<T>(
    output_path: impl AsRef<Path>,
    template_path: impl AsRef<Path>,
    value: &T,
    options: &TemplateOptions,
) -> Result<()>
where
    T: Serialize,
{
    let template = std::fs::read(template_path)?;
    let output = fill_bytes(&template, value, options)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .create_new(!options.overwrite_file())
        .truncate(options.overwrite_file())
        .open(output_path)?;
    file.write_all(&output)?;
    Ok(())
}

pub(crate) fn fill_bytes<T>(
    template: &[u8],
    value: &T,
    options: &TemplateOptions,
) -> Result<Vec<u8>>
where
    T: Serialize,
{
    let value = serialize_value(value)?;
    fill_bytes_value(template, &value, options)
}

pub(crate) fn serialize_value<T>(value: &T) -> Result<Value>
where
    T: Serialize,
{
    serde_json::to_value(value)
        .map_err(|error| Error::template(format!("cannot serialize template data: {error}")))
}

pub(crate) fn fill_bytes_value(
    template: &[u8],
    value: &Value,
    options: &TemplateOptions,
) -> Result<Vec<u8>> {
    fill_bytes_value_with_check(template, value, options, &mut || Ok(()))
}

#[cfg(all(feature = "async", not(target_arch = "wasm32")))]
pub(crate) fn fill_path_value_to_writer<W, F>(
    writer: &mut W,
    template_path: &Path,
    value: &Value,
    options: &TemplateOptions,
    check: &mut F,
) -> Result<()>
where
    W: Write,
    F: FnMut() -> Result<()>,
{
    check()?;
    let template = std::fs::read(template_path)?;
    check()?;
    let output = fill_bytes_value_with_check(&template, value, options, check)?;
    check()?;
    writer.write_all(&output)?;
    Ok(())
}

fn fill_bytes_value_with_check<F>(
    template: &[u8],
    value: &Value,
    options: &TemplateOptions,
    check: &mut F,
) -> Result<Vec<u8>>
where
    F: FnMut() -> Result<()>,
{
    let mut archive = ZipArchive::new(Cursor::new(template))
        .map_err(|error| Error::template(format!("cannot open template workbook: {error}")))?;
    let shared_strings = read_shared_strings(&mut archive)?;
    let output = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(output);

    for index in 0..archive.len() {
        check()?;
        let mut entry = archive
            .by_index(index)
            .map_err(|error| Error::template(format!("cannot read template entry: {error}")))?;
        let name = entry.name().to_owned();
        if is_worksheet(&name) {
            let compression = entry.compression();
            let modified = entry.last_modified();
            let permissions = entry.unix_mode();
            let mut xml = Vec::new();
            entry.read_to_end(&mut xml)?;
            drop(entry);
            let rendered = render_worksheet(
                &xml,
                &shared_strings,
                value,
                options.ignore_missing_variables(),
                check,
            )?;
            let mut file_options = SimpleFileOptions::default().compression_method(compression);
            if let Some(modified) = modified {
                file_options = file_options.last_modified_time(modified);
            }
            if let Some(permissions) = permissions {
                file_options = file_options.unix_permissions(permissions);
            }
            writer
                .start_file(name, file_options)
                .map_err(|error| Error::template(format!("cannot create output entry: {error}")))?;
            writer.write_all(&rendered)?;
        } else {
            writer
                .raw_copy_file(entry)
                .map_err(|error| Error::template(format!("cannot copy template entry: {error}")))?;
        }
    }

    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|error| Error::template(format!("cannot finish template workbook: {error}")))
}

fn is_worksheet(name: &str) -> bool {
    name.starts_with("xl/worksheets/") && name.ends_with(".xml")
}

fn read_shared_strings<R>(archive: &mut ZipArchive<R>) -> Result<Vec<String>>
where
    R: Read + std::io::Seek,
{
    let Ok(mut entry) = archive.by_name("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    let mut xml = Vec::new();
    entry.read_to_end(&mut xml)?;
    let mut reader = Reader::from_reader(xml.as_slice());
    reader.config_mut().trim_text(false);
    let mut values = Vec::new();
    let mut current = None::<String>;
    let mut in_text = false;
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::template(format!("invalid shared strings XML: {error}")))?
        {
            Event::Start(event) if event.name().as_ref() == b"si" => {
                current = Some(String::new());
            }
            Event::End(event) if event.name().as_ref() == b"si" => {
                values.push(current.take().unwrap_or_default());
            }
            Event::Start(event) if event.name().as_ref() == b"t" => in_text = true,
            Event::End(event) if event.name().as_ref() == b"t" => in_text = false,
            Event::Text(text) if in_text => {
                if let Some(current) = current.as_mut() {
                    current.push_str(&text.xml10_content().map_err(|error| {
                        Error::template(format!("invalid shared string text: {error}"))
                    })?);
                }
            }
            Event::GeneralRef(reference) if in_text => {
                if let Some(current) = current.as_mut() {
                    append_reference(&reference, current)?;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(values)
}

fn render_worksheet(
    xml: &[u8],
    shared_strings: &[String],
    data: &Value,
    ignore_missing: bool,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut prefix = Vec::new();
    let mut rows = Vec::<Vec<Event<'static>>>::new();
    let mut suffix = Vec::new();
    let mut current_row = None::<Vec<Event<'static>>>;
    let mut seen_sheet_data = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|error| Error::template(format!("invalid worksheet XML: {error}")))?;
        if matches!(event, Event::Eof) {
            break;
        }
        if let Some(row) = current_row.as_mut() {
            let is_row_end = matches!(&event, Event::End(end) if end.name().as_ref() == b"row");
            row.push(event.into_owned());
            if is_row_end {
                rows.push(current_row.take().expect("row is present"));
            }
        } else if matches!(&event, Event::Start(start) if start.name().as_ref() == b"row") {
            current_row = Some(vec![event.into_owned()]);
        } else if !seen_sheet_data {
            if matches!(&event, Event::Start(start) if start.name().as_ref() == b"sheetData") {
                seen_sheet_data = true;
            }
            prefix.push(event.into_owned());
        } else {
            suffix.push(event.into_owned());
        }
    }

    if !seen_sheet_data {
        return Err(Error::template("worksheet does not contain sheetData"));
    }

    let has_groups = rows
        .iter()
        .map(|row| row_group_marker(row, shared_strings))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .any(|marker| marker.is_some());
    if has_groups
        && suffix.iter().any(|event| {
            matches!(event, Event::Start(start) | Event::Empty(start) if start.name().as_ref() == b"mergeCell")
        })
    {
        return Err(Error::template(
            "grouped template blocks do not support merged cells",
        ));
    }

    let mut rendered_rows = Vec::new();
    let mut shift = 0_isize;
    let mut previous_source_row = 0_usize;
    let mut max_output_row = 1_usize;
    let mut row_index = 0_usize;
    while row_index < rows.len() {
        check()?;
        let row = &rows[row_index];
        let source_row = row_number(row).unwrap_or(previous_source_row.saturating_add(1));
        previous_source_row = source_row;
        match row_group_marker(row, shared_strings)? {
            Some(GroupMarker::Start) => {
                let end_index = find_group_end(&rows, row_index + 1, shared_strings)?;
                let block = &rows[row_index + 1..end_index];
                let (root, values, header_row) = group_descriptor(block, shared_strings, data)?;
                if block.iter().any(|row| row_has_formula(row)) {
                    return Err(Error::template(
                        "grouped template blocks do not support formula cells",
                    ));
                }
                let output_start = shifted_row(source_row, shift)?;
                let mut emitted = 0_usize;
                let mut previous_header = None::<String>;
                for (item_index, item) in values.iter().enumerate() {
                    for (block_index, block_row) in block.iter().enumerate() {
                        if Some(block_index) == header_row {
                            let key = group_header_key(
                                block_row,
                                shared_strings,
                                data,
                                &root,
                                item,
                                item_index,
                                ignore_missing,
                            )?;
                            if previous_header.as_deref() == Some(&key) {
                                continue;
                            }
                            previous_header = Some(key);
                        }
                        let target_row = output_start.checked_add(emitted).ok_or_else(|| {
                            Error::template("grouped template row index overflow")
                        })?;
                        rendered_rows.push(render_row(
                            block_row,
                            target_row,
                            shared_strings,
                            data,
                            Some(&root),
                            Some(item),
                            item_index,
                            ignore_missing,
                            Some(block_index) == header_row,
                        )?);
                        emitted += 1;
                        max_output_row = max_output_row.max(target_row);
                    }
                }
                let end_source_row = row_number(&rows[end_index])
                    .unwrap_or(source_row.saturating_add(end_index - row_index));
                let consumed = end_source_row.saturating_sub(source_row).saturating_add(1);
                shift = shift
                    .checked_add(emitted as isize - consumed as isize)
                    .ok_or_else(|| Error::template("grouped template row shift overflow"))?;
                previous_source_row = end_source_row;
                row_index = end_index + 1;
                continue;
            }
            Some(GroupMarker::End) => {
                return Err(Error::template("template contains unmatched @endgroup"));
            }
            None => {}
        }
        let output_row = shifted_row(source_row, shift)?;
        let expansion = expansion_for_row(row, shared_strings, data)?;
        let items: Vec<Option<&Value>> = match expansion.as_ref() {
            Some((_, values)) if !values.is_empty() => values.iter().map(Some).collect(),
            Some(_) => vec![None],
            None => vec![None],
        };
        for (item_index, item) in items.iter().enumerate() {
            let target_row = output_row.saturating_add(item_index);
            rendered_rows.push(render_row(
                row,
                target_row,
                shared_strings,
                data,
                expansion.as_ref().map(|(root, _)| root.as_str()),
                *item,
                item_index,
                ignore_missing,
                false,
            )?);
            max_output_row = max_output_row.max(target_row);
        }
        shift = shift
            .checked_add(items.len().saturating_sub(1) as isize)
            .ok_or_else(|| Error::template("template row shift overflow"))?;
        row_index += 1;
    }

    update_dimension(&mut prefix, max_output_row)?;
    let mut output = Writer::new(Vec::with_capacity(xml.len()));
    for event in prefix.into_iter().chain(rendered_rows.into_iter().flatten()).chain(suffix) {
        output
            .write_event(event)
            .map_err(|error| Error::template(format!("cannot write worksheet XML: {error}")))?;
    }
    Ok(output.into_inner())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GroupMarker {
    Start,
    End,
}

fn row_group_marker(row: &[Event<'_>], shared_strings: &[String]) -> Result<Option<GroupMarker>> {
    let texts = cell_texts(row, shared_strings)?;
    let marker = texts.iter().find_map(|text| match text.trim() {
        "@group" => Some(GroupMarker::Start),
        "@endgroup" => Some(GroupMarker::End),
        _ => None,
    });
    if marker.is_some()
        && texts.iter().any(|text| {
            let text = text.trim();
            !text.is_empty() && !matches!(text, "@group" | "@endgroup")
        })
    {
        return Err(Error::template("group marker rows cannot contain other text"));
    }
    Ok(marker)
}

fn find_group_end(
    rows: &[Vec<Event<'static>>],
    start: usize,
    shared_strings: &[String],
) -> Result<usize> {
    for (index, row) in rows.iter().enumerate().skip(start) {
        match row_group_marker(row, shared_strings)? {
            Some(GroupMarker::Start) => {
                return Err(Error::template("nested @group blocks are not supported"));
            }
            Some(GroupMarker::End) => {
                if index == start {
                    return Err(Error::template("grouped template block cannot be empty"));
                }
                return Ok(index);
            }
            None => {}
        }
    }
    Err(Error::template("template @group block is missing @endgroup"))
}

fn group_descriptor<'a>(
    rows: &[Vec<Event<'static>>],
    shared_strings: &[String],
    data: &'a Value,
) -> Result<(String, &'a Vec<Value>, Option<usize>)> {
    let mut root = None::<String>;
    let mut header_row = None;
    for (row_index, row) in rows.iter().enumerate() {
        for text in cell_texts(row, shared_strings)? {
            if text.starts_with("@header") && header_row.replace(row_index).is_some() {
                return Err(Error::template(
                    "grouped template block can contain only one @header row",
                ));
            }
            for token in placeholder_tokens(&text) {
                let Some((candidate, _)) = token.split_once('.') else {
                    continue;
                };
                if data.get(candidate).is_some_and(Value::is_array) {
                    if root.as_deref().is_some_and(|root| root != candidate) {
                        return Err(Error::template(
                            "grouped template block cannot use multiple array roots",
                        ));
                    }
                    root = Some(candidate.to_owned());
                }
            }
        }
    }
    let root =
        root.ok_or_else(|| Error::template("grouped template block does not reference an array"))?;
    let values = data.get(&root).and_then(Value::as_array).expect("validated array root");
    if values.is_empty() {
        return Err(Error::template("grouped template array cannot be empty"));
    }
    Ok((root, values, header_row))
}

fn row_has_formula(row: &[Event<'_>]) -> bool {
    row.iter().any(|event| {
        matches!(event, Event::Start(start) | Event::Empty(start) if start.name().as_ref() == b"f")
    })
}

fn shifted_row(source_row: usize, shift: isize) -> Result<usize> {
    source_row
        .checked_add_signed(shift)
        .filter(|row| *row > 0)
        .ok_or_else(|| Error::template("template row index is outside the worksheet"))
}

#[allow(clippy::too_many_arguments)]
fn group_header_key(
    row: &[Event<'_>],
    shared_strings: &[String],
    data: &Value,
    collection_root: &str,
    item: &Value,
    item_index: usize,
    ignore_missing: bool,
) -> Result<String> {
    let mut key = String::new();
    for text in cell_texts(row, shared_strings)? {
        if let Some(template) = text.strip_prefix("@header") {
            let rendered = render_text(
                template,
                data,
                Some(collection_root),
                Some(item),
                item_index,
                ignore_missing,
            )?;
            key.push_str(&rendered_value_text(&rendered));
        }
    }
    Ok(key)
}

fn rendered_value_text(value: &RenderedValue) -> String {
    match value {
        RenderedValue::Empty => String::new(),
        RenderedValue::Bool(value) => value.to_string(),
        RenderedValue::Number(value) | RenderedValue::String(value) => value.clone(),
    }
}

fn row_number(events: &[Event<'_>]) -> Option<usize> {
    let Event::Start(row) = events.first()? else {
        return None;
    };
    attribute(row, b"r").and_then(|value| value.parse().ok())
}

fn expansion_for_row<'a>(
    row: &[Event<'_>],
    shared_strings: &[String],
    data: &'a Value,
) -> Result<Option<(String, &'a Vec<Value>)>> {
    for text in cell_texts(row, shared_strings)? {
        for token in placeholder_tokens(&text) {
            let Some((root, _)) = token.split_once('.') else {
                continue;
            };
            if let Some(values) = data.get(root).and_then(Value::as_array) {
                return Ok(Some((root.to_owned(), values)));
            }
        }
    }
    Ok(None)
}

fn cell_texts(row: &[Event<'_>], shared_strings: &[String]) -> Result<Vec<String>> {
    let mut texts = Vec::new();
    let mut index = 0;
    while index < row.len() {
        if matches!(&row[index], Event::Start(start) if start.name().as_ref() == b"c") {
            let end = cell_end(row, index)?;
            if let Some(text) = cell_text(&row[index..=end], shared_strings)? {
                texts.push(text);
            }
            index = end + 1;
        } else {
            index += 1;
        }
    }
    Ok(texts)
}

#[allow(clippy::too_many_arguments)]
fn render_row(
    row: &[Event<'_>],
    target_row: usize,
    shared_strings: &[String],
    data: &Value,
    collection_root: Option<&str>,
    item: Option<&Value>,
    item_index: usize,
    ignore_missing: bool,
    group_header: bool,
) -> Result<Vec<Event<'static>>> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < row.len() {
        match &row[index] {
            Event::Start(start) if start.name().as_ref() == b"row" => {
                output.push(Event::Start(replace_attribute(start, b"r", &target_row.to_string())));
                index += 1;
            }
            Event::Start(start) if start.name().as_ref() == b"c" => {
                let end = cell_end(row, index)?;
                let address = attribute(start, b"r").unwrap_or_else(|| format!("A{target_row}"));
                let column =
                    address.chars().take_while(char::is_ascii_alphabetic).collect::<String>();
                let address = format!("{column}{target_row}");
                if let Some(mut text) = cell_text(&row[index..=end], shared_strings)?
                    .filter(|text| text.contains("{{") || is_conditional_template(text))
                {
                    if group_header {
                        text = text.strip_prefix("@header").unwrap_or(&text).to_owned();
                    }
                    let rendered = render_text(
                        &text,
                        data,
                        collection_root,
                        item,
                        item_index,
                        ignore_missing,
                    )?;
                    output.extend(render_cell(start, &address, rendered));
                } else {
                    output.push(Event::Start(replace_attribute(start, b"r", &address)));
                    output.extend(row[index + 1..=end].iter().cloned().map(Event::into_owned));
                }
                index = end + 1;
            }
            event => {
                output.push(event.clone().into_owned());
                index += 1;
            }
        }
    }
    Ok(output)
}

fn is_conditional_template(value: &str) -> bool {
    value.trim_start().starts_with("@if(")
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

fn cell_text(events: &[Event<'_>], shared_strings: &[String]) -> Result<Option<String>> {
    let Event::Start(cell) = &events[0] else {
        return Ok(None);
    };
    let cell_type = attribute(cell, b"t").unwrap_or_default();
    let mut value = String::new();
    let mut in_value = false;
    let mut in_inline_text = false;
    for event in &events[1..] {
        match event {
            Event::Start(start) if start.name().as_ref() == b"v" => in_value = true,
            Event::End(end) if end.name().as_ref() == b"v" => in_value = false,
            Event::Start(start) if start.name().as_ref() == b"t" => in_inline_text = true,
            Event::End(end) if end.name().as_ref() == b"t" => in_inline_text = false,
            Event::Text(text) if in_value || in_inline_text => value.push_str(
                &text
                    .xml10_content()
                    .map_err(|error| Error::template(format!("invalid cell text: {error}")))?,
            ),
            Event::GeneralRef(reference) if in_value || in_inline_text => {
                append_reference(reference, &mut value)?;
            }
            _ => {}
        }
    }
    if cell_type == "s" {
        let index =
            value.parse::<usize>().map_err(|_| Error::template("invalid shared string index"))?;
        Ok(shared_strings.get(index).cloned())
    } else if matches!(cell_type.as_str(), "inlineStr" | "str") {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

enum RenderedValue {
    Empty,
    Bool(bool),
    Number(String),
    String(String),
}

fn render_text(
    template: &str,
    data: &Value,
    collection_root: Option<&str>,
    item: Option<&Value>,
    item_index: usize,
    ignore_missing: bool,
) -> Result<RenderedValue> {
    let conditional = select_conditional_branch(template, item)?;
    let template = conditional.as_deref().unwrap_or(template);
    let tokens = placeholder_tokens(template);
    if tokens.len() == 1 && template.trim() == format!("{{{{{}}}}}", tokens[0]) {
        let value =
            resolve_value(&tokens[0], data, collection_root, item, item_index, ignore_missing)?;
        return Ok(match value {
            Value::Null => RenderedValue::Empty,
            Value::Bool(value) => RenderedValue::Bool(value),
            Value::Number(value) => RenderedValue::Number(value.to_string()),
            value => RenderedValue::String(safe_string(&value_to_string(&value))),
        });
    }

    let mut rendered = template.to_owned();
    for token in tokens {
        let value = resolve_value(&token, data, collection_root, item, item_index, ignore_missing)?;
        rendered = rendered.replace(&format!("{{{{{token}}}}}"), &value_to_string(&value));
    }
    Ok(RenderedValue::String(safe_string(&rendered)))
}

fn select_conditional_branch(template: &str, item: Option<&Value>) -> Result<Option<String>> {
    let normalized = template.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.lines().map(str::trim).collect::<Vec<_>>();
    if !lines.first().is_some_and(|line| line.starts_with("@if(")) {
        return Ok(None);
    }
    let item = item
        .ok_or_else(|| Error::template("conditional template blocks require an enumerable item"))?;
    let mut index = 0_usize;
    let mut selected = None;
    let mut matched = false;
    let mut saw_else = false;
    loop {
        let directive = lines
            .get(index)
            .ok_or_else(|| Error::template("conditional template block is missing @endif"))?;
        if *directive == "@endif" {
            if index + 1 != lines.len() {
                return Err(Error::template("conditional template block has content after @endif"));
            }
            return Ok(Some(selected.unwrap_or_default()));
        }
        let condition = if let Some(condition) =
            directive.strip_prefix("@if(").and_then(|value| value.strip_suffix(')'))
        {
            if index != 0 {
                return Err(Error::template("@if must begin a conditional template block"));
            }
            Some(condition)
        } else if let Some(condition) =
            directive.strip_prefix("@elseif(").and_then(|value| value.strip_suffix(')'))
        {
            if index == 0 || saw_else {
                return Err(Error::template("@elseif is out of order"));
            }
            Some(condition)
        } else if *directive == "@else" {
            if index == 0 || saw_else {
                return Err(Error::template("@else is out of order"));
            }
            saw_else = true;
            None
        } else {
            return Err(Error::template(format!(
                "invalid conditional template directive '{directive}'"
            )));
        };
        let body = lines
            .get(index + 1)
            .ok_or_else(|| Error::template("conditional template directive has no branch body"))?;
        if body.starts_with('@') {
            return Err(Error::template("conditional template branch body cannot be a directive"));
        }
        if !matched {
            let branch_matches = match condition {
                Some(condition) => evaluate_condition(condition, item)?,
                None => true,
            };
            if branch_matches {
                selected = Some((*body).to_owned());
                matched = true;
            }
        }
        index += 2;
    }
}

fn evaluate_condition(condition: &str, item: &Value) -> Result<bool> {
    let parts = condition.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(Error::template(format!(
            "conditional expression '{condition}' must use 'field operator value'"
        )));
    }
    let actual = item.get(parts[0]).ok_or_else(|| {
        Error::template(format!("conditional field '{}' was not found", parts[0]))
    })?;
    compare_condition_value(actual, parts[1], parts[2])
}

fn compare_condition_value(actual: &Value, operator: &str, expected: &str) -> Result<bool> {
    match actual {
        Value::String(actual) => match operator {
            "==" => Ok(actual == expected),
            "!=" => Ok(actual != expected),
            _ => Err(Error::template(format!(
                "conditional operator '{operator}' is not supported for strings"
            ))),
        },
        Value::Number(actual) => {
            let actual = actual.as_f64().ok_or_else(|| {
                Error::template("conditional number cannot be represented as f64")
            })?;
            let expected = expected.parse::<f64>().map_err(|_| {
                Error::template(format!("conditional value '{expected}' is not a number"))
            })?;
            match operator {
                "==" => Ok(actual == expected),
                "!=" => Ok(actual != expected),
                ">" => Ok(actual > expected),
                "<" => Ok(actual < expected),
                ">=" => Ok(actual >= expected),
                "<=" => Ok(actual <= expected),
                _ => Err(Error::template(format!(
                    "conditional operator '{operator}' is not supported for numbers"
                ))),
            }
        }
        Value::Bool(actual) => {
            let expected = expected.parse::<bool>().map_err(|_| {
                Error::template(format!("conditional value '{expected}' is not a boolean"))
            })?;
            match operator {
                "==" => Ok(*actual == expected),
                "!=" => Ok(*actual != expected),
                _ => Err(Error::template(format!(
                    "conditional operator '{operator}' is not supported for booleans"
                ))),
            }
        }
        Value::Null => Ok(false),
        Value::Array(_) | Value::Object(_) => {
            Err(Error::template("conditional fields must contain scalar values"))
        }
    }
}

fn resolve_value(
    token: &str,
    data: &Value,
    collection_root: Option<&str>,
    item: Option<&Value>,
    item_index: usize,
    ignore_missing: bool,
) -> Result<Value> {
    if token == "$rowindex" {
        return Ok(Value::from(item_index + 1));
    }
    let path = token.split('.').collect::<Vec<_>>();
    let resolved = if collection_root == path.first().copied() {
        item.and_then(|item| resolve_path(item, &path[1..]))
    } else {
        resolve_path(data, &path)
    };
    match resolved {
        Some(value) if !value.is_array() && !value.is_object() => Ok(value.clone()),
        Some(_) => Ok(Value::Null),
        None if ignore_missing => Ok(Value::Null),
        None => Err(Error::template(format!("template variable '{token}' was not found"))),
    }
}

fn resolve_path<'a>(mut value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    for segment in path {
        value = value.get(*segment)?;
    }
    Some(value)
}

fn placeholder_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut remainder = value;
    while let Some(start) = remainder.find("{{") {
        remainder = &remainder[start + 2..];
        let Some(end) = remainder.find("}}") else {
            break;
        };
        let token = remainder[..end].trim();
        if !token.is_empty() {
            tokens.push(token.to_owned());
        }
        remainder = &remainder[end + 2..];
    }
    tokens
}

fn render_cell(
    original: &BytesStart<'_>,
    address: &str,
    value: RenderedValue,
) -> Vec<Event<'static>> {
    let cell_type = match value {
        RenderedValue::Bool(_) => Some("b"),
        RenderedValue::String(_) => Some("inlineStr"),
        RenderedValue::Empty | RenderedValue::Number(_) => None,
    };
    let cell = replace_cell_attributes(original, address, cell_type);
    let mut events = vec![Event::Start(cell)];
    match value {
        RenderedValue::Empty => {}
        RenderedValue::Bool(value) => {
            events.extend(value_events(if value { "1" } else { "0" }));
        }
        RenderedValue::Number(value) => events.extend(value_events(&value)),
        RenderedValue::String(value) => {
            events.push(Event::Start(BytesStart::new("is").into_owned()));
            events.push(Event::Start(BytesStart::new("t").into_owned()));
            events.push(Event::Text(BytesText::new(&value).into_owned()));
            events.push(Event::End(BytesEnd::new("t").into_owned()));
            events.push(Event::End(BytesEnd::new("is").into_owned()));
        }
    }
    events.push(Event::End(BytesEnd::new("c").into_owned()));
    events
}

fn value_events(value: &str) -> Vec<Event<'static>> {
    vec![
        Event::Start(BytesStart::new("v").into_owned()),
        Event::Text(BytesText::new(value).into_owned()),
        Event::End(BytesEnd::new("v").into_owned()),
    ]
}

fn safe_string(value: &str) -> String {
    if value.starts_with('=') || value.starts_with("$=") {
        format!("'{value}")
    } else {
        value.to_owned()
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => String::new(),
    }
}

fn replace_attribute(start: &BytesStart<'_>, key: &[u8], value: &str) -> BytesStart<'static> {
    replace_attributes(start, &[(key, Some(value))])
}

fn replace_cell_attributes(
    start: &BytesStart<'_>,
    address: &str,
    cell_type: Option<&str>,
) -> BytesStart<'static> {
    replace_attributes(start, &[(b"r", Some(address)), (b"t", cell_type)])
}

fn replace_attributes(
    start: &BytesStart<'_>,
    replacements: &[(&[u8], Option<&str>)],
) -> BytesStart<'static> {
    let mut output = BytesStart::new(String::from_utf8_lossy(start.name().as_ref()).into_owned());
    for attribute in start.attributes().with_checks(false).flatten() {
        if replacements.iter().any(|(key, _)| attribute.key.as_ref() == *key) {
            continue;
        }
        output.push_attribute((attribute.key.as_ref(), attribute.value.as_ref()));
    }
    for (key, value) in replacements {
        if let Some(value) = value {
            output.push_attribute((*key, value.as_bytes()));
        }
    }
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

fn update_dimension(events: &mut [Event<'static>], max_row: usize) -> Result<()> {
    for event in events {
        let Event::Empty(dimension) = event else {
            continue;
        };
        if dimension.name().as_ref() != b"dimension" {
            continue;
        }
        let Some(reference) = attribute(dimension, b"ref") else {
            return Ok(());
        };
        let (start, end) = reference.split_once(':').unwrap_or((&reference, &reference));
        let end_column = end.chars().take_while(char::is_ascii_alphabetic).collect::<String>();
        let updated = format!("{start}:{end_column}{max_row}");
        *dimension = replace_attribute(dimension, b"ref", &updated);
        return Ok(());
    }
    Ok(())
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
