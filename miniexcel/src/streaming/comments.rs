use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::Path;

use chrono::{DateTime, FixedOffset, NaiveDateTime};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use uuid::Uuid;
use zip::ZipArchive;
use zip::result::ZipError;

use crate::comments::{NoteParts, ThreadedCommentParts, ThreadedReplyParts};
use crate::{
    CellReference, CommentPerson, CommentTimestamp, Error, Result, SheetComments, ThreadedComment,
    ThreadedCommentReply,
};

const MAX_COMMENT_PART_BYTES: u64 = 16 * 1024 * 1024;
const MAX_COMMENT_RECORDS: usize = 262_144;

#[derive(Clone, Debug)]
struct Relationship {
    id: String,
    relationship_type: String,
    target: String,
}

#[derive(Debug)]
struct RawThread {
    id: Uuid,
    parent_id: Option<Uuid>,
    cell: CellReference,
    person_id: Uuid,
    created_at: Option<CommentTimestamp>,
    resolved: bool,
    text: String,
}

#[derive(Debug)]
struct RawNote {
    id: Option<Uuid>,
    cell: CellReference,
    author: Option<String>,
    text: String,
}

pub(super) fn get_comments(
    path: impl AsRef<Path>,
    sheet_name: Option<&str>,
) -> Result<SheetComments> {
    let file = std::fs::File::open(path)?;
    get_comments_from_reader(BufReader::new(file), sheet_name)
}

pub(super) fn get_comments_from_bytes(
    bytes: &[u8],
    sheet_name: Option<&str>,
) -> Result<SheetComments> {
    get_comments_from_reader(Cursor::new(bytes), sheet_name)
}

pub(super) fn get_comments_from_reader<R>(
    reader: R,
    sheet_name: Option<&str>,
) -> Result<SheetComments>
where
    R: Read + Seek,
{
    let mut archive = ZipArchive::new(reader)
        .map_err(|error| Error::stream(format!("cannot open XLSX comments reader: {error}")))?;
    validate_entry_names(&mut archive)?;
    let root_relationships = read_relationships(&mut archive, "_rels/.rels", "")?;
    let workbook_path = root_relationships
        .iter()
        .find(|relationship| {
            relationship.relationship_type.rsplit('/').next() == Some("officeDocument")
        })
        .map(|relationship| relationship.target.clone())
        .ok_or_else(|| Error::stream("workbook relationship was not found"))?;
    let workbook_xml = read_part(&mut archive, &workbook_path)?;
    let workbook_relationship_path = relationship_part_path(&workbook_path)?;
    let workbook_relationships =
        read_relationships(&mut archive, &workbook_relationship_path, &workbook_path)?;
    let (resolved_sheet_name, sheet_relationship_id) = select_sheet(&workbook_xml, sheet_name)?;
    let worksheet_path = workbook_relationships
        .iter()
        .find(|relationship| relationship.id == sheet_relationship_id)
        .filter(|relationship| {
            relationship.relationship_type.rsplit('/').next() == Some("worksheet")
        })
        .map(|relationship| relationship.target.clone())
        .ok_or_else(|| {
            Error::invalid_comments(&resolved_sheet_name, "worksheet relationship was not found")
        })?;

    let people =
        read_people(&mut archive, &workbook_relationships, &workbook_path, &resolved_sheet_name)?;
    let worksheet_relationship_path = relationship_part_path(&worksheet_path)?;
    let has_worksheet_relationships = {
        match archive.by_name(&worksheet_relationship_path) {
            Ok(_) => true,
            Err(ZipError::FileNotFound) => false,
            Err(error) => {
                return Err(Error::stream(format!("cannot read worksheet relationships: {error}")));
            }
        }
    };
    if !has_worksheet_relationships {
        return Ok(SheetComments::new(resolved_sheet_name, Vec::new(), Vec::new()));
    }
    let worksheet_relationships =
        read_relationships(&mut archive, &worksheet_relationship_path, &worksheet_path)?;

    let mut raw_threads = Vec::new();
    for relationship in worksheet_relationships.iter().filter(|relationship| {
        relationship.relationship_type.rsplit('/').next() == Some("threadedComment")
    }) {
        raw_threads.extend(parse_threaded_comments(
            &read_part(&mut archive, &relationship.target)?,
            &resolved_sheet_name,
        )?);
    }
    let threaded_comments = build_threads(raw_threads, &people, &resolved_sheet_name)?;
    let thread_cells = threaded_comments
        .iter()
        .map(|comment| (comment.cell().row(), comment.cell().column(), comment.id()))
        .collect::<BTreeSet<_>>();

    let mut notes = Vec::new();
    for relationship in worksheet_relationships.iter().filter(|relationship| {
        relationship.relationship_type.rsplit('/').next() == Some("comments")
    }) {
        notes.extend(parse_notes(
            &read_part(&mut archive, &relationship.target)?,
            &thread_cells,
            &resolved_sheet_name,
        )?);
    }
    Ok(SheetComments::new(resolved_sheet_name, threaded_comments, notes))
}

