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
    let value = serde_json::to_value(value)
        .map_err(|error| Error::template(format!("cannot serialize template data: {error}")))?;
    let mut archive = ZipArchive::new(Cursor::new(template))
        .map_err(|error| Error::template(format!("cannot open template workbook: {error}")))?;
    let shared_strings = read_shared_strings(&mut archive)?;
    let output = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(output);

    for index in 0..archive.len() {
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
                &value,
                options.ignore_missing_variables(),
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

    let mut rendered_rows = Vec::new();
    let mut shift = 0_usize;
    let mut previous_source_row = 0_usize;
    let mut max_output_row = 1_usize;
    for row in rows {
        let source_row = row_number(&row).unwrap_or(previous_source_row.saturating_add(1));
        previous_source_row = source_row;
        let output_row = source_row.saturating_add(shift);
        let expansion = expansion_for_row(&row, shared_strings, data)?;
        let items: Vec<Option<&Value>> = match expansion.as_ref() {
            Some((_, values)) if !values.is_empty() => values.iter().map(Some).collect(),
            Some(_) => vec![None],
            None => vec![None],
        };
        for (item_index, item) in items.iter().enumerate() {
            let target_row = output_row.saturating_add(item_index);
            rendered_rows.push(render_row(
                &row,
                target_row,
                shared_strings,
                data,
                expansion.as_ref().map(|(root, _)| root.as_str()),
                *item,
                item_index,
                ignore_missing,
            )?);
            max_output_row = max_output_row.max(target_row);
        }
        shift = shift.saturating_add(items.len().saturating_sub(1));
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
                if let Some(text) =
                    cell_text(&row[index..=end], shared_strings)?.filter(|text| text.contains("{{"))
                {
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
