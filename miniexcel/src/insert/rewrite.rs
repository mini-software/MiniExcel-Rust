use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::donor::DonorWorksheet;
use super::package::{DefinedName, PackageInventory, WorkbookSheet, WorksheetAllocation};
use super::style::rebase_styles;
use crate::{Error, Result, SheetVisibility, TargetRelationshipPolicy};

const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
const WORKBOOK_PATH: &str = "xl/workbook.xml";
const WORKBOOK_RELS_PATH: &str = "xl/_rels/workbook.xml.rels";
const WORKSHEET_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const WORKSHEET_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const CALC_CHAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PackageRewriteStage {
    Copy,
    Finish,
}

#[derive(Clone, Debug)]
pub(super) struct ReplacementPlan {
    target: WorkbookSheet,
    styles_path: String,
    removed_entries: BTreeSet<String>,
    calc_chain_relationship_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplacementRelationshipKind {
    Table,
    Drawing,
    Comments,
    VmlDrawing,
    Hyperlink,
    Image,
}

pub(super) fn plan_replacement(
    inventory: &PackageInventory,
    sheet_name: &str,
    policy: TargetRelationshipPolicy,
) -> Result<ReplacementPlan> {
    let target = inventory
        .find_sheet(sheet_name)
        .cloned()
        .ok_or_else(|| Error::sheet_not_found(sheet_name))?;
    let direct_relationships = inventory
        .relationships
        .iter()
        .filter(|relationship| relationship.source.as_deref() == Some(target.target.as_str()))
        .collect::<Vec<_>>();
    if policy == TargetRelationshipPolicy::Reject && !direct_relationships.is_empty() {
        return Err(Error::unsupported_package_feature(format!(
            "worksheet '{}' owns relationship type '{}'",
            target.name, direct_relationships[0].relationship_type
        )));
    }

    let mut removed_entries = BTreeSet::new();
    if policy == TargetRelationshipPolicy::RemoveSupported {
        let target_relationship_part = relationship_part_path(&target.target)?;
        if inventory.entry_names.contains(&target_relationship_part) {
            removed_entries.insert(target_relationship_part);
        }

        let mut candidates = BTreeSet::new();
        let mut queue = VecDeque::new();
        for relationship in direct_relationships {
            match replacement_relationship_kind(&relationship.relationship_type) {
                Some(
                    ReplacementRelationshipKind::Table
                    | ReplacementRelationshipKind::Drawing
                    | ReplacementRelationshipKind::Comments
                    | ReplacementRelationshipKind::VmlDrawing,
                ) => {
                    let part = relationship.normalized_target.clone().ok_or_else(|| {
                        Error::unsupported_package_feature(format!(
                            "worksheet relationship '{}' cannot be external",
                            relationship.relationship_type
                        ))
                    })?;
                    if candidates.insert(part.clone()) {
                        queue.push_back(part);
                    }
                }
                Some(ReplacementRelationshipKind::Hyperlink)
                    if relationship
                        .target_mode
                        .as_deref()
                        .is_some_and(|mode| mode.eq_ignore_ascii_case("External")) => {}
                _ => {
                    return Err(Error::unsupported_package_feature(format!(
                        "worksheet replacement cannot remove relationship type '{}'",
                        relationship.relationship_type
                    )));
                }
            }
        }

        while let Some(source) = queue.pop_front() {
            if !inventory.entry_names.contains(&source) {
                return Err(Error::insert_package(format!(
                    "worksheet-owned part '{source}' is missing"
                )));
            }
            for relationship in inventory
                .relationships
                .iter()
                .filter(|relationship| relationship.source.as_deref() == Some(source.as_str()))
            {
                match replacement_relationship_kind(&relationship.relationship_type) {
                    Some(ReplacementRelationshipKind::Image) => {
                        let part = relationship.normalized_target.clone().ok_or_else(|| {
                            Error::unsupported_package_feature(
                                "worksheet-owned drawing contains an external image",
                            )
                        })?;
                        if candidates.insert(part.clone()) {
                            queue.push_back(part);
                        }
                    }
                    _ => {
                        return Err(Error::unsupported_package_feature(format!(
                            "worksheet-owned closure contains relationship type '{}'",
                            relationship.relationship_type
                        )));
                    }
                }
            }
        }

        let mut preserved = candidates
            .iter()
            .filter(|candidate| {
                inventory.relationships.iter().any(|relationship| {
                    relationship.normalized_target.as_deref() == Some(candidate.as_str())
                        && relationship.source.as_deref() != Some(target.target.as_str())
                        && !relationship
                            .source
                            .as_deref()
                            .is_some_and(|source| candidates.contains(source))
                })
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut preserve_queue = preserved.iter().cloned().collect::<VecDeque<_>>();
        while let Some(source) = preserve_queue.pop_front() {
            for relationship in inventory
                .relationships
                .iter()
                .filter(|relationship| relationship.source.as_deref() == Some(source.as_str()))
            {
                if let Some(target) = relationship.normalized_target.as_ref() {
                    if candidates.contains(target) && preserved.insert(target.clone()) {
                        preserve_queue.push_back(target.clone());
                    }
                }
            }
        }
        for part in candidates.difference(&preserved) {
            removed_entries.insert(part.clone());
            let relationship_part = relationship_part_path(part)?;
            if inventory.entry_names.contains(&relationship_part) {
                removed_entries.insert(relationship_part);
            }
        }
    }

    let mut calc_chain_relationships = inventory.relationships.iter().filter(|relationship| {
        relationship.source.as_deref() == Some(WORKBOOK_PATH)
            && is_calculation_chain_relationship(&relationship.relationship_type)
    });
    let calc_chain = calc_chain_relationships.next();
    if calc_chain_relationships.next().is_some() {
        return Err(Error::unsafe_package("workbook has multiple calculation-chain relationships"));
    }
    let calc_chain_relationship_id = if let Some(relationship) = calc_chain {
        let path = relationship.normalized_target.clone().ok_or_else(|| {
            Error::unsupported_package_feature("calculation-chain relationship cannot be external")
        })?;
        if !inventory.entry_names.contains(&path) {
            return Err(Error::insert_package(format!(
                "calculation-chain part '{path}' is missing"
            )));
        }
        let content_type = inventory
            .content_types
            .overrides
            .iter()
            .find(|entry| entry.part_name == path)
            .ok_or_else(|| {
                Error::insert_package(format!(
                    "calculation-chain part '{path}' has no content-type override"
                ))
            })?;
        if content_type.content_type != CALC_CHAIN_CONTENT_TYPE {
            return Err(Error::unsupported_package_feature(format!(
                "calculation-chain relationship target '{path}' has incompatible content type '{}'",
                content_type.content_type
            )));
        }
        if inventory.relationships.iter().any(|candidate| {
            candidate.normalized_target.as_deref() == Some(path.as_str())
                && !(candidate.source.as_deref() == Some(WORKBOOK_PATH)
                    && candidate.id == relationship.id)
        }) {
            return Err(Error::unsupported_package_feature(format!(
                "calculation-chain part '{path}' is referenced by another relationship"
            )));
        }
        if inventory
            .relationships
            .iter()
            .any(|candidate| candidate.source.as_deref() == Some(path.as_str()))
        {
            return Err(Error::unsupported_package_feature(
                "calculation-chain part owns relationships",
            ));
        }
        removed_entries.insert(path.clone());
        let relationship_part = relationship_part_path(&path)?;
        if inventory.entry_names.contains(&relationship_part) {
            removed_entries.insert(relationship_part);
        }
        Some(relationship.id.clone())
    } else {
        None
    };

    Ok(ReplacementPlan {
        target,
        styles_path: styles_path(inventory)?,
        removed_entries,
        calc_chain_relationship_id,
    })
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
        donor.visibility,
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
        &BTreeSet::new(),
        Some((&allocation.package_path, style_rebase.worksheet_xml.as_slice())),
        &mut checkpoint,
    )
}

pub(super) fn replace_worksheet_to_writer_with_hook<R, W, F>(
    mut source: R,
    destination: W,
    donor: &DonorWorksheet,
    plan: &ReplacementPlan,
    mut checkpoint: F,
) -> Result<W>
where
    R: Read + Seek,
    W: Write + Seek,
    F: FnMut(PackageRewriteStage) -> Result<()>,
{
    source.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(source).map_err(|error| {
        Error::insert_package(format!("cannot reopen source workbook: {error}"))
    })?;
    let workbook_xml = read_part(&mut archive, WORKBOOK_PATH)?;
    let workbook_rels_xml = read_part(&mut archive, WORKBOOK_RELS_PATH)?;
    let content_types_xml = read_part(&mut archive, CONTENT_TYPES_PATH)?;
    let styles_xml = read_part(&mut archive, &plan.styles_path)?;
    let target_worksheet_xml = read_part(&mut archive, &plan.target.target)?;
    let style_rebase = rebase_styles(&styles_xml, donor)?;
    let worksheet_xml = preserve_tab_selected(&style_rebase.worksheet_xml, &target_worksheet_xml)?;
    let patched_workbook =
        force_full_calculation(&replace_target_filter_defined_name(&workbook_xml, plan, donor)?)?;
    let patched_workbook_rels =
        remove_relationship_by_id(&workbook_rels_xml, plan.calc_chain_relationship_id.as_deref())?;
    let patched_content_types =
        remove_content_type_overrides(&content_types_xml, &plan.removed_entries)?;

    let mut replacements = BTreeMap::from([
        (plan.styles_path.clone(), style_rebase.styles_xml),
        (plan.target.target.clone(), worksheet_xml),
    ]);
    if patched_workbook != workbook_xml {
        replacements.insert(WORKBOOK_PATH.to_owned(), patched_workbook);
    }
    if patched_workbook_rels != workbook_rels_xml {
        replacements.insert(WORKBOOK_RELS_PATH.to_owned(), patched_workbook_rels);
    }
    if patched_content_types != content_types_xml {
        replacements.insert(CONTENT_TYPES_PATH.to_owned(), patched_content_types);
    }
    write_package(archive, destination, &replacements, &plan.removed_entries, None, &mut checkpoint)
}

fn is_calculation_chain_relationship(relationship_type: &str) -> bool {
    const TYPES: [&str; 2] = [
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/calcChain",
    ];
    TYPES.contains(&relationship_type)
}

fn replacement_relationship_kind(relationship_type: &str) -> Option<ReplacementRelationshipKind> {
    const BASES: [&str; 2] = [
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/",
    ];
    let kind = BASES.iter().find_map(|base| relationship_type.strip_prefix(base))?;
    match kind {
        "table" => Some(ReplacementRelationshipKind::Table),
        "drawing" => Some(ReplacementRelationshipKind::Drawing),
        "comments" => Some(ReplacementRelationshipKind::Comments),
        "vmlDrawing" => Some(ReplacementRelationshipKind::VmlDrawing),
        "hyperlink" => Some(ReplacementRelationshipKind::Hyperlink),
        "image" => Some(ReplacementRelationshipKind::Image),
        _ => None,
    }
}

fn preserve_tab_selected(donor_xml: &[u8], target_xml: &[u8]) -> Result<Vec<u8>> {
    let target_value = first_sheet_view_attribute(target_xml, b"tabSelected")?;
    let mut reader = Reader::from_reader(donor_xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(donor_xml.len()));
    loop {
        let event = reader.read_event().map_err(|error| {
            Error::insert_package(format!("invalid donor worksheet XML: {error}"))
        })?;
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"sheetView" => {
                write_event(
                    &mut writer,
                    Event::Start(replace_optional_attribute(
                        &start,
                        b"tabSelected",
                        target_value.as_deref(),
                    )?),
                )?;
            }
            Event::Empty(empty) if local_name(empty.name().as_ref()) == b"sheetView" => {
                write_event(
                    &mut writer,
                    Event::Empty(replace_optional_attribute(
                        &empty,
                        b"tabSelected",
                        target_value.as_deref(),
                    )?),
                )?;
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
    }
    Ok(writer.into_inner())
}

fn first_sheet_view_attribute(xml: &[u8], attribute_name: &[u8]) -> Result<Option<String>> {
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid worksheet XML: {error}")))?
        {
            Event::Start(start) | Event::Empty(start)
                if local_name(start.name().as_ref()) == b"sheetView" =>
            {
                return xml_attribute(&start, attribute_name);
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}

fn replace_optional_attribute(
    element: &BytesStart<'_>,
    attribute_name: &[u8],
    replacement: Option<&str>,
) -> Result<BytesStart<'static>> {
    let qualified_name = element.name();
    let name = std::str::from_utf8(qualified_name.as_ref())
        .map_err(|_| Error::insert_package("worksheet element name is not UTF-8"))?;
    let mut output = BytesStart::new(name.to_owned());
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| {
            Error::insert_package(format!("invalid worksheet attribute: {error}"))
        })?;
        if local_name(attribute.key.as_ref()) == attribute_name {
            continue;
        }
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|_| Error::insert_package("worksheet attribute name is not UTF-8"))?;
        let value = attribute
            .decode_and_unescape_value(element.decoder())
            .map_err(|error| Error::insert_package(format!("invalid worksheet value: {error}")))?;
        output.push_attribute((key, value.as_ref()));
    }
    if let Some(replacement) = replacement {
        let name = std::str::from_utf8(attribute_name)
            .map_err(|_| Error::insert_package("worksheet attribute name is not UTF-8"))?;
        output.push_attribute((name, replacement));
    }
    Ok(output)
}

fn relationship_part_path(source: &str) -> Result<String> {
    let (directory, name) = source.rsplit_once('/').map_or(("", source), |value| value);
    if name.is_empty() {
        return Err(Error::insert_package(format!(
            "cannot derive relationship path for '{source}'"
        )));
    }
    if directory.is_empty() {
        Ok(format!("_rels/{name}.rels"))
    } else {
        Ok(format!("{directory}/_rels/{name}.rels"))
    }
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
    visibility: SheetVisibility,
    allocation: &WorksheetAllocation,
    local_sheet_id: usize,
    local_defined_names: &[DefinedName],
) -> Result<Vec<u8>> {
    let has_defined_names = has_direct_workbook_child(xml, b"definedNames")?;
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
                    visibility,
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

fn remove_relationship_by_id(xml: &[u8], relationship_id: Option<&str>) -> Result<Vec<u8>> {
    let Some(relationship_id) = relationship_id else {
        return Ok(xml.to_vec());
    };
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut removed = 0;
    let mut skip_depth = 0_usize;
    loop {
        let event = reader.read_event().map_err(|error| {
            Error::insert_package(format!("invalid workbook relationships XML: {error}"))
        })?;
        if skip_depth > 0 {
            match event {
                Event::Start(_) => skip_depth += 1,
                Event::End(_) => skip_depth -= 1,
                Event::Eof => {
                    return Err(Error::insert_package(
                        "unterminated removed workbook relationship",
                    ));
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"Relationship" => {
                if xml_attribute(&start, b"Id")?.as_deref() == Some(relationship_id) {
                    removed += 1;
                    skip_depth = 1;
                } else {
                    write_event(&mut writer, Event::Start(start))?;
                }
            }
            Event::Empty(empty) if local_name(empty.name().as_ref()) == b"Relationship" => {
                if xml_attribute(&empty, b"Id")?.as_deref() == Some(relationship_id) {
                    removed += 1;
                } else {
                    write_event(&mut writer, Event::Empty(empty))?;
                }
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
    }
    if removed != 1 {
        return Err(Error::insert_package(format!(
            "workbook contains {removed} relationships with ID '{relationship_id}'"
        )));
    }
    Ok(writer.into_inner())
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

fn remove_content_type_overrides(xml: &[u8], removed: &BTreeSet<String>) -> Result<Vec<u8>> {
    if removed.is_empty() {
        return Ok(xml.to_vec());
    }
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut skip_depth = 0_usize;
    loop {
        let event = reader.read_event().map_err(|error| {
            Error::insert_package(format!("invalid content types XML: {error}"))
        })?;
        if skip_depth > 0 {
            match event {
                Event::Start(_) => skip_depth += 1,
                Event::End(_) => skip_depth -= 1,
                Event::Eof => {
                    return Err(Error::insert_package(
                        "unterminated removed content-type override",
                    ));
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"Override" => {
                if override_is_removed(&start, removed)? {
                    skip_depth = 1;
                } else {
                    write_event(&mut writer, Event::Start(start))?;
                }
            }
            Event::Empty(empty) if local_name(empty.name().as_ref()) == b"Override" => {
                if !override_is_removed(&empty, removed)? {
                    write_event(&mut writer, Event::Empty(empty))?;
                }
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
    }
    Ok(writer.into_inner())
}

fn override_is_removed(element: &BytesStart<'_>, removed: &BTreeSet<String>) -> Result<bool> {
    let Some(part_name) = xml_attribute(element, b"PartName")? else {
        return Err(Error::insert_package("content-type override has no PartName"));
    };
    Ok(removed.contains(part_name.trim_start_matches('/')))
}

fn replace_target_filter_defined_name(
    xml: &[u8],
    plan: &ReplacementPlan,
    donor: &DonorWorksheet,
) -> Result<Vec<u8>> {
    const FILTER_NAME: &str = "_xlnm._FilterDatabase";
    let has_defined_names = has_direct_workbook_child(xml, b"definedNames")?;
    let retained_names = count_retained_defined_names(xml, plan.target.index)?;
    let donor_names = donor
        .local_defined_names
        .iter()
        .filter(|name| name.name == FILTER_NAME)
        .collect::<Vec<_>>();
    let keep_container = retained_names + donor_names.len() > 0;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len() + 128));
    let mut depth = 0_usize;
    let mut in_defined_names = false;
    let mut skip_depth = 0_usize;
    let mut pending_defined_names = false;
    let mut element_prefix = String::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid workbook XML: {error}")))?;
        if skip_depth > 0 {
            match event {
                Event::Start(_) => {
                    skip_depth += 1;
                    depth += 1;
                }
                Event::End(_) => {
                    skip_depth -= 1;
                    depth = depth.saturating_sub(1);
                }
                Event::Eof => {
                    return Err(Error::insert_package("unterminated definedName element"));
                }
                _ => {}
            }
            continue;
        }
        if pending_defined_names && is_direct_child_after_defined_names(&event, depth) {
            write_replacement_defined_names(
                &mut writer,
                &donor_names,
                &plan.target,
                &element_prefix,
            )?;
            pending_defined_names = false;
        }
        match event {
            Event::Start(start) if local_name(start.name().as_ref()) == b"definedNames" => {
                in_defined_names = true;
                element_prefix = element_prefix_from_name(start.name().as_ref())?;
                depth += 1;
                if keep_container {
                    write_event(&mut writer, Event::Start(start))?;
                } else {
                    skip_depth = 1;
                }
            }
            Event::Empty(empty) if local_name(empty.name().as_ref()) == b"definedNames" => {
                element_prefix = element_prefix_from_name(empty.name().as_ref())?;
                if !donor_names.is_empty() {
                    write_replacement_defined_names(
                        &mut writer,
                        &donor_names,
                        &plan.target,
                        &element_prefix,
                    )?;
                }
            }
            Event::Start(start)
                if in_defined_names
                    && depth == 2
                    && local_name(start.name().as_ref()) == b"definedName"
                    && defined_name_is_target_filter(&start, plan.target.index)? =>
            {
                skip_depth = 1;
                depth += 1;
            }
            Event::Empty(empty)
                if in_defined_names
                    && depth == 2
                    && local_name(empty.name().as_ref()) == b"definedName"
                    && defined_name_is_target_filter(&empty, plan.target.index)? => {}
            Event::End(end) if local_name(end.name().as_ref()) == b"definedNames" => {
                for name in &donor_names {
                    write_retargeted_defined_name(
                        &mut writer,
                        name,
                        &plan.target,
                        &element_prefix,
                    )?;
                }
                write_event(&mut writer, Event::End(end))?;
                in_defined_names = false;
                depth = depth.saturating_sub(1);
            }
            Event::End(end) if local_name(end.name().as_ref()) == b"sheets" => {
                element_prefix = element_prefix_from_name(end.name().as_ref())?;
                write_event(&mut writer, Event::End(end))?;
                depth = depth.saturating_sub(1);
                if !has_defined_names && !donor_names.is_empty() {
                    pending_defined_names = true;
                }
            }
            Event::Start(start) => {
                write_event(&mut writer, Event::Start(start))?;
                depth += 1;
            }
            Event::End(end) => {
                write_event(&mut writer, Event::End(end))?;
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
    }
    if pending_defined_names {
        return Err(Error::insert_package(
            "workbook ended before replacement definedNames could be inserted",
        ));
    }
    Ok(writer.into_inner())
}

fn force_full_calculation(xml: &[u8]) -> Result<Vec<u8>> {
    let has_calc_pr = has_direct_workbook_child(xml, b"calcPr")?;
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len() + 96));
    let mut depth = 0_usize;
    let mut calc_pr_count = 0_usize;
    let mut workbook_prefix = String::new();
    let mut inserted = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid workbook XML: {error}")))?;
        if !has_calc_pr && !inserted && is_direct_child_after_calc_pr(&event, depth) {
            write_calc_pr(&mut writer, &workbook_prefix)?;
            inserted = true;
        }
        match event {
            Event::Start(start)
                if depth == 0 && local_name(start.name().as_ref()) == b"workbook" =>
            {
                workbook_prefix = element_prefix(start.name().as_ref())?;
                write_event(&mut writer, Event::Start(start))?;
                depth += 1;
            }
            Event::Start(start) if depth == 1 && local_name(start.name().as_ref()) == b"calcPr" => {
                write_event(&mut writer, Event::Start(force_calc_attributes(&start)?))?;
                calc_pr_count += 1;
                depth += 1;
            }
            Event::Empty(empty) if depth == 1 && local_name(empty.name().as_ref()) == b"calcPr" => {
                write_event(&mut writer, Event::Empty(force_calc_attributes(&empty)?))?;
                calc_pr_count += 1;
            }
            Event::Start(start) => {
                write_event(&mut writer, Event::Start(start))?;
                depth += 1;
            }
            Event::End(end) => {
                write_event(&mut writer, Event::End(end))?;
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
    }
    if has_calc_pr && calc_pr_count != 1 {
        return Err(Error::unsafe_package(format!(
            "workbook contains {calc_pr_count} calcPr elements"
        )));
    }
    if !has_calc_pr && !inserted {
        return Err(Error::insert_package("workbook ended before calcPr could be inserted"));
    }
    Ok(writer.into_inner())
}

fn has_direct_workbook_child(xml: &[u8], child_name: &[u8]) -> Result<bool> {
    let mut reader = Reader::from_reader(xml);
    let mut depth = 0_usize;
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid workbook XML: {error}")))?
        {
            Event::Start(start) => {
                if depth == 1 && local_name(start.name().as_ref()) == child_name {
                    return Ok(true);
                }
                depth += 1;
            }
            Event::Empty(empty)
                if depth == 1 && local_name(empty.name().as_ref()) == child_name =>
            {
                return Ok(true);
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn force_calc_attributes(element: &BytesStart<'_>) -> Result<BytesStart<'static>> {
    let full_calc = replace_optional_attribute(element, b"fullCalcOnLoad", Some("1"))?;
    replace_optional_attribute(&full_calc, b"forceFullCalc", Some("1"))
}

fn write_calc_pr(writer: &mut Writer<Vec<u8>>, element_prefix: &str) -> Result<()> {
    let mut calc_pr = BytesStart::new(qualify(element_prefix, "calcPr"));
    calc_pr.push_attribute(("fullCalcOnLoad", "1"));
    calc_pr.push_attribute(("forceFullCalc", "1"));
    write_event(writer, Event::Empty(calc_pr))
}

fn is_direct_child_after_calc_pr(event: &Event<'_>, depth: usize) -> bool {
    if depth != 1 {
        return false;
    }
    match event {
        Event::Start(start) | Event::Empty(start) => matches!(
            local_name(start.name().as_ref()),
            b"oleSize"
                | b"customWorkbookViews"
                | b"pivotCaches"
                | b"smartTagPr"
                | b"smartTagTypes"
                | b"webPublishing"
                | b"fileRecoveryPr"
                | b"webPublishObjects"
                | b"extLst"
        ),
        Event::End(end) => local_name(end.name().as_ref()) == b"workbook",
        _ => false,
    }
}

fn count_retained_defined_names(xml: &[u8], target_index: usize) -> Result<usize> {
    let mut reader = Reader::from_reader(xml);
    let mut count = 0;
    let mut depth = 0_usize;
    let mut defined_names_depth = None;
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid workbook XML: {error}")))?
        {
            Event::Start(start) => {
                if depth == 1 && local_name(start.name().as_ref()) == b"definedNames" {
                    defined_names_depth = Some(depth + 1);
                } else if defined_names_depth == Some(depth)
                    && local_name(start.name().as_ref()) == b"definedName"
                    && !defined_name_is_target_filter(&start, target_index)?
                {
                    count += 1;
                }
                depth += 1;
            }
            Event::Empty(empty)
                if defined_names_depth == Some(depth)
                    && local_name(empty.name().as_ref()) == b"definedName"
                    && !defined_name_is_target_filter(&empty, target_index)? =>
            {
                count += 1;
            }
            Event::End(end) => {
                depth = depth.saturating_sub(1);
                if defined_names_depth == Some(depth + 1)
                    && local_name(end.name().as_ref()) == b"definedNames"
                {
                    defined_names_depth = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(count)
}

fn defined_name_is_target_filter(element: &BytesStart<'_>, target_index: usize) -> Result<bool> {
    let name = xml_attribute(element, b"name")?;
    let local_sheet_id =
        xml_attribute(element, b"localSheetId")?.and_then(|value| value.parse::<usize>().ok());
    Ok(name.as_deref() == Some("_xlnm._FilterDatabase") && local_sheet_id == Some(target_index))
}

fn write_replacement_defined_names(
    writer: &mut Writer<Vec<u8>>,
    names: &[&DefinedName],
    target: &WorkbookSheet,
    element_prefix: &str,
) -> Result<()> {
    write_event(writer, Event::Start(BytesStart::new(qualify(element_prefix, "definedNames"))))?;
    for name in names {
        write_retargeted_defined_name(writer, name, target, element_prefix)?;
    }
    write_event(writer, Event::End(BytesEnd::new(qualify(element_prefix, "definedNames"))))
}

fn write_retargeted_defined_name(
    writer: &mut Writer<Vec<u8>>,
    name: &DefinedName,
    target: &WorkbookSheet,
    element_prefix: &str,
) -> Result<()> {
    let range = name
        .formula
        .rsplit_once('!')
        .map(|(_, range)| range)
        .ok_or_else(|| Error::insert_package("AutoFilter defined name has no range"))?;
    let escaped_name = target.name.replace('\'', "''");
    let formula = format!("'{escaped_name}'!{range}");
    write_defined_name_formula(writer, name, target.index, element_prefix, &formula)
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
    visibility: SheetVisibility,
    allocation: &WorksheetAllocation,
    element_prefix: &str,
    relationship_prefix: &str,
) -> Result<()> {
    let mut sheet = BytesStart::new(qualify(element_prefix, "sheet"));
    let sheet_id = allocation.sheet_id.to_string();
    let relationship_id_name = qualify(relationship_prefix, "id");
    sheet.push_attribute(("name", sheet_name));
    sheet.push_attribute(("sheetId", sheet_id.as_str()));
    match visibility {
        SheetVisibility::Visible => {}
        SheetVisibility::Hidden => sheet.push_attribute(("state", "hidden")),
        SheetVisibility::VeryHidden => sheet.push_attribute(("state", "veryHidden")),
    }
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
    write_defined_name_formula(
        writer,
        defined_name,
        local_sheet_id,
        element_prefix,
        &defined_name.formula,
    )
}

fn write_defined_name_formula(
    writer: &mut Writer<Vec<u8>>,
    defined_name: &DefinedName,
    local_sheet_id: usize,
    element_prefix: &str,
    formula: &str,
) -> Result<()> {
    let mut element = BytesStart::new(qualify(element_prefix, "definedName"));
    let local_sheet_id = local_sheet_id.to_string();
    element.push_attribute(("name", defined_name.name.as_str()));
    element.push_attribute(("localSheetId", local_sheet_id.as_str()));
    if defined_name.hidden {
        element.push_attribute(("hidden", "1"));
    }
    write_event(writer, Event::Start(element))?;
    write_event(writer, Event::Text(BytesText::new(formula)))?;
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

fn element_prefix_from_name(name: &[u8]) -> Result<String> {
    element_prefix(name)
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

fn xml_attribute(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| Error::insert_package(format!("invalid XML attribute: {error}")))?;
        if local_name(attribute.key.as_ref()) == name {
            return attribute
                .decode_and_unescape_value(event.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| Error::insert_package(format!("invalid XML value: {error}")));
        }
    }
    Ok(None)
}

fn write_package<R, W, F>(
    mut archive: ZipArchive<R>,
    destination: W,
    replacements: &BTreeMap<String, Vec<u8>>,
    removed_entries: &BTreeSet<String>,
    new_entry: Option<(&str, &[u8])>,
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
        if removed_entries.contains(&name) {
            continue;
        }
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

    if let Some((worksheet_path, worksheet_xml)) = new_entry {
        writer
            .start_file(
                worksheet_path,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|error| {
                Error::insert_package(format!(
                    "cannot create worksheet '{worksheet_path}': {error}"
                ))
            })?;
        writer.write_all(worksheet_xml)?;
    }
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
    use crate::insert::package::{ContentTypes, PackageRelationship, WorkbookSheet};
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
    fn replacement_plan_preserves_shared_parts_and_rejects_unknown_relationships() {
        let mut inventory = PackageInventory {
            entry_names: BTreeSet::from([
                "xl/styles.xml".to_owned(),
                "xl/worksheets/target.xml".to_owned(),
                "xl/worksheets/other.xml".to_owned(),
                "xl/worksheets/_rels/target.xml.rels".to_owned(),
                "xl/drawings/drawing1.xml".to_owned(),
                "xl/drawings/_rels/drawing1.xml.rels".to_owned(),
                "xl/media/shared.png".to_owned(),
            ]),
            content_types: ContentTypes::default(),
            relationships: vec![
                relationship("xl/workbook.xml", "rIdStyles", "styles", "xl/styles.xml"),
                relationship(
                    "xl/worksheets/target.xml",
                    "rIdDrawing",
                    "drawing",
                    "xl/drawings/drawing1.xml",
                ),
                relationship(
                    "xl/drawings/drawing1.xml",
                    "rIdImage",
                    "image",
                    "xl/media/shared.png",
                ),
                relationship(
                    "xl/worksheets/other.xml",
                    "rIdShared",
                    "image",
                    "xl/media/shared.png",
                ),
            ],
            sheets: vec![
                worksheet("Target", "xl/worksheets/target.xml", 0),
                worksheet("Other", "xl/worksheets/other.xml", 1),
            ],
            views: Vec::new(),
            defined_names: Vec::new(),
        };

        let plan =
            plan_replacement(&inventory, "Target", TargetRelationshipPolicy::RemoveSupported)
                .unwrap();
        assert!(plan.removed_entries.contains("xl/drawings/drawing1.xml"));
        assert!(plan.removed_entries.contains("xl/drawings/_rels/drawing1.xml.rels"));
        assert!(!plan.removed_entries.contains("xl/media/shared.png"));

        inventory.relationships.push(relationship(
            "xl/worksheets/target.xml",
            "rIdPivot",
            "pivotTable",
            "xl/pivotTables/pivotTable1.xml",
        ));
        assert!(
            plan_replacement(&inventory, "Target", TargetRelationshipPolicy::RemoveSupported,)
                .is_err()
        );

        let mut unknown_uri = inventory.clone();
        unknown_uri.relationships.pop();
        unknown_uri.relationships.push(PackageRelationship {
            source: Some("xl/worksheets/target.xml".to_owned()),
            id: "rIdFake".to_owned(),
            relationship_type: "https://example.invalid/relationships/table".to_owned(),
            target: "xl/tables/fake.xml".to_owned(),
            normalized_target: Some("xl/tables/fake.xml".to_owned()),
            target_mode: None,
        });
        assert!(
            plan_replacement(&unknown_uri, "Target", TargetRelationshipPolicy::RemoveSupported,)
                .is_err()
        );

        let mut external_image = inventory;
        external_image.relationships[2].normalized_target = None;
        external_image.relationships[2].target = "https://example.invalid/image.png".to_owned();
        external_image.relationships[2].target_mode = Some("External".to_owned());
        assert!(
            plan_replacement(&external_image, "Target", TargetRelationshipPolicy::RemoveSupported,)
                .is_err()
        );
    }

    #[test]
    fn replacement_plan_rejects_calc_chain_relationship_to_non_chain_part() {
        let mut inventory = PackageInventory {
            entry_names: BTreeSet::from([
                "xl/styles.xml".to_owned(),
                "xl/worksheets/target.xml".to_owned(),
            ]),
            content_types: ContentTypes {
                defaults: Vec::new(),
                overrides: vec![super::super::package::ContentTypeOverride {
                    part_name: "xl/styles.xml".to_owned(),
                    content_type:
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"
                            .to_owned(),
                }],
            },
            relationships: vec![
                relationship("xl/workbook.xml", "rIdStyles", "styles", "xl/styles.xml"),
                relationship("xl/workbook.xml", "rIdCalc", "calcChain", "xl/styles.xml"),
            ],
            sheets: vec![worksheet("Target", "xl/worksheets/target.xml", 0)],
            views: Vec::new(),
            defined_names: Vec::new(),
        };
        assert!(plan_replacement(&inventory, "Target", TargetRelationshipPolicy::Reject).is_err());

        inventory.content_types.overrides[0].content_type = CALC_CHAIN_CONTENT_TYPE.to_owned();
        assert!(plan_replacement(&inventory, "Target", TargetRelationshipPolicy::Reject).is_err());
    }

    #[test]
    fn full_calculation_patch_preserves_attributes_and_inserts_direct_child() {
        let existing = br#"<workbook><calcPr calcId="77" calcMode="manual" fullCalcOnLoad="0" forceFullCalc="0"/><extLst/></workbook>"#;
        let patched = String::from_utf8(force_full_calculation(existing).unwrap()).unwrap();
        assert!(patched.contains("calcId=\"77\""));
        assert!(patched.contains("calcMode=\"manual\""));
        assert!(patched.contains("fullCalcOnLoad=\"1\""));
        assert!(patched.contains("forceFullCalc=\"1\""));
        assert_eq!(patched.matches("<calcPr ").count(), 1);

        let missing = br#"<workbook><sheets/><extLst><calcPr foreign="1"/></extLst></workbook>"#;
        let patched = String::from_utf8(force_full_calculation(missing).unwrap()).unwrap();
        assert_eq!(patched.matches("<calcPr ").count(), 2);
        let direct = patched.find("fullCalcOnLoad=\"1\"").unwrap();
        let ext = patched.find("<extLst>").unwrap();
        assert!(direct < ext);
        assert!(patched.contains("<calcPr foreign=\"1\"/>"));
    }

    fn relationship(
        source: &str,
        id: &str,
        kind: &str,
        normalized_target: &str,
    ) -> PackageRelationship {
        PackageRelationship {
            source: Some(source.to_owned()),
            id: id.to_owned(),
            relationship_type: format!(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}"
            ),
            target: normalized_target.to_owned(),
            normalized_target: Some(normalized_target.to_owned()),
            target_mode: None,
        }
    }

    fn worksheet(name: &str, target: &str, index: usize) -> WorkbookSheet {
        WorkbookSheet {
            index,
            name: name.to_owned(),
            sheet_id: index as u32 + 1,
            relationship_id: format!("rId{}", index + 1),
            target: target.to_owned(),
            visibility: SheetVisibility::Visible,
        }
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
        let patched = append_workbook(
            workbook,
            "New Data",
            SheetVisibility::Visible,
            &allocation,
            1,
            &[defined_name],
        )
        .unwrap();
        let text = String::from_utf8(patched).unwrap();
        assert!(text.contains("<definedNames><definedName"));
        assert!(text.find("</sheets>").unwrap() < text.find("<definedNames>").unwrap());
        assert!(text.find("</externalReferences>").unwrap() < text.find("<definedNames>").unwrap());
        assert!(text.find("</definedNames>").unwrap() < text.find("<calcPr").unwrap());

        let types = br#"<Types><Override PartName="/xl/worksheets/new.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
        assert_eq!(append_content_type_override(types, &allocation, false).unwrap(), types);

        let prefixed_workbook = br#"<x:workbook xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:q="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x:sheets><x:sheet name="A" sheetId="1" q:id="rId1"/></x:sheets><x:calcPr calcId="7"/></x:workbook>"#;
        let prefixed = String::from_utf8(
            append_workbook(
                prefixed_workbook,
                "New Data",
                SheetVisibility::Visible,
                &allocation,
                1,
                &[],
            )
            .unwrap(),
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