fn validate_entry_names<R>(archive: &mut ZipArchive<R>) -> Result<()>
where
    R: Read + Seek,
{
    let mut names = BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| Error::stream(format!("cannot inspect comments package: {error}")))?;
        let name = entry.name();
        if name.starts_with('/')
            || name.contains('\\')
            || name.split('/').any(|part| part.is_empty() || matches!(part, "." | ".."))
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err(Error::stream(format!("unsafe or duplicate package path '{name}'")));
        }
    }
    Ok(())
}

fn select_sheet(workbook_xml: &[u8], requested: Option<&str>) -> Result<(String, String)> {
    let mut reader = Reader::from_reader(workbook_xml);
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::stream(format!("invalid workbook XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"sheet" =>
            {
                let name = attribute(&event, b"name")?
                    .ok_or_else(|| Error::stream("workbook sheet has no name"))?;
                if requested.is_none_or(|requested| requested.eq_ignore_ascii_case(&name)) {
                    let relationship_id = attribute(&event, b"id")?
                        .ok_or_else(|| Error::stream("workbook sheet has no relationship"))?;
                    return Ok((name, relationship_id));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    match requested {
        Some(name) => Err(Error::sheet_not_found(name)),
        None => Err(Error::no_worksheets()),
    }
}

fn read_relationships<R>(
    archive: &mut ZipArchive<R>,
    relationship_path: &str,
    source_path: &str,
) -> Result<Vec<Relationship>>
where
    R: Read + Seek,
{
    let xml = read_part(archive, relationship_path)?;
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut relationships = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::stream(format!("invalid relationships XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"Relationship" =>
            {
                if attribute(&event, b"TargetMode")?
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("External"))
                {
                    continue;
                }
                let id = attribute(&event, b"Id")?
                    .ok_or_else(|| Error::stream("Relationship has no ID"))?;
                let relationship_type = attribute(&event, b"Type")?
                    .ok_or_else(|| Error::stream("Relationship has no type"))?;
                let target = attribute(&event, b"Target")?
                    .ok_or_else(|| Error::stream("Relationship has no target"))?;
                relationships.push(Relationship {
                    id,
                    relationship_type,
                    target: resolve_target(source_path, &target)?,
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(relationships)
}

fn read_people<R>(
    archive: &mut ZipArchive<R>,
    relationships: &[Relationship],
    _workbook_path: &str,
    sheet_name: &str,
) -> Result<BTreeMap<Uuid, CommentPerson>>
where
    R: Read + Seek,
{
    let mut people = BTreeMap::new();
    for relationship in relationships
        .iter()
        .filter(|relationship| relationship.relationship_type.rsplit('/').next() == Some("person"))
    {
        let xml = read_part(archive, &relationship.target)?;
        let mut reader = Reader::from_reader(xml.as_slice());
        loop {
            match reader.read_event().map_err(|error| {
                Error::invalid_comments(sheet_name, format!("invalid people XML: {error}"))
            })? {
                Event::Start(event) | Event::Empty(event)
                    if local_name(event.name().as_ref()) == b"person" =>
                {
                    let id =
                        parse_uuid(&required_attribute(&event, b"id", sheet_name)?, sheet_name)?;
                    if people.contains_key(&id) {
                        return Err(Error::invalid_comments(
                            sheet_name,
                            format!("duplicate person ID {id}"),
                        ));
                    }
                    people.insert(
                        id,
                        CommentPerson::new(
                            id,
                            attribute(&event, b"displayName")?.unwrap_or_default(),
                            attribute(&event, b"userId")?,
                            attribute(&event, b"providerId")?,
                        ),
                    );
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    Ok(people)
}

fn parse_threaded_comments(xml: &[u8], sheet_name: &str) -> Result<Vec<RawThread>> {
    let mut reader = Reader::from_reader(xml);
    let mut threads = Vec::new();
    let mut current = None::<RawThread>;
    let mut in_text = false;
    loop {
        match reader.read_event().map_err(|error| {
            Error::invalid_comments(sheet_name, format!("invalid threaded comments XML: {error}"))
        })? {
            Event::Start(event) if local_name(event.name().as_ref()) == b"threadedComment" => {
                if threads.len() >= MAX_COMMENT_RECORDS {
                    return Err(Error::invalid_comments(sheet_name, "too many threaded comments"));
                }
                current = Some(RawThread {
                    id: parse_uuid(&required_attribute(&event, b"id", sheet_name)?, sheet_name)?,
                    parent_id: attribute(&event, b"parentId")?
                        .map(|value| parse_uuid(&value, sheet_name))
                        .transpose()?,
                    cell: required_attribute(&event, b"ref", sheet_name)?.parse()?,
                    person_id: parse_uuid(
                        &required_attribute(&event, b"personId", sheet_name)?,
                        sheet_name,
                    )?,
                    created_at: attribute(&event, b"dT")?
                        .map(|value| parse_timestamp(&value, sheet_name))
                        .transpose()?,
                    resolved: parse_done(attribute(&event, b"done")?.as_deref(), sheet_name)?,
                    text: String::new(),
                });
            }
            Event::Start(event)
                if current.is_some() && local_name(event.name().as_ref()) == b"text" =>
            {
                in_text = true;
            }
            Event::Text(text) if in_text && current.is_some() => {
                let decoded = text.decode().map_err(|error| {
                    Error::invalid_comments(
                        sheet_name,
                        format!("invalid threaded comment text: {error}"),
                    )
                })?;
                append_normalized_xml_text(&mut current.as_mut().unwrap().text, &decoded);
            }
            Event::GeneralRef(reference) if in_text && current.is_some() => {
                append_reference(&reference, &mut current.as_mut().unwrap().text, sheet_name)?;
            }
            Event::End(event) if local_name(event.name().as_ref()) == b"text" => in_text = false,
            Event::End(event) if local_name(event.name().as_ref()) == b"threadedComment" => {
                threads.push(current.take().ok_or_else(|| {
                    Error::invalid_comments(sheet_name, "threaded comment end without start")
                })?);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if current.is_some() {
        return Err(Error::invalid_comments(sheet_name, "unterminated threaded comment"));
    }
    Ok(threads)
}

fn build_threads(
    raw: Vec<RawThread>,
    people: &BTreeMap<Uuid, CommentPerson>,
    sheet_name: &str,
) -> Result<Vec<ThreadedComment>> {
    let mut seen = BTreeSet::new();
    for thread in &raw {
        if !seen.insert(thread.id) {
            return Err(Error::invalid_comments(
                sheet_name,
                format!("duplicate threaded comment ID {}", thread.id),
            ));
        }
    }
    let root_ids = raw
        .iter()
        .filter(|thread| thread.parent_id.is_none())
        .map(|thread| thread.id)
        .collect::<BTreeSet<_>>();
    let mut replies = BTreeMap::<Uuid, Vec<ThreadedCommentReply>>::new();
    for reply in raw.iter().filter(|thread| thread.parent_id.is_some()) {
        let parent_id = reply.parent_id.expect("reply parent checked");
        if !root_ids.contains(&parent_id) {
            return Err(Error::invalid_comments(
                sheet_name,
                format!("orphan or nested reply {} references {parent_id}", reply.id),
            ));
        }
        replies.entry(parent_id).or_default().push(
            ThreadedReplyParts {
                id: reply.id,
                parent_id,
                person_id: reply.person_id,
                person: people.get(&reply.person_id).cloned(),
                created_at: reply.created_at.clone(),
                text: reply.text.clone(),
            }
            .into(),
        );
    }
    Ok(raw
        .into_iter()
        .filter(|thread| thread.parent_id.is_none())
        .map(|thread| {
            ThreadedCommentParts {
                id: thread.id,
                cell: thread.cell,
                person_id: thread.person_id,
                person: people.get(&thread.person_id).cloned(),
                created_at: thread.created_at,
                resolved: thread.resolved,
                text: thread.text,
                replies: replies.remove(&thread.id).unwrap_or_default(),
            }
            .into()
        })
        .collect())
}

fn parse_notes(
    xml: &[u8],
    thread_cells: &BTreeSet<(usize, usize, Uuid)>,
    sheet_name: &str,
) -> Result<Vec<crate::NoteComment>> {
    let mut reader = Reader::from_reader(xml);
    let mut authors = Vec::new();
    let mut current_author = None::<String>;
    let mut notes = Vec::new();
    let mut current = None::<RawNote>;
    let mut in_text = false;
    let mut in_author = false;
    loop {
        match reader.read_event().map_err(|error| {
            Error::invalid_comments(sheet_name, format!("invalid notes XML: {error}"))
        })? {
            Event::Start(event) if local_name(event.name().as_ref()) == b"author" => {
                current_author = Some(String::new());
                in_author = true;
            }
            Event::Empty(event) if local_name(event.name().as_ref()) == b"author" => {
                authors.push(String::new());
            }
            Event::Text(text) if in_author => {
                let decoded = text.decode().map_err(|error| {
                    Error::invalid_comments(sheet_name, format!("invalid note author: {error}"))
                })?;
                append_normalized_xml_text(current_author.as_mut().unwrap(), &decoded);
            }
            Event::End(event) if local_name(event.name().as_ref()) == b"author" => {
                authors.push(current_author.take().unwrap_or_default());
                in_author = false;
            }
            Event::Start(event) if local_name(event.name().as_ref()) == b"comment" => {
                if notes.len() >= MAX_COMMENT_RECORDS {
                    return Err(Error::invalid_comments(sheet_name, "too many notes"));
                }
                let author = attribute(&event, b"authorId")?
                    .and_then(|value| value.parse::<usize>().ok())
                    .and_then(|index| authors.get(index).cloned())
                    .filter(|value| !value.is_empty());
                current = Some(RawNote {
                    id: attribute(&event, b"uid")?
                        .map(|value| parse_uuid(&value, sheet_name))
                        .transpose()?,
                    cell: required_attribute(&event, b"ref", sheet_name)?.parse()?,
                    author,
                    text: String::new(),
                });
            }
            Event::Start(event)
                if current.is_some() && local_name(event.name().as_ref()) == b"t" =>
            {
                in_text = true;
            }
            Event::Text(text) if in_text && current.is_some() => {
                let decoded = text.decode().map_err(|error| {
                    Error::invalid_comments(sheet_name, format!("invalid note text: {error}"))
                })?;
                append_normalized_xml_text(&mut current.as_mut().unwrap().text, &decoded);
            }
            Event::GeneralRef(reference) if in_text && current.is_some() => {
                append_reference(&reference, &mut current.as_mut().unwrap().text, sheet_name)?;
            }
            Event::End(event) if local_name(event.name().as_ref()) == b"t" => in_text = false,
            Event::End(event) if local_name(event.name().as_ref()) == b"comment" => {
                let note = current
                    .take()
                    .ok_or_else(|| Error::invalid_comments(sheet_name, "note end without start"))?;
                let shadow = note.author.as_deref().and_then(thread_marker).is_some_and(|id| {
                    thread_cells.contains(&(note.cell.row(), note.cell.column(), id))
                });
                if !shadow {
                    notes.push(note);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(notes
        .into_iter()
        .map(|note| {
            NoteParts { id: note.id, cell: note.cell, author: note.author, text: note.text }.into()
        })
        .collect())
}

fn parse_done(value: Option<&str>, sheet_name: &str) -> Result<bool> {
    match value {
        None | Some("0" | "false") => Ok(false),
        Some("1" | "true") => Ok(true),
        Some(value) => {
            Err(Error::invalid_comments(sheet_name, format!("invalid done value '{value}'")))
        }
    }
}

fn parse_timestamp(value: &str, sheet_name: &str) -> Result<CommentTimestamp> {
    if let Ok(value) = DateTime::<FixedOffset>::parse_from_rfc3339(value) {
        return Ok(CommentTimestamp::Offset(value));
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .map(CommentTimestamp::Local)
        .map_err(|_| Error::invalid_comments(sheet_name, format!("invalid timestamp '{value}'")))
}

fn parse_uuid(value: &str, sheet_name: &str) -> Result<Uuid> {
    Uuid::parse_str(value.trim_matches(['{', '}']))
        .map_err(|_| Error::invalid_comments(sheet_name, format!("invalid UUID '{value}'")))
}

fn thread_marker(author: &str) -> Option<Uuid> {
    author.strip_prefix("tc={")?.strip_suffix('}').and_then(|value| Uuid::parse_str(value).ok())
}

fn required_attribute(event: &BytesStart<'_>, name: &[u8], sheet_name: &str) -> Result<String> {
    attribute(event, name)?.ok_or_else(|| {
        Error::invalid_comments(
            sheet_name,
            format!(
                "{} element has no {}",
                String::from_utf8_lossy(local_name(event.name().as_ref())),
                String::from_utf8_lossy(name)
            ),
        )
    })
}

fn attribute(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    for attribute in event.attributes().with_checks(true) {
        let attribute =
            attribute.map_err(|error| Error::stream(format!("invalid XML attribute: {error}")))?;
        if local_name(attribute.key.as_ref()) == name {
            return attribute
                .decode_and_unescape_value(event.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| Error::stream(format!("invalid XML value: {error}")));
        }
    }
    Ok(None)
}

fn read_part<R>(archive: &mut ZipArchive<R>, path: &str) -> Result<Vec<u8>>
where
    R: Read + Seek,
{
    let entry = archive
        .by_name(path)
        .map_err(|error| Error::stream(format!("cannot read comments part '{path}': {error}")))?;
    if entry.size() > MAX_COMMENT_PART_BYTES {
        return Err(Error::stream(format!("comments part '{path}' is too large")));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.take(MAX_COMMENT_PART_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_COMMENT_PART_BYTES {
        return Err(Error::stream(format!("comments part '{path}' is too large")));
    }
    Ok(bytes)
}

fn relationship_part_path(source: &str) -> Result<String> {
    let (directory, file) = source
        .rsplit_once('/')
        .ok_or_else(|| Error::stream(format!("invalid package part '{source}'")))?;
    Ok(format!("{directory}/_rels/{file}.rels"))
}

fn resolve_target(source: &str, target: &str) -> Result<String> {
    if target.contains('\\') || target.contains(['?', '#']) {
        return Err(Error::stream(format!("unsafe relationship target '{target}'")));
    }
    let base = source.rsplit_once('/').map_or("", |(directory, _)| directory);
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_owned()
    } else if base.is_empty() {
        target.to_owned()
    } else {
        format!("{base}/{target}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error::stream(format!(
                        "relationship target '{target}' escapes the package root"
                    )));
                }
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(Error::stream(format!("relationship target '{target}' is empty")));
    }
    Ok(parts.join("/"))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn append_normalized_xml_text(target: &mut String, value: &str) {
    target.push_str(&value.replace("\r\n", "\n").replace('\r', "\n"));
}

fn append_reference(
    reference: &quick_xml::events::BytesRef<'_>,
    target: &mut String,
    sheet_name: &str,
) -> Result<()> {
    let decoded = reference.decode().map_err(|error| {
        Error::invalid_comments(sheet_name, format!("invalid XML reference: {error}"))
    })?;
    match decoded.as_ref() {
        "lt" => target.push('<'),
        "gt" => target.push('>'),
        "amp" => target.push('&'),
        "quot" => target.push('"'),
        "apos" => target.push('\''),
        _ => {
            if let Some(value) = reference.resolve_char_ref().map_err(|error| {
                Error::invalid_comments(sheet_name, format!("invalid XML reference: {error}"))
            })? {
                target.push(value);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_threads_and_orphan_replies() {
        let id = uuid("00000000-0000-0000-0000-000000000001");
        let duplicate = vec![raw_thread(id, None), raw_thread(id, None)];
        assert!(build_threads(duplicate, &BTreeMap::new(), "Data").is_err());

        let orphan = vec![raw_thread(
            uuid("00000000-0000-0000-0000-000000000002"),
            Some(uuid("00000000-0000-0000-0000-000000000099")),
        )];
        assert!(build_threads(orphan, &BTreeMap::new(), "Data").is_err());
    }

    #[test]
    fn preserves_unresolved_person_id_without_fabricating_person() {
        let person_id = uuid("00000000-0000-0000-0000-000000000010");
        let comments = build_threads(
            vec![raw_thread(uuid("00000000-0000-0000-0000-000000000001"), None)],
            &BTreeMap::new(),
            "Data",
        )
        .unwrap();
        assert_eq!(comments[0].person_id(), person_id);
        assert!(comments[0].person().is_none());
    }

    #[test]
    fn suppresses_only_matching_thread_compatibility_note() {
        let thread_id = uuid("00000000-0000-0000-0000-000000000001");
        let cell: CellReference = "A2".parse().unwrap();
        let xml = format!(
            r#"<comments><authors><author>tc={{{thread_id}}}</author><author>local</author></authors><commentList><comment ref="A2" authorId="0"><text><t>shadow</t></text></comment><comment ref="A2" authorId="1"><text><r><t>real &amp; </t></r><r><t>visible</t></r></text></comment></commentList></comments>"#
        );
        let notes = parse_notes(
            xml.as_bytes(),
            &BTreeSet::from([(cell.row(), cell.column(), thread_id)]),
            "Data",
        )
        .unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].author(), Some("local"));
        assert_eq!(notes[0].text(), "real & visible");
    }

    #[test]
    fn preserves_offset_and_local_timestamp_semantics() {
        assert!(matches!(
            parse_timestamp("2026-03-21T12:07:24+08:00", "Data").unwrap(),
            CommentTimestamp::Offset(_)
        ));
        assert!(matches!(
            parse_timestamp("2026-03-21T12:07:24", "Data").unwrap(),
            CommentTimestamp::Local(_)
        ));
    }

    fn raw_thread(id: Uuid, parent_id: Option<Uuid>) -> RawThread {
        RawThread {
            id,
            parent_id,
            cell: "A2".parse().unwrap(),
            person_id: uuid("00000000-0000-0000-0000-000000000010"),
            created_at: None,
            resolved: false,
            text: String::new(),
        }
    }

    fn uuid(value: &str) -> Uuid {
        Uuid::parse_str(value).unwrap()
    }
}
