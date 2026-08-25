use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use crate::writer::validate_sheet_name;
use crate::{Error, Result, SheetVisibility};

const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
const WORKBOOK_PATH: &str = "xl/workbook.xml";
const WORKBOOK_RELS_PATH: &str = "xl/_rels/workbook.xml.rels";
const MAX_PACKAGE_ENTRIES: usize = 65_535;
const MAX_CONTROL_PART_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_CONTROL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_XML_ATTRIBUTE_BYTES: usize = 64 * 1024;
const MAX_XML_DEPTH: usize = 256;
const MAX_RELATIONSHIPS: usize = 262_144;

#[derive(Default)]
struct ControlXmlBudget {
    bytes: u64,
}

impl ControlXmlBudget {
    fn consume(&mut self, bytes: u64) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| Error::unsafe_package("control XML aggregate size overflow"))?;
        if self.bytes > MAX_TOTAL_CONTROL_BYTES {
            return Err(Error::unsafe_package(format!(
                "control XML aggregate size exceeds limit {MAX_TOTAL_CONTROL_BYTES}"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContentTypeDefault {
    pub(crate) extension: String,
    pub(crate) content_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContentTypeOverride {
    pub(crate) part_name: String,
    pub(crate) content_type: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ContentTypes {
    pub(crate) defaults: Vec<ContentTypeDefault>,
    pub(crate) overrides: Vec<ContentTypeOverride>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackageRelationship {
    pub(crate) source: Option<String>,
    pub(crate) id: String,
    pub(crate) relationship_type: String,
    pub(crate) target: String,
    pub(crate) normalized_target: Option<String>,
    pub(crate) target_mode: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkbookSheet {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) sheet_id: u32,
    pub(crate) relationship_id: String,
    pub(crate) target: String,
    pub(crate) visibility: SheetVisibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkbookView {
    pub(crate) active_tab: usize,
    pub(crate) first_sheet: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DefinedName {
    pub(crate) name: String,
    pub(crate) local_sheet_id: Option<usize>,
    pub(crate) hidden: bool,
    pub(crate) formula: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorksheetAllocation {
    pub(crate) sheet_id: u32,
    pub(crate) relationship_id: String,
    pub(crate) workbook_target: String,
    pub(crate) package_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackageInventory {
    pub(crate) entry_names: BTreeSet<String>,
    pub(crate) content_types: ContentTypes,
    pub(crate) relationships: Vec<PackageRelationship>,
    pub(crate) sheets: Vec<WorkbookSheet>,
    pub(crate) views: Vec<WorkbookView>,
    pub(crate) defined_names: Vec<DefinedName>,
}

impl PackageInventory {
    pub(crate) fn inspect<R>(reader: R) -> Result<Self>
    where
        R: Read + Seek,
    {
        let mut reader = reader;
        let central_directory_start = {
            let archive = ZipArchive::new(&mut reader)
                .map_err(|error| Error::insert_package(format!("cannot open XLSX ZIP: {error}")))?;
            if archive.len() > MAX_PACKAGE_ENTRIES {
                return Err(Error::unsafe_package(format!(
                    "ZIP entry count {} exceeds limit {MAX_PACKAGE_ENTRIES}",
                    archive.len()
                )));
            }
            archive.central_directory_start()
        };
        let raw_entry_count = inspect_central_directory(&mut reader, central_directory_start)?;
        let mut archive = ZipArchive::new(reader)
            .map_err(|error| Error::insert_package(format!("cannot open XLSX ZIP: {error}")))?;
        if raw_entry_count != archive.len() {
            return Err(Error::unsafe_package(
                "ZIP central directory contains duplicate entry names",
            ));
        }
        let entry_names = inspect_entries(&mut archive)?;
        let mut control_budget = ControlXmlBudget::default();
        let content_types = parse_content_types(&read_control_part(
            &mut archive,
            CONTENT_TYPES_PATH,
            &mut control_budget,
        )?)?;
        validate_content_type_uniqueness(&content_types)?;
        reject_unsupported_content_types(&content_types)?;

        let mut relationships = Vec::new();
        let relationship_paths =
            entry_names.iter().filter(|name| name.ends_with(".rels")).cloned().collect::<Vec<_>>();
        for path in relationship_paths {
            let source = relationship_source(&path)?;
            let xml = read_control_part(&mut archive, &path, &mut control_budget)?;
            relationships.extend(parse_relationships(source.as_deref(), &xml)?);
            if relationships.len() > MAX_RELATIONSHIPS {
                return Err(Error::unsafe_package(format!(
                    "relationship count {} exceeds limit {MAX_RELATIONSHIPS}",
                    relationships.len()
                )));
            }
        }
        reject_unsupported_relationships(&relationships)?;
        validate_relationship_id_uniqueness(&relationships)?;
        validate_relationship_target_uniqueness(&relationships)?;
        validate_relationship_cycles(&relationships)?;

        let workbook_xml = read_control_part(&mut archive, WORKBOOK_PATH, &mut control_budget)?;
        let (sheet_elements, views, defined_names) = parse_workbook(&workbook_xml)?;
        let workbook_relationships = relationships
            .iter()
            .filter(|relationship| relationship.source.as_deref() == Some(WORKBOOK_PATH))
            .map(|relationship| (relationship.id.as_str(), relationship))
            .collect::<BTreeMap<_, _>>();
        let mut sheets = Vec::with_capacity(sheet_elements.len());
        let mut names = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for (index, sheet) in sheet_elements.into_iter().enumerate() {
            let normalized_name = normalize_sheet_name(&sheet.name);
            if !names.insert(normalized_name) {
                return Err(Error::unsafe_package(format!(
                    "worksheet name '{}' is duplicated case-insensitively",
                    sheet.name
                )));
            }
            if !ids.insert(sheet.sheet_id) {
                return Err(Error::unsafe_package(format!(
                    "worksheet sheetId '{}' is duplicated",
                    sheet.sheet_id
                )));
            }
            let relationship =
                workbook_relationships.get(sheet.relationship_id.as_str()).ok_or_else(|| {
                    Error::insert_package(format!(
                        "worksheet '{}' references missing relationship '{}'",
                        sheet.name, sheet.relationship_id
                    ))
                })?;
            if relationship.relationship_type.rsplit('/').next() != Some("worksheet") {
                return Err(Error::insert_package(format!(
                    "worksheet '{}' relationship '{}' is not a worksheet",
                    sheet.name, sheet.relationship_id
                )));
            }
            let target = relationship.normalized_target.clone().ok_or_else(|| {
                Error::insert_package(format!("worksheet '{}' uses an external target", sheet.name))
            })?;
            if !entry_names.contains(&target) {
                return Err(Error::insert_package(format!(
                    "worksheet '{}' target '{}' is missing",
                    sheet.name, target
                )));
            }
            if !targets.insert(target.clone()) {
                return Err(Error::unsafe_package(format!(
                    "worksheet target '{target}' is referenced more than once"
                )));
            }
            sheets.push(WorkbookSheet {
                index,
                name: sheet.name,
                sheet_id: sheet.sheet_id,
                relationship_id: sheet.relationship_id,
                target,
                visibility: sheet.visibility,
            });
        }
        if sheets.is_empty() {
            return Err(Error::no_worksheets());
        }
        if views.iter().any(|view| view.active_tab >= sheets.len()) {
            return Err(Error::unsafe_package("workbook activeTab is outside the sheet list"));
        }

        Ok(Self { entry_names, content_types, relationships, sheets, views, defined_names })
    }

    pub(crate) fn find_sheet(&self, name: &str) -> Option<&WorkbookSheet> {
        let normalized = normalize_sheet_name(name);
        self.sheets.iter().find(|sheet| normalize_sheet_name(&sheet.name) == normalized)
    }

    pub(crate) fn ensure_sheet_absent(&self, name: &str) -> Result<()> {
        validate_sheet_name(name, &std::collections::HashSet::new())?;
        if self.find_sheet(name).is_some() {
            return Err(Error::existing_worksheet(name));
        }
        Ok(())
    }

    pub(crate) fn allocate_worksheet(&self) -> Result<WorksheetAllocation> {
        let sheet_id = self
            .sheets
            .iter()
            .map(|sheet| sheet.sheet_id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| Error::insert_package("worksheet sheetId space is exhausted"))?;
        let workbook_relationship_ids = self
            .relationships
            .iter()
            .filter(|relationship| relationship.source.as_deref() == Some(WORKBOOK_PATH))
            .map(|relationship| relationship.id.as_str())
            .collect::<BTreeSet<_>>();
        let relationship_number = (1_u32..)
            .find(|number| !workbook_relationship_ids.contains(format!("rId{number}").as_str()))
            .ok_or_else(|| Error::insert_package("workbook relationship ID space is exhausted"))?;
        let relationship_id = format!("rId{relationship_number}");

        let worksheet_number = (1_u32..)
            .find(|number| !self.entry_names.contains(&format!("xl/worksheets/sheet{number}.xml")))
            .ok_or_else(|| Error::insert_package("worksheet path space is exhausted"))?;
        Ok(WorksheetAllocation {
            sheet_id,
            relationship_id,
            workbook_target: format!("worksheets/sheet{worksheet_number}.xml"),
            package_path: format!("xl/worksheets/sheet{worksheet_number}.xml"),
        })
    }
}

fn inspect_central_directory<R>(reader: &mut R, start: u64) -> Result<usize>
where
    R: Read + Seek,
{
    reader.seek(SeekFrom::Start(start))?;
    let mut count = 0;
    loop {
        let mut signature = [0; 4];
        reader.read_exact(&mut signature)?;
        if signature != [0x50, 0x4B, 0x01, 0x02] {
            break;
        }
        let mut header = [0; 42];
        reader.read_exact(&mut header)?;
        let flags = u16::from_le_bytes([header[4], header[5]]);
        if flags & 1 != 0 {
            return Err(Error::unsupported_package_feature(
                "encrypted ZIP central-directory entry",
            ));
        }
        let name_length = u16::from_le_bytes([header[24], header[25]]) as i64;
        let extra_length = u16::from_le_bytes([header[26], header[27]]) as i64;
        let comment_length = u16::from_le_bytes([header[28], header[29]]) as i64;
        reader.seek(SeekFrom::Current(name_length + extra_length + comment_length))?;
        count += 1;
        if count > MAX_PACKAGE_ENTRIES {
            return Err(Error::unsafe_package(format!(
                "ZIP entry count exceeds limit {MAX_PACKAGE_ENTRIES}"
            )));
        }
    }
    Ok(count)
}

#[derive(Debug)]
struct SheetElement {
    name: String,
    sheet_id: u32,
    relationship_id: String,
    visibility: SheetVisibility,
}

fn inspect_entries<R>(archive: &mut ZipArchive<R>) -> Result<BTreeSet<String>>
where
    R: Read + Seek,
{
    let mut names = BTreeSet::new();
    let mut normalized_names = BTreeSet::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|error| Error::insert_package(format!("cannot inspect ZIP entry: {error}")))?;
        if file.encrypted() {
            return Err(Error::unsupported_package_feature(format!(
                "encrypted ZIP entry '{}'",
                file.name()
            )));
        }
        let name = validate_entry_name(file.name())?;
        if !names.insert(name.clone()) {
            return Err(Error::unsafe_package(format!("duplicate ZIP entry '{name}'")));
        }
        let normalized = canonical_part_identity(&name)?;
        if !normalized_names.insert(normalized) {
            return Err(Error::unsafe_package(format!(
                "ZIP entry '{name}' collides case-insensitively"
            )));
        }
    }
    for required in [CONTENT_TYPES_PATH, WORKBOOK_PATH, WORKBOOK_RELS_PATH] {
        if !names.contains(required) {
            return Err(Error::insert_package(format!(
                "required package part '{required}' is missing"
            )));
        }
    }
    if names.iter().any(|name| name.to_ascii_lowercase().starts_with("_xmlsignatures/")) {
        return Err(Error::unsupported_package_feature("digitally signed OPC package"));
    }
    Ok(names)
}

fn validate_entry_name(name: &str) -> Result<String> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains(['?', '#'])
        || name.split('/').any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
        || name.as_bytes().get(1) == Some(&b':')
    {
        return Err(Error::unsafe_package(format!("unsafe ZIP entry path '{name}'")));
    }
    Ok(name.to_owned())
}

fn read_control_part<R>(
    archive: &mut ZipArchive<R>,
    path: &str,
    budget: &mut ControlXmlBudget,
) -> Result<Vec<u8>>
where
    R: Read + Seek,
{
    let file = archive
        .by_name(path)
        .map_err(|error| Error::insert_package(format!("cannot read '{path}': {error}")))?;
    if file.size() > MAX_CONTROL_PART_BYTES {
        return Err(Error::unsafe_package(format!("control part '{path}' is too large")));
    }
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.take(MAX_CONTROL_PART_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONTROL_PART_BYTES {
        return Err(Error::unsafe_package(format!("control part '{path}' is too large")));
    }
    budget.consume(bytes.len() as u64)?;
    validate_control_xml(&bytes, path)?;
    Ok(bytes)
}

fn validate_control_xml(xml: &[u8], path: &str) -> Result<()> {
    const STRICT_NAMESPACE_PREFIX: &str = "http://purl.oclc.org/ooxml/";
    let mut reader = Reader::from_reader(xml);
    let mut depth = 0_usize;
    loop {
        match reader.read_event().map_err(|error| {
            Error::insert_package(format!("invalid control XML '{path}': {error}"))
        })? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return Err(Error::unsafe_package(format!(
                        "control part '{path}' exceeds XML depth limit {MAX_XML_DEPTH}"
                    )));
                }
                validate_control_attributes(&event, path, STRICT_NAMESPACE_PREFIX)?;
            }
            Event::Empty(event) => {
                validate_control_attributes(&event, path, STRICT_NAMESPACE_PREFIX)?;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn validate_control_attributes(
    event: &BytesStart<'_>,
    path: &str,
    strict_namespace_prefix: &str,
) -> Result<()> {
    for attribute in event.attributes().with_checks(true) {
        let attribute = attribute.map_err(|error| {
            Error::insert_package(format!(
                "invalid XML attribute in control part '{path}': {error}"
            ))
        })?;
        if attribute.value.len() > MAX_XML_ATTRIBUTE_BYTES {
            return Err(Error::unsafe_package(format!(
                "control part '{path}' contains an oversized XML attribute"
            )));
        }
        let value = attribute.decode_and_unescape_value(event.decoder()).map_err(|error| {
            Error::insert_package(format!("invalid XML value in control part '{path}': {error}"))
        })?;
        if value.len() > MAX_XML_ATTRIBUTE_BYTES {
            return Err(Error::unsafe_package(format!(
                "control part '{path}' contains an oversized decoded XML attribute"
            )));
        }
        if value.starts_with(strict_namespace_prefix) {
            return Err(Error::unsupported_package_feature(format!(
                "Strict OOXML namespace in control part '{path}'"
            )));
        }
    }
    Ok(())
}

fn parse_content_types(xml: &[u8]) -> Result<ContentTypes> {
    let mut reader = Reader::from_reader(xml);
    let mut content_types = ContentTypes::default();
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid content types XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"Default" =>
            {
                content_types.defaults.push(ContentTypeDefault {
                    extension: xml_attribute(&event, b"Extension")?.ok_or_else(|| {
                        Error::insert_package("content type Default has no Extension")
                    })?,
                    content_type: xml_attribute(&event, b"ContentType")?.ok_or_else(|| {
                        Error::insert_package("content type Default has no ContentType")
                    })?,
                });
            }
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"Override" =>
            {
                let part_name = xml_attribute(&event, b"PartName")?.ok_or_else(|| {
                    Error::insert_package("content type Override has no PartName")
                })?;
                content_types.overrides.push(ContentTypeOverride {
                    part_name: normalize_absolute_part_name(&part_name)?,
                    content_type: xml_attribute(&event, b"ContentType")?.ok_or_else(|| {
                        Error::insert_package("content type Override has no ContentType")
                    })?,
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(content_types)
}

fn parse_relationships(source: Option<&str>, xml: &[u8]) -> Result<Vec<PackageRelationship>> {
    let mut reader = Reader::from_reader(xml);
    let mut relationships = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid relationships XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"Relationship" =>
            {
                let target_mode = xml_attribute(&event, b"TargetMode")?;
                let target = xml_attribute(&event, b"Target")?
                    .ok_or_else(|| Error::insert_package("Relationship has no Target"))?;
                let normalized_target = if target_mode
                    .as_deref()
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("External"))
                {
                    None
                } else {
                    Some(normalize_relationship_target(source, &target)?)
                };
                relationships.push(PackageRelationship {
                    source: source.map(str::to_owned),
                    id: xml_attribute(&event, b"Id")?
                        .ok_or_else(|| Error::insert_package("Relationship has no Id"))?,
                    relationship_type: xml_attribute(&event, b"Type")?
                        .ok_or_else(|| Error::insert_package("Relationship has no Type"))?,
                    target,
                    normalized_target,
                    target_mode,
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(relationships)
}

fn parse_workbook(xml: &[u8]) -> Result<(Vec<SheetElement>, Vec<WorkbookView>, Vec<DefinedName>)> {
    let mut reader = Reader::from_reader(xml);
    let mut sheets = Vec::new();
    let mut views = Vec::new();
    let mut defined_names = Vec::new();
    let mut current_defined_name = None::<DefinedName>;
    loop {
        match reader
            .read_event()
            .map_err(|error| Error::insert_package(format!("invalid workbook XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"workbookView" =>
            {
                views.push(WorkbookView {
                    active_tab: xml_attribute(&event, b"activeTab")?
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                    first_sheet: xml_attribute(&event, b"firstSheet")?
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                });
            }
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"sheet" =>
            {
                let name = xml_attribute(&event, b"name")?
                    .ok_or_else(|| Error::insert_package("workbook sheet has no name"))?;
                validate_sheet_name(&name, &std::collections::HashSet::new())?;
                let visibility = match xml_attribute(&event, b"state")?.as_deref() {
                    None | Some("visible") => SheetVisibility::Visible,
                    Some("hidden") => SheetVisibility::Hidden,
                    Some("veryHidden") => SheetVisibility::VeryHidden,
                    Some(state) => {
                        return Err(Error::insert_package(format!(
                            "worksheet '{name}' has invalid state '{state}'"
                        )));
                    }
                };
                sheets.push(SheetElement {
                    name,
                    sheet_id: xml_attribute(&event, b"sheetId")?
                        .ok_or_else(|| Error::insert_package("workbook sheet has no sheetId"))?
                        .parse()
                        .map_err(|_| Error::insert_package("workbook sheet has invalid sheetId"))?,
                    relationship_id: xml_attribute(&event, b"id")?.ok_or_else(|| {
                        Error::insert_package("workbook sheet has no relationship ID")
                    })?,
                    visibility,
                });
            }
            Event::Start(event) if local_name(event.name().as_ref()) == b"definedName" => {
                current_defined_name = Some(DefinedName {
                    name: xml_attribute(&event, b"name")?
                        .ok_or_else(|| Error::insert_package("definedName has no name"))?,
                    local_sheet_id: xml_attribute(&event, b"localSheetId")?
                        .and_then(|value| value.parse().ok()),
                    hidden: xml_attribute(&event, b"hidden")?.as_deref() == Some("1"),
                    formula: String::new(),
                });
            }
            Event::Text(text) if current_defined_name.is_some() => {
                current_defined_name.as_mut().expect("defined-name state").formula.push_str(
                    &text.xml10_content().map_err(|error| {
                        Error::insert_package(format!("invalid defined-name text: {error}"))
                    })?,
                );
            }
            Event::GeneralRef(reference) if current_defined_name.is_some() => {
                append_xml_reference(
                    &reference,
                    &mut current_defined_name.as_mut().expect("defined-name state").formula,
                )?;
            }
            Event::End(event) if local_name(event.name().as_ref()) == b"definedName" => {
                defined_names.push(current_defined_name.take().ok_or_else(|| {
                    Error::insert_package("definedName closing tag has no start tag")
                })?);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((sheets, views, defined_names))
}

fn reject_unsupported_content_types(content_types: &ContentTypes) -> Result<()> {
    for content_type in content_types
        .defaults
        .iter()
        .map(|entry| entry.content_type.as_str())
        .chain(content_types.overrides.iter().map(|entry| entry.content_type.as_str()))
    {
        let lowered = content_type.to_ascii_lowercase();
        if lowered.contains("macroenabled") || lowered.contains("vbaproject") {
            return Err(Error::unsupported_package_feature(format!(
                "macro-enabled content type '{content_type}'"
            )));
        }
        if lowered.contains("digital-signature") {
            return Err(Error::unsupported_package_feature(format!(
                "digital signature content type '{content_type}'"
            )));
        }
    }
    Ok(())
}

fn validate_content_type_uniqueness(content_types: &ContentTypes) -> Result<()> {
    let mut extensions = BTreeSet::new();
    for entry in &content_types.defaults {
        if !extensions.insert(entry.extension.to_lowercase()) {
            return Err(Error::unsafe_package(format!(
                "content-type default extension '{}' is duplicated",
                entry.extension
            )));
        }
    }
    let mut part_names = BTreeSet::new();
    for entry in &content_types.overrides {
        if !part_names.insert(entry.part_name.to_lowercase()) {
            return Err(Error::unsafe_package(format!(
                "content-type override '{}' is duplicated",
                entry.part_name
            )));
        }
    }
    Ok(())
}

fn reject_unsupported_relationships(relationships: &[PackageRelationship]) -> Result<()> {
    for relationship in relationships {
        if let Some("vbaProject" | "vbaProjectSignature" | "digital-signature" | "signature") =
            relationship.relationship_type.rsplit('/').next()
        {
            return Err(Error::unsupported_package_feature(format!(
                "relationship type '{}'",
                relationship.relationship_type
            )));
        }
    }
    Ok(())
}

fn validate_relationship_id_uniqueness(relationships: &[PackageRelationship]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for relationship in relationships {
        let key = (relationship.source.as_deref(), relationship.id.as_str());
        if !seen.insert(key) {
            return Err(Error::unsafe_package(format!(
                "relationship ID '{}' is duplicated for source '{}'",
                relationship.id,
                relationship.source.as_deref().unwrap_or("/")
            )));
        }
    }
    Ok(())
}

fn validate_relationship_target_uniqueness(relationships: &[PackageRelationship]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for relationship in relationships {
        let Some(target) = relationship.normalized_target.as_deref() else {
            continue;
        };
        let key = (relationship.source.as_deref(), target);
        if !seen.insert(key) {
            return Err(Error::unsafe_package(format!(
                "relationship target '{target}' is duplicated for source '{}'",
                relationship.source.as_deref().unwrap_or("/")
            )));
        }
    }
    Ok(())
}

fn validate_relationship_cycles(relationships: &[PackageRelationship]) -> Result<()> {
    let mut graph = BTreeMap::<&str, Vec<&str>>::new();
    for relationship in relationships {
        if let (Some(source), Some(target)) =
            (relationship.source.as_deref(), relationship.normalized_target.as_deref())
        {
            graph.entry(source).or_default().push(target);
        }
    }
    let mut states = BTreeMap::<&str, u8>::new();
    for node in graph.keys().copied() {
        visit_relationship_node(node, &graph, &mut states)?;
    }
    Ok(())
}

fn visit_relationship_node<'a>(
    node: &'a str,
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    states: &mut BTreeMap<&'a str, u8>,
) -> Result<()> {
    match states.get(node) {
        Some(1) => {
            return Err(Error::unsafe_package(format!(
                "internal relationship cycle includes '{node}'"
            )));
        }
        Some(2) => return Ok(()),
        _ => {}
    }
    states.insert(node, 1);
    if let Some(targets) = graph.get(node) {
        for target in targets {
            if graph.contains_key(target) {
                visit_relationship_node(target, graph, states)?;
            }
        }
    }
    states.insert(node, 2);
    Ok(())
}

fn relationship_source(path: &str) -> Result<Option<String>> {
    if path == "_rels/.rels" {
        return Ok(None);
    }
    let (prefix, name) = path
        .rsplit_once("_rels/")
        .ok_or_else(|| Error::unsafe_package(format!("invalid relationship part path '{path}'")))?;
    let name = name.strip_suffix(".rels").ok_or_else(|| {
        Error::unsafe_package(format!("invalid relationship part suffix '{path}'"))
    })?;
    validate_entry_name(&format!("{prefix}{name}")).map(Some)
}

fn normalize_relationship_target(source: Option<&str>, target: &str) -> Result<String> {
    if target.contains('\\') {
        return Err(Error::unsafe_package(format!(
            "relationship target '{target}' contains a backslash"
        )));
    }
    let base = source.and_then(|source| source.rsplit_once('/').map(|(base, _)| base));
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_owned()
    } else if let Some(base) = base {
        format!("{base}/{target}")
    } else {
        target.to_owned()
    };
    normalize_part_path(&combined)
}

fn normalize_absolute_part_name(part_name: &str) -> Result<String> {
    if !part_name.starts_with('/') {
        return Err(Error::unsafe_package(format!(
            "content type part name '{part_name}' is not absolute"
        )));
    }
    normalize_part_path(part_name.trim_start_matches('/'))
}

fn normalize_part_path(path: &str) -> Result<String> {
    let normalized_uri = canonicalize_part_uri(path)?;
    let mut parts = Vec::new();
    for segment in normalized_uri.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error::unsafe_package(format!(
                        "part path '{path}' escapes the package root"
                    )));
                }
            }
            segment if segment.as_bytes().get(1) == Some(&b':') => {
                return Err(Error::unsafe_package(format!(
                    "part path '{path}' contains a drive prefix"
                )));
            }
            segment => parts.push(segment),
        }
    }
    if parts.is_empty() {
        return Err(Error::unsafe_package(format!("part path '{path}' is empty")));
    }
    Ok(parts.join("/"))
}

fn canonical_part_identity(path: &str) -> Result<String> {
    Ok(canonicalize_part_uri(path)?.to_lowercase())
}

fn canonicalize_part_uri(path: &str) -> Result<String> {
    if path.contains(['?', '#']) {
        return Err(Error::unsafe_package(format!(
            "part path '{path}' contains a query or fragment"
        )));
    }
    let bytes = path.as_bytes();
    let mut output = String::with_capacity(path.len());
    let mut index = 0;
    let mut chunk_start = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        output.push_str(&path[chunk_start..index]);
        if index + 2 >= bytes.len() {
            return Err(Error::unsafe_package(format!(
                "part path '{path}' contains invalid percent encoding"
            )));
        }
        let high = hex_value(bytes[index + 1]);
        let low = hex_value(bytes[index + 2]);
        let (Some(high), Some(low)) = (high, low) else {
            return Err(Error::unsafe_package(format!(
                "part path '{path}' contains invalid percent encoding"
            )));
        };
        let decoded = (high << 4) | low;
        if decoded.is_ascii_alphanumeric()
            || matches!(decoded, b'-' | b'.' | b'_' | b'~' | b'/' | b'\\')
        {
            return Err(Error::unsafe_package(format!(
                "part path '{path}' contains non-canonical percent encoding"
            )));
        }
        output.push('%');
        output.push(char::from(b"0123456789ABCDEF"[high as usize]));
        output.push(char::from(b"0123456789ABCDEF"[low as usize]));
        index += 3;
        chunk_start = index;
    }
    output.push_str(&path[chunk_start..]);
    Ok(output)
}

const fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn xml_attribute(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    for attribute in event.attributes().with_checks(true) {
        let attribute = attribute
            .map_err(|error| Error::insert_package(format!("invalid XML attribute: {error}")))?;
        if attribute.value.len() > MAX_XML_ATTRIBUTE_BYTES {
            return Err(Error::unsafe_package("oversized XML attribute"));
        }
        if local_name(attribute.key.as_ref()) == name {
            let value = attribute
                .decode_and_unescape_value(event.decoder())
                .map_err(|error| Error::insert_package(format!("invalid XML value: {error}")))?;
            if value.len() > MAX_XML_ATTRIBUTE_BYTES {
                return Err(Error::unsafe_package("oversized decoded XML attribute"));
            }
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn append_xml_reference(
    reference: &quick_xml::events::BytesRef<'_>,
    target: &mut String,
) -> Result<()> {
    let decoded = reference
        .decode()
        .map_err(|error| Error::insert_package(format!("invalid XML reference: {error}")))?;
    match decoded.as_ref() {
        "lt" => target.push('<'),
        "gt" => target.push('>'),
        "amp" => target.push('&'),
        "quot" => target.push('"'),
        "apos" => target.push('\''),
        _ => {
            if let Some(value) = reference
                .resolve_char_ref()
                .map_err(|error| Error::insert_package(format!("invalid XML reference: {error}")))?
            {
                target.push(value);
            }
        }
    }
    Ok(())
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn normalize_sheet_name(name: &str) -> String {
    name.to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::*;

    #[test]
    fn package_inventory_preserves_order_and_allocates_independent_ids() {
        let bytes = workbook_package(&[
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("xl/worksheets/data.xml", b"<worksheet/>"),
            ("xl/worksheets/_rels/data.xml.rels", DATA_RELS.as_bytes()),
            ("xl/worksheets/sheet1.xml", b"<worksheet/>"),
            ("xl/worksheets/sheet3.xml", b"<worksheet/>"),
            ("xl/tables/table1.xml", b"<table/>"),
            ("xl/drawings/drawing1.xml", b"<drawing/>"),
            ("xl/comments1.xml", b"<comments/>"),
            ("xl/drawings/vmlDrawing1.vml", b"<xml/>"),
        ]);
        let inventory = PackageInventory::inspect(Cursor::new(bytes)).unwrap();
        assert_eq!(
            inventory.sheets.iter().map(|sheet| sheet.name.as_str()).collect::<Vec<_>>(),
            ["Data", "Hidden", "Archive"]
        );
        assert_eq!(
            inventory.sheets.iter().map(|sheet| sheet.sheet_id).collect::<Vec<_>>(),
            [7, 42, 3,]
        );
        assert_eq!(inventory.views, [WorkbookView { active_tab: 0, first_sheet: 0 }]);
        assert_eq!(inventory.defined_names[0].name, "GlobalTotal");
        assert_eq!(inventory.find_sheet("dAtA").unwrap().target, "xl/worksheets/data.xml");
        assert!(inventory.ensure_sheet_absent("DATA").is_err());
        assert!(inventory.relationships.iter().any(|relationship| {
            relationship.source.as_deref() == Some("xl/worksheets/data.xml")
                && relationship.id == "rId3"
                && relationship.target == "../comments1.xml"
                && relationship.normalized_target.as_deref() == Some("xl/comments1.xml")
        }));
        assert!(inventory.relationships.iter().any(|relationship| {
            relationship.source.is_none()
                && relationship.normalized_target.as_deref() == Some("xl/workbook.xml")
        }));

        let allocation = inventory.allocate_worksheet().unwrap();
        assert_eq!(allocation.sheet_id, 43);
        assert_eq!(allocation.relationship_id, "rId1");
        assert_eq!(allocation.package_path, "xl/worksheets/sheet2.xml");
        assert_eq!(allocation.workbook_target, "worksheets/sheet2.xml");
    }

    #[test]
    fn relationship_targets_are_normalized_without_sheet_id_assumptions() {
        assert_eq!(
            normalize_relationship_target(Some("xl/workbook.xml"), "worksheets/custom.xml")
                .unwrap(),
            "xl/worksheets/custom.xml"
        );
        assert_eq!(
            normalize_relationship_target(
                Some("xl/worksheets/data.xml"),
                "../drawings/drawing1.xml"
            )
            .unwrap(),
            "xl/drawings/drawing1.xml"
        );
        assert_eq!(
            normalize_relationship_target(None, "/xl/workbook.xml").unwrap(),
            "xl/workbook.xml"
        );
        assert!(normalize_relationship_target(Some("xl/workbook.xml"), "../../evil.xml").is_err());
        assert!(normalize_relationship_target(Some("xl/workbook.xml"), "..\\evil.xml").is_err());
    }

    #[test]
    fn preflight_rejects_unsafe_duplicate_macro_signed_and_malformed_packages() {
        assert!(PackageInventory::inspect(Cursor::new(b"not a zip".to_vec())).is_err());

        let duplicate_source = zip_entries(&[
            (CONTENT_TYPES_PATH, TYPES),
            (WORKBOOK_PATH, WORKBOOK),
            (WORKBOOK_RELS_PATH, WORKBOOK_RELS),
            ("xl/workbook.xm1", WORKBOOK),
            ("xl/worksheets/data.xml", "<worksheet/>"),
            ("xl/worksheets/sheet1.xml", "<worksheet/>"),
            ("xl/worksheets/sheet3.xml", "<worksheet/>"),
        ]);
        let duplicate = replace_bytes(duplicate_source, b"xl/workbook.xm1", b"xl/workbook.xml");
        assert!(PackageInventory::inspect(Cursor::new(duplicate)).is_err());

        let encrypted = mark_first_entry_encrypted(workbook_package(&[
            ("xl/worksheets/data.xml", b"<worksheet/>"),
            ("xl/worksheets/sheet1.xml", b"<worksheet/>"),
            ("xl/worksheets/sheet3.xml", b"<worksheet/>"),
        ]));
        assert!(PackageInventory::inspect(Cursor::new(encrypted)).is_err());

        for extra in [("../evil.xml", "bad"), ("_xmlsignatures/sig1.xml", "<Signature/>")] {
            let bytes = workbook_package(&[(extra.0, extra.1.as_bytes())]);
            assert!(PackageInventory::inspect(Cursor::new(bytes)).is_err());
        }

        let macro_types = TYPES.replace(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
            "application/vnd.ms-excel.sheet.macroEnabled.main+xml",
        );
        let macro_package = zip_entries(&[
            (CONTENT_TYPES_PATH, &macro_types),
            (WORKBOOK_PATH, WORKBOOK),
            (WORKBOOK_RELS_PATH, WORKBOOK_RELS),
            ("xl/worksheets/data.xml", "<worksheet/>"),
            ("xl/worksheets/sheet1.xml", "<worksheet/>"),
            ("xl/worksheets/sheet3.xml", "<worksheet/>"),
        ]);
        assert!(PackageInventory::inspect(Cursor::new(macro_package)).is_err());

        let duplicate_types = TYPES.replace(
            "</Types>",
            "<Default Extension=\"XML\" ContentType=\"application/xml\"/></Types>",
        );
        let duplicate_types_package = zip_entries(&[
            (CONTENT_TYPES_PATH, &duplicate_types),
            (WORKBOOK_PATH, WORKBOOK),
            (WORKBOOK_RELS_PATH, WORKBOOK_RELS),
            ("xl/worksheets/data.xml", "<worksheet/>"),
            ("xl/worksheets/sheet1.xml", "<worksheet/>"),
            ("xl/worksheets/sheet3.xml", "<worksheet/>"),
        ]);
        assert!(PackageInventory::inspect(Cursor::new(duplicate_types_package)).is_err());

        let duplicate_override_types = TYPES.replace(
            "</Types>",
            "<Override PartName=\"/XL/WORKBOOK.XML\" ContentType=\"application/xml\"/></Types>",
        );
        let duplicate_override_package = zip_entries(&[
            (CONTENT_TYPES_PATH, &duplicate_override_types),
            (WORKBOOK_PATH, WORKBOOK),
            (WORKBOOK_RELS_PATH, WORKBOOK_RELS),
            ("xl/worksheets/data.xml", "<worksheet/>"),
            ("xl/worksheets/sheet1.xml", "<worksheet/>"),
            ("xl/worksheets/sheet3.xml", "<worksheet/>"),
        ]);
        assert!(PackageInventory::inspect(Cursor::new(duplicate_override_package)).is_err());

        let malformed_rels = WORKBOOK_RELS.replace("Id=\"rId9\"", "Id=\"rId2\"");
        let malformed = zip_entries(&[
            (CONTENT_TYPES_PATH, TYPES),
            (WORKBOOK_PATH, WORKBOOK),
            (WORKBOOK_RELS_PATH, &malformed_rels),
            ("xl/worksheets/data.xml", "<worksheet/>"),
            ("xl/worksheets/sheet1.xml", "<worksheet/>"),
            ("xl/worksheets/sheet3.xml", "<worksheet/>"),
        ]);
        assert!(PackageInventory::inspect(Cursor::new(malformed)).is_err());
    }

    #[test]
    fn preflight_enforces_entry_and_control_xml_budgets() {
        let mut central_directory = Vec::with_capacity((MAX_PACKAGE_ENTRIES + 1) * 46);
        for _ in 0..=MAX_PACKAGE_ENTRIES {
            central_directory.extend_from_slice(&[0x50, 0x4B, 0x01, 0x02]);
            central_directory.extend_from_slice(&[0; 42]);
        }
        let error = inspect_central_directory(&mut Cursor::new(central_directory), 0).unwrap_err();
        assert!(error.to_string().contains("entry count"));

        let oversized = vec![b' '; MAX_CONTROL_PART_BYTES as usize + 1];
        let package = zip_binary_entries(&[
            (CONTENT_TYPES_PATH, oversized.as_slice()),
            (WORKBOOK_PATH, WORKBOOK.as_bytes()),
            (WORKBOOK_RELS_PATH, WORKBOOK_RELS.as_bytes()),
        ]);
        let error = PackageInventory::inspect(Cursor::new(package)).unwrap_err();
        assert!(error.to_string().contains("control part"));

        let mut budget = ControlXmlBudget::default();
        budget.consume(MAX_TOTAL_CONTROL_BYTES).unwrap();
        let error = budget.consume(1).unwrap_err();
        assert!(error.to_string().contains("aggregate size"));
    }

    fn workbook_package(extra_entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut entries = vec![
            (CONTENT_TYPES_PATH, TYPES.as_bytes()),
            (WORKBOOK_PATH, WORKBOOK.as_bytes()),
            (WORKBOOK_RELS_PATH, WORKBOOK_RELS.as_bytes()),
        ];
        entries.extend_from_slice(extra_entries);
        zip_binary_entries(&entries)
    }

    fn zip_entries(entries: &[(&str, &str)]) -> Vec<u8> {
        let binary =
            entries.iter().map(|(name, value)| (*name, value.as_bytes())).collect::<Vec<_>>();
        zip_binary_entries(&binary)
    }

    fn zip_binary_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, payload) in entries {
            writer.start_file(name, options).unwrap();
            writer.write_all(payload).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn replace_bytes(mut bytes: Vec<u8>, from: &[u8], to: &[u8]) -> Vec<u8> {
        assert_eq!(from.len(), to.len());
        let mut replacements = 0;
        for index in 0..=bytes.len() - from.len() {
            if &bytes[index..index + from.len()] == from {
                bytes[index..index + to.len()].copy_from_slice(to);
                replacements += 1;
            }
        }
        assert_eq!(replacements, 2, "local and central ZIP names must be replaced");
        bytes
    }

    fn mark_first_entry_encrypted(mut bytes: Vec<u8>) -> Vec<u8> {
        for (signature, flag_offset) in
            [([0x50, 0x4B, 0x03, 0x04], 6), ([0x50, 0x4B, 0x01, 0x02], 8)]
        {
            let offset = bytes
                .windows(signature.len())
                .position(|window| window == signature)
                .expect("ZIP header signature")
                + flag_offset;
            let flags = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) | 1;
            bytes[offset..offset + 2].copy_from_slice(&flags.to_le_bytes());
        }
        bytes
    }

    const TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/data.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet3.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    const ROOT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/xl/workbook.xml"/></Relationships>"#;
    const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><bookViews><workbookView activeTab="0" firstSheet="0"/></bookViews><sheets><sheet name="Data" sheetId="7" r:id="rId9"/><sheet name="Hidden" sheetId="42" state="hidden" r:id="rId2"/><sheet name="Archive" sheetId="3" state="veryHidden" r:id="rId14"/></sheets><definedNames><definedName name="GlobalTotal">'Data'!$A$1</definedName></definedNames></workbook>"#;
    const WORKBOOK_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/data.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId14" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/></Relationships>"#;
    const DATA_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#;
}
