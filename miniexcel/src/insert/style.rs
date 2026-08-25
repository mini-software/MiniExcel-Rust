use std::collections::{BTreeMap, HashMap};

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};

use super::donor::DonorWorksheet;
use crate::{Error, Result};

const FIRST_CUSTOM_NUM_FMT_ID: u32 = 164;
const MAX_NUM_FMT_ID: u32 = u16::MAX as u32;
const MAX_NUMBER_FORMATS: usize = 250;
const MAX_FONTS: usize = 512;
const MAX_FILLS: usize = 256;
const MAX_BORDERS: usize = 256;
const MAX_CELL_STYLES: usize = 65_490;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StyleRebaseResult {
    pub(crate) styles_xml: Vec<u8>,
    pub(crate) worksheet_xml: Vec<u8>,
    pub(crate) cell_xf_map: Vec<u32>,
}

pub(crate) fn rebase_styles(
    target_styles_xml: &[u8],
    donor: &DonorWorksheet,
) -> Result<StyleRebaseResult> {
    rebase_style_xml(target_styles_xml, &donor.styles.xml, &donor.worksheet_xml)
}

fn rebase_style_xml(
    target_styles_xml: &[u8],
    donor_styles_xml: &[u8],
    donor_worksheet_xml: &[u8],
) -> Result<StyleRebaseResult> {
    let target = StyleDocument::parse(target_styles_xml)?;
    let donor = StyleDocument::parse(donor_styles_xml)?;

    let target_num_fmts = target.nodes(StyleSection::NumFmts)?;
    let donor_num_fmts = donor.nodes(StyleSection::NumFmts)?;
    let target_style_xfs = target.nodes(StyleSection::CellStyleXfs)?;
    let target_cell_xfs = target.nodes(StyleSection::CellXfs)?;
    let donor_style_xfs = donor.nodes(StyleSection::CellStyleXfs)?;
    let donor_cell_xfs = donor.nodes(StyleSection::CellXfs)?;

    let (num_fmt_map, appended_num_fmts) = merge_number_formats(
        &target_num_fmts,
        &donor_num_fmts,
        target_style_xfs.iter().chain(&target_cell_xfs),
    )?;
    let (font_map, appended_fonts) = merge_plain_components(
        &target.nodes(StyleSection::Fonts)?,
        &donor.nodes(StyleSection::Fonts)?,
    )?;
    let (fill_map, appended_fills) = merge_plain_components(
        &target.nodes(StyleSection::Fills)?,
        &donor.nodes(StyleSection::Fills)?,
    )?;
    let (border_map, appended_borders) = merge_plain_components(
        &target.nodes(StyleSection::Borders)?,
        &donor.nodes(StyleSection::Borders)?,
    )?;

    let (style_xf_map, appended_style_xfs) = merge_xfs(
        &target_style_xfs,
        &donor_style_xfs,
        &num_fmt_map,
        &font_map,
        &fill_map,
        &border_map,
        None,
    )?;
    let (cell_xf_map, appended_cell_xfs) = merge_xfs(
        &target_cell_xfs,
        &donor_cell_xfs,
        &num_fmt_map,
        &font_map,
        &fill_map,
        &border_map,
        Some(&style_xf_map),
    )?;

    validate_limits(
        target_num_fmts.len() + appended_num_fmts.len(),
        target.nodes(StyleSection::Fonts)?.len() + appended_fonts.len(),
        target.nodes(StyleSection::Fills)?.len() + appended_fills.len(),
        target.nodes(StyleSection::Borders)?.len() + appended_borders.len(),
        target_style_xfs.len() + appended_style_xfs.len(),
        target_cell_xfs.len() + appended_cell_xfs.len(),
    )?;

    let appended = BTreeMap::from([
        (StyleSection::NumFmts, appended_num_fmts),
        (StyleSection::Fonts, appended_fonts),
        (StyleSection::Fills, appended_fills),
        (StyleSection::Borders, appended_borders),
        (StyleSection::CellStyleXfs, appended_style_xfs),
        (StyleSection::CellXfs, appended_cell_xfs),
    ]);
    let styles_xml = target.render(&appended)?;
    let worksheet_xml = rewrite_worksheet_styles(donor_worksheet_xml, &cell_xf_map)?;
    Ok(StyleRebaseResult { styles_xml, worksheet_xml, cell_xf_map })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum StyleSection {
    NumFmts,
    Fonts,
    Fills,
    Borders,
    CellStyleXfs,
    CellXfs,
}

impl StyleSection {
    fn from_name(name: &[u8]) -> Option<Self> {
        match local_name(name) {
            b"numFmts" => Some(Self::NumFmts),
            b"fonts" => Some(Self::Fonts),
            b"fills" => Some(Self::Fills),
            b"borders" => Some(Self::Borders),
            b"cellStyleXfs" => Some(Self::CellStyleXfs),
            b"cellXfs" => Some(Self::CellXfs),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::NumFmts => "numFmts",
            Self::Fonts => "fonts",
            Self::Fills => "fills",
            Self::Borders => "borders",
            Self::CellStyleXfs => "cellStyleXfs",
            Self::CellXfs => "cellXfs",
        }
    }

    const fn child_name(self) -> &'static [u8] {
        match self {
            Self::NumFmts => b"numFmt",
            Self::Fonts => b"font",
            Self::Fills => b"fill",
            Self::Borders => b"border",
            Self::CellStyleXfs | Self::CellXfs => b"xf",
        }
    }

    const fn required(self) -> bool {
        !matches!(self, Self::NumFmts)
    }
}

#[derive(Clone, Debug)]
struct XmlNode {
    events: Vec<Event<'static>>,
}

impl XmlNode {
    fn key(&self) -> Result<Vec<u8>> {
        serialize_events(&self.events)
    }

    fn root(&self) -> Result<&BytesStart<'static>> {
        match self.events.first() {
            Some(Event::Start(start) | Event::Empty(start)) => Ok(start),
            _ => Err(Error::insert_package("style component has no element root")),
        }
    }

    fn with_attributes(&self, updates: &[(&[u8], String)]) -> Result<Self> {
        let mut events = self.events.clone();
        let root = self.root()?;
        let replacement = replace_attributes(root, updates)?;
        events[0] = match events.first() {
            Some(Event::Start(_)) => Event::Start(replacement),
            Some(Event::Empty(_)) => Event::Empty(replacement),
            _ => return Err(Error::insert_package("style component has no element root")),
        };
        Ok(Self { events })
    }
}

#[derive(Debug)]
struct StyleDocument {
    events: Vec<Event<'static>>,
    ranges: BTreeMap<StyleSection, (usize, usize)>,
}

impl StyleDocument {
    fn parse(xml: &[u8]) -> Result<Self> {
        let mut reader = Reader::from_reader(xml);
        reader.config_mut().trim_text(false);
        let mut events = Vec::new();
        loop {
            let event = reader
                .read_event()
                .map_err(|error| Error::insert_package(format!("invalid styles XML: {error}")))?;
            if matches!(event, Event::Eof) {
                break;
            }
            events.push(event.into_owned());
        }

        let mut stack = Vec::<Option<StyleSection>>::new();
        let mut ranges = BTreeMap::new();
        let mut style_sheet_seen = false;
        for (index, event) in events.iter().enumerate() {
            match event {
                Event::Start(start) => {
                    let parent_is_style_sheet = stack.len() == 1 && style_sheet_seen;
                    let section = parent_is_style_sheet
                        .then(|| StyleSection::from_name(start.name().as_ref()))
                        .flatten();
                    if section.is_some_and(|section| ranges.contains_key(&section)) {
                        return Err(Error::insert_package(
                            "styles XML contains duplicate sections",
                        ));
                    }
                    if stack.is_empty() {
                        if local_name(start.name().as_ref()) != b"styleSheet" {
                            return Err(Error::insert_package("styles XML root is not styleSheet"));
                        }
                        if start.name().as_ref().contains(&b':') {
                            return Err(Error::unsupported_package_feature(
                                "prefixed styles XML namespace layout",
                            ));
                        }
                        style_sheet_seen = true;
                    }
                    if let Some(section) = section {
                        ranges.insert(section, (index, usize::MAX));
                    }
                    stack.push(section);
                }
                Event::Empty(empty) if stack.len() == 1 && style_sheet_seen => {
                    if let Some(section) = StyleSection::from_name(empty.name().as_ref()) {
                        if ranges.insert(section, (index, index)).is_some() {
                            return Err(Error::insert_package(
                                "styles XML contains duplicate sections",
                            ));
                        }
                    }
                }
                Event::End(_) => {
                    let section = stack
                        .pop()
                        .ok_or_else(|| Error::insert_package("unbalanced styles XML"))?;
                    if let Some(section) = section {
                        ranges.get_mut(&section).expect("style section range").1 = index;
                    }
                }
                _ => {}
            }
        }
        if !style_sheet_seen || !stack.is_empty() {
            return Err(Error::insert_package("incomplete styles XML"));
        }
        for section in [
            StyleSection::Fonts,
            StyleSection::Fills,
            StyleSection::Borders,
            StyleSection::CellStyleXfs,
            StyleSection::CellXfs,
        ] {
            if section.required() && !ranges.contains_key(&section) {
                return Err(Error::insert_package(format!(
                    "styles XML is missing {}",
                    section.name()
                )));
            }
        }
        Ok(Self { events, ranges })
    }

    fn nodes(&self, section: StyleSection) -> Result<Vec<XmlNode>> {
        let Some(&(start, end)) = self.ranges.get(&section) else {
            return Ok(Vec::new());
        };
        if start == end {
            return Ok(Vec::new());
        }
        let mut nodes = Vec::new();
        let mut index = start + 1;
        while index < end {
            match &self.events[index] {
                Event::Start(root) => {
                    let node_end = element_end(&self.events, index)?;
                    if local_name(root.name().as_ref()) == section.child_name() {
                        nodes.push(XmlNode { events: self.events[index..=node_end].to_vec() });
                    }
                    index = node_end + 1;
                }
                Event::Empty(root) => {
                    if local_name(root.name().as_ref()) == section.child_name() {
                        nodes.push(XmlNode { events: vec![self.events[index].clone()] });
                    }
                    index += 1;
                }
                _ => index += 1,
            }
        }
        Ok(nodes)
    }

    fn render(&self, appended: &BTreeMap<StyleSection, Vec<XmlNode>>) -> Result<Vec<u8>> {
        let mut writer = Writer::new(Vec::new());
        let missing_num_fmts = !self.ranges.contains_key(&StyleSection::NumFmts)
            && appended.get(&StyleSection::NumFmts).is_some_and(|nodes| !nodes.is_empty());
        let fonts_start = self.ranges[&StyleSection::Fonts].0;
        let mut index = 0;
        while index < self.events.len() {
            if missing_num_fmts && index == fonts_start {
                write_new_section(
                    &mut writer,
                    StyleSection::NumFmts,
                    &appended[&StyleSection::NumFmts],
                )?;
            }
            let section = self
                .ranges
                .iter()
                .find_map(|(section, range)| (range.0 == index).then_some((*section, *range)));
            if let Some((section, (start, end))) = section {
                let additions = &appended[&section];
                if additions.is_empty() {
                    write_event(&mut writer, self.events[index].clone())?;
                    index += 1;
                    continue;
                }
                let count = self.nodes(section)?.len() + additions.len();
                let root = match &self.events[start] {
                    Event::Start(root) | Event::Empty(root) => root,
                    _ => return Err(Error::insert_package("invalid style section root")),
                };
                write_event(
                    &mut writer,
                    Event::Start(replace_attributes(root, &[(b"count", count.to_string())])?),
                )?;
                if start != end {
                    for event in &self.events[start + 1..end] {
                        write_event(&mut writer, event.clone())?;
                    }
                }
                for node in additions {
                    for event in &node.events {
                        write_event(&mut writer, event.clone())?;
                    }
                }
                let end_name = match &self.events[end] {
                    Event::End(end) => end.to_owned(),
                    Event::Empty(_) => BytesEnd::new(section.name()),
                    _ => return Err(Error::insert_package("invalid style section end")),
                };
                write_event(&mut writer, Event::End(end_name))?;
                index = end + 1;
            } else {
                write_event(&mut writer, self.events[index].clone())?;
                index += 1;
            }
        }
        Ok(writer.into_inner())
    }
}

fn merge_number_formats<'a, I>(
    target: &[XmlNode],
    donor: &[XmlNode],
    target_xfs: I,
) -> Result<(HashMap<u32, u32>, Vec<XmlNode>)>
where
    I: IntoIterator<Item = &'a XmlNode>,
{
    let mut formats_by_code = HashMap::<String, u32>::new();
    let mut existing_ids = HashMap::<u32, String>::new();
    let mut max_id = FIRST_CUSTOM_NUM_FMT_ID - 1;
    for node in target {
        let (id, code) = number_format(node)?;
        if id < FIRST_CUSTOM_NUM_FMT_ID {
            return Err(Error::insert_package(format!(
                "custom number format uses reserved ID {id}"
            )));
        }
        if existing_ids.insert(id, code.clone()).is_some() {
            return Err(Error::insert_package(format!("duplicate number format ID {id}")));
        }
        formats_by_code.entry(code).or_insert(id);
        max_id = max_id.max(id);
    }
    for xf in target_xfs {
        max_id = max_id.max(numeric_attribute(xf.root()?, b"numFmtId")?.unwrap_or(0));
    }

    let mut mapping = HashMap::new();
    let mut appended = Vec::new();
    let mut donor_ids = HashMap::new();
    for node in donor {
        let (id, code) = number_format(node)?;
        if id < FIRST_CUSTOM_NUM_FMT_ID {
            return Err(Error::insert_package(format!(
                "donor custom number format uses reserved ID {id}"
            )));
        }
        if donor_ids.insert(id, code.clone()).is_some() {
            return Err(Error::insert_package(format!("duplicate donor number format ID {id}")));
        }
        let target_id = if let Some(existing) = formats_by_code.get(&code) {
            *existing
        } else {
            max_id = max_id
                .checked_add(1)
                .filter(|id| *id <= MAX_NUM_FMT_ID)
                .ok_or_else(|| Error::insert_package("custom number format ID limit exceeded"))?;
            formats_by_code.insert(code, max_id);
            appended.push(node.with_attributes(&[(b"numFmtId", max_id.to_string())])?);
            max_id
        };
        mapping.insert(id, target_id);
    }
    Ok((mapping, appended))
}

fn number_format(node: &XmlNode) -> Result<(u32, String)> {
    let root = node.root()?;
    let id = numeric_attribute(root, b"numFmtId")?
        .ok_or_else(|| Error::insert_package("number format has no numFmtId"))?;
    let code = attribute(root, b"formatCode")?
        .ok_or_else(|| Error::insert_package("number format has no formatCode"))?;
    Ok((id, code))
}

fn merge_plain_components(
    target: &[XmlNode],
    donor: &[XmlNode],
) -> Result<(Vec<u32>, Vec<XmlNode>)> {
    let mut indexes = HashMap::<Vec<u8>, u32>::new();
    for (index, node) in target.iter().enumerate() {
        indexes.entry(node.key()?).or_insert(index as u32);
    }
    let mut appended = Vec::new();
    let mut mapping = Vec::with_capacity(donor.len());
    for node in donor {
        let key = node.key()?;
        let target_index = if let Some(index) = indexes.get(&key) {
            *index
        } else {
            let index = (target.len() + appended.len()) as u32;
            indexes.insert(key, index);
            appended.push(node.clone());
            index
        };
        mapping.push(target_index);
    }
    Ok((mapping, appended))
}

#[allow(clippy::too_many_arguments)]
fn merge_xfs(
    target: &[XmlNode],
    donor: &[XmlNode],
    num_fmt_map: &HashMap<u32, u32>,
    font_map: &[u32],
    fill_map: &[u32],
    border_map: &[u32],
    xf_map: Option<&[u32]>,
) -> Result<(Vec<u32>, Vec<XmlNode>)> {
    let mut indexes = HashMap::<Vec<u8>, u32>::new();
    for (index, node) in target.iter().enumerate() {
        indexes.entry(node.key()?).or_insert(index as u32);
    }
    let mut appended = Vec::new();
    let mut mapping = Vec::with_capacity(donor.len());
    for node in donor {
        let rewritten = rewrite_xf(node, num_fmt_map, font_map, fill_map, border_map, xf_map)?;
        let key = rewritten.key()?;
        let target_index = if let Some(index) = indexes.get(&key) {
            *index
        } else {
            let index = (target.len() + appended.len()) as u32;
            indexes.insert(key, index);
            appended.push(rewritten);
            index
        };
        mapping.push(target_index);
    }
    Ok((mapping, appended))
}

fn rewrite_xf(
    node: &XmlNode,
    num_fmt_map: &HashMap<u32, u32>,
    font_map: &[u32],
    fill_map: &[u32],
    border_map: &[u32],
    xf_map: Option<&[u32]>,
) -> Result<XmlNode> {
    let root = node.root()?;
    let donor_num_fmt = numeric_attribute(root, b"numFmtId")?.unwrap_or(0);
    let target_num_fmt = if donor_num_fmt < FIRST_CUSTOM_NUM_FMT_ID {
        donor_num_fmt
    } else {
        *num_fmt_map.get(&donor_num_fmt).ok_or_else(|| {
            Error::insert_package(format!(
                "style references undefined custom number format {donor_num_fmt}"
            ))
        })?
    };
    let references = [
        (b"numFmtId".as_slice(), target_num_fmt),
        (b"fontId".as_slice(), mapped_index(root, b"fontId", font_map, "font")?),
        (b"fillId".as_slice(), mapped_index(root, b"fillId", fill_map, "fill")?),
        (b"borderId".as_slice(), mapped_index(root, b"borderId", border_map, "border")?),
    ];
    let mut updates = Vec::new();
    for (name, target_id) in references {
        if attribute(root, name)?.is_some() || target_id != 0 {
            updates.push((name, target_id.to_string()));
        }
    }
    if let Some(xf_map) = xf_map {
        let target_xf = mapped_index(root, b"xfId", xf_map, "cell-style XF")?;
        if attribute(root, b"xfId")?.is_some() || target_xf != 0 {
            updates.push((b"xfId".as_slice(), target_xf.to_string()));
        }
    }
    node.with_attributes(&updates)
}

fn mapped_index(
    root: &BytesStart<'_>,
    attribute_name: &[u8],
    mapping: &[u32],
    component: &str,
) -> Result<u32> {
    let donor_id = numeric_attribute(root, attribute_name)?.unwrap_or(0);
    mapping.get(donor_id as usize).copied().ok_or_else(|| {
        Error::insert_package(format!(
            "style references missing donor {component} index {donor_id}"
        ))
    })
}

fn validate_limits(
    number_formats: usize,
    fonts: usize,
    fills: usize,
    borders: usize,
    style_xfs: usize,
    cell_xfs: usize,
) -> Result<()> {
    for (name, count, limit) in [
        ("numFmts", number_formats, MAX_NUMBER_FORMATS),
        ("fonts", fonts, MAX_FONTS),
        ("fills", fills, MAX_FILLS),
        ("borders", borders, MAX_BORDERS),
        ("cellStyleXfs", style_xfs, MAX_CELL_STYLES),
        ("cellXfs", cell_xfs, MAX_CELL_STYLES),
    ] {
        if count > limit {
            return Err(Error::insert_package(format!(
                "style table {name} count {count} exceeds Excel limit {limit}"
            )));
        }
    }
    Ok(())
}

fn rewrite_worksheet_styles(xml: &[u8], mapping: &[u32]) -> Result<Vec<u8>> {
    let default_style = *mapping
        .first()
        .ok_or_else(|| Error::insert_package("donor styles contain no cell XFs"))?;
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    loop {
        let event = reader.read_event().map_err(|error| {
            Error::insert_package(format!("invalid donor worksheet XML: {error}"))
        })?;
        match event {
            Event::Start(cell) if local_name(cell.name().as_ref()) == b"c" => {
                write_event(
                    &mut writer,
                    Event::Start(rewrite_cell_style(&cell, mapping, default_style)?),
                )?;
            }
            Event::Empty(cell) if local_name(cell.name().as_ref()) == b"c" => {
                write_event(
                    &mut writer,
                    Event::Empty(rewrite_cell_style(&cell, mapping, default_style)?),
                )?;
            }
            Event::Eof => break,
            event => write_event(&mut writer, event)?,
        }
    }
    Ok(writer.into_inner())
}

fn rewrite_cell_style(
    cell: &BytesStart<'_>,
    mapping: &[u32],
    default_style: u32,
) -> Result<BytesStart<'static>> {
    let donor_style = numeric_attribute(cell, b"s")?.unwrap_or(0);
    let target_style = mapping.get(donor_style as usize).copied().ok_or_else(|| {
        Error::insert_package(format!("worksheet references missing donor style {donor_style}"))
    })?;
    if attribute(cell, b"s")?.is_none() && default_style == 0 {
        return clone_start(cell);
    }
    replace_attributes(cell, &[(b"s", target_style.to_string())])
}

fn write_new_section(
    writer: &mut Writer<Vec<u8>>,
    section: StyleSection,
    nodes: &[XmlNode],
) -> Result<()> {
    let mut start = BytesStart::new(section.name());
    let count = nodes.len().to_string();
    start.push_attribute(("count", count.as_str()));
    write_event(writer, Event::Start(start))?;
    for node in nodes {
        for event in &node.events {
            write_event(writer, event.clone())?;
        }
    }
    write_event(writer, Event::End(BytesEnd::new(section.name())))
}

fn element_end(events: &[Event<'_>], start: usize) -> Result<usize> {
    let mut depth = 0;
    for (offset, event) in events[start..].iter().enumerate() {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(start + offset);
                }
            }
            _ => {}
        }
    }
    Err(Error::insert_package("unterminated style component"))
}

fn numeric_attribute(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<u32>> {
    attribute(event, name)?
        .map(|value| {
            value.parse().map_err(|_| {
                Error::insert_package(format!(
                    "style attribute '{}' is not numeric",
                    String::from_utf8_lossy(name)
                ))
            })
        })
        .transpose()
}

fn attribute(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| Error::insert_package(format!("invalid style attribute: {error}")))?;
        if local_name(attribute.key.as_ref()) == name {
            return attribute
                .decode_and_unescape_value(event.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| Error::insert_package(format!("invalid style value: {error}")));
        }
    }
    Ok(None)
}

fn replace_attributes(
    event: &BytesStart<'_>,
    updates: &[(&[u8], String)],
) -> Result<BytesStart<'static>> {
    let qualified_name = event.name();
    let name = std::str::from_utf8(qualified_name.as_ref())
        .map_err(|_| Error::insert_package("style element name is not UTF-8"))?;
    let mut output = BytesStart::new(name.to_owned());
    let mut written = vec![false; updates.len()];
    for source in event.attributes().with_checks(false) {
        let source = source
            .map_err(|error| Error::insert_package(format!("invalid style attribute: {error}")))?;
        if let Some((index, (_, value))) =
            updates.iter().enumerate().find(|(_, (key, _))| local_name(source.key.as_ref()) == *key)
        {
            let key = std::str::from_utf8(source.key.as_ref())
                .map_err(|_| Error::insert_package("style attribute name is not UTF-8"))?;
            output.push_attribute((key, value.as_str()));
            written[index] = true;
        } else {
            let key = std::str::from_utf8(source.key.as_ref())
                .map_err(|_| Error::insert_package("style attribute name is not UTF-8"))?;
            let value = source.decode_and_unescape_value(event.decoder()).map_err(|error| {
                Error::insert_package(format!("invalid style attribute value: {error}"))
            })?;
            output.push_attribute((key, value.as_ref()));
        }
    }
    for ((key, value), written) in updates.iter().zip(written) {
        if !written {
            let key = std::str::from_utf8(key)
                .map_err(|_| Error::insert_package("style attribute name is not UTF-8"))?;
            output.push_attribute((key, value.as_str()));
        }
    }
    Ok(output)
}

fn clone_start(event: &BytesStart<'_>) -> Result<BytesStart<'static>> {
    replace_attributes(event, &[])
}

fn serialize_events(events: &[Event<'_>]) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    for event in events {
        write_event(&mut writer, event.clone())?;
    }
    Ok(writer.into_inner())
}

fn write_event(writer: &mut Writer<Vec<u8>>, event: Event<'_>) -> Result<()> {
    writer
        .write_event(event)
        .map_err(|error| Error::insert_package(format!("cannot write style XML: {error}")))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use chrono::{Duration, NaiveDate, NaiveTime};
    use serde::Serialize;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    use super::*;
    use crate::insert::donor::{DonorBuilder, extract_donor};
    use crate::writer::XlsxWriter;
    use crate::{CellValue, DynamicRow, MiniExcel, ReadOptions, SheetVisibility, WriteOptions};

    #[derive(Serialize)]
    struct CustomFormatRow {
        #[serde(rename = "Custom")]
        custom: f64,
    }

    #[test]
    fn style_rebase_appends_dependencies_and_preserves_target_nodes_and_extensions() {
        let (donor, _) = formatted_donor();
        let target = target_styles();
        let target_document = StyleDocument::parse(target.as_bytes()).unwrap();

        let rebased = rebase_styles(target.as_bytes(), &donor).unwrap();
        let merged = StyleDocument::parse(&rebased.styles_xml).unwrap();
        assert_eq!(rebased.cell_xf_map.len(), donor.styles.cell_xfs);
        for section in [
            StyleSection::NumFmts,
            StyleSection::Fonts,
            StyleSection::Fills,
            StyleSection::Borders,
            StyleSection::CellStyleXfs,
            StyleSection::CellXfs,
        ] {
            let target_nodes = target_document.nodes(section).unwrap();
            let merged_nodes = merged.nodes(section).unwrap();
            assert_eq!(
                target_nodes.iter().map(|node| node.key().unwrap()).collect::<Vec<_>>(),
                merged_nodes[..target_nodes.len()]
                    .iter()
                    .map(|node| node.key().unwrap())
                    .collect::<Vec<_>>(),
                "target {} prefix changed",
                section.name()
            );
        }
        let xml = String::from_utf8(rebased.styles_xml.clone()).unwrap();
        assert!(xml.contains("<ext uri=\"preserve-me\"><future value=\"unchanged\"/></ext>"));
        assert!(number_format_ids(&rebased.styles_xml).iter().any(|id| *id > 200));
        let merged_cell_xfs = merged.nodes(StyleSection::CellXfs).unwrap();
        assert!(
            worksheet_style_ids(&rebased.worksheet_xml)
                .iter()
                .all(|id| (*id as usize) < merged_cell_xfs.len())
        );
    }

    #[test]
    fn style_rebase_is_stable_and_deduplicates_exact_components() {
        let (donor, _) = formatted_donor();
        let first = rebase_styles(target_styles().as_bytes(), &donor).unwrap();
        let donor_again = DonorWorksheet {
            sheet_name: donor.sheet_name.clone(),
            visibility: donor.visibility,
            worksheet_xml: donor.worksheet_xml.clone(),
            data_row_count: donor.data_row_count,
            styles: super::super::donor::DonorStyleModel {
                xml: donor.styles.xml.clone(),
                ..donor.styles.clone()
            },
            local_defined_names: donor.local_defined_names.clone(),
        };
        let second = rebase_styles(&first.styles_xml, &donor_again).unwrap();
        assert_eq!(first.styles_xml, second.styles_xml);
        assert_eq!(first.cell_xf_map, second.cell_xf_map);
    }

    #[test]
    fn rebased_styles_roundtrip_date_time_duration_and_custom_formats() {
        let (donor, donor_package) = formatted_donor();
        let rebased = rebase_styles(target_styles().as_bytes(), &donor).unwrap();
        let package =
            replace_donor_parts(&donor_package, &rebased.styles_xml, &rebased.worksheet_xml);
        let mut rows = Vec::new();
        MiniExcel::visit_structured_rows_from_reader(
            &mut Cursor::new(package),
            &ReadOptions::new(),
            |row| {
                rows.push(row.clone());
                Ok(true)
            },
        )
        .unwrap();
        let data = &rows[1];
        assert!(matches!(data.cells()[0].value(), CellValue::DateTime(_)));
        assert!(matches!(data.cells()[1].value(), CellValue::DateTime(_)));
        assert!(matches!(data.cells()[2].value(), CellValue::Duration(_)));

        let (custom_donor, custom_package) = custom_format_donor();
        let custom_rebased = rebase_styles(&rebased.styles_xml, &custom_donor).unwrap();
        let custom_package = replace_donor_parts(
            &custom_package,
            &custom_rebased.styles_xml,
            &custom_rebased.worksheet_xml,
        );
        let mut custom_rows = Vec::new();
        MiniExcel::visit_structured_rows_from_reader(
            &mut Cursor::new(custom_package),
            &ReadOptions::new(),
            |row| {
                custom_rows.push(row.clone());
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(custom_rows[1].cells()[0].number_format(), Some("0.0000"));
    }

    #[test]
    fn style_rebase_rejects_missing_references_and_excel_limits() {
        let donor_styles = target_styles().replace("fontId=\"0\"", "fontId=\"99\"");
        assert!(
            rebase_style_xml(target_styles().as_bytes(), donor_styles.as_bytes(), b"<worksheet/>")
                .is_err()
        );
        assert!(validate_limits(MAX_NUMBER_FORMATS + 1, 1, 2, 1, 1, 1).is_err());
        assert!(validate_limits(1, MAX_FONTS + 1, 2, 1, 1, 1).is_err());
        assert!(validate_limits(1, 1, MAX_FILLS + 1, 1, 1, 1).is_err());
        assert!(validate_limits(1, 1, 2, MAX_BORDERS + 1, 1, 1).is_err());
        assert!(validate_limits(1, 1, 2, 1, MAX_CELL_STYLES + 1, 1).is_err());
        assert!(validate_limits(1, 1, 2, 1, 1, MAX_CELL_STYLES + 1).is_err());
        let prefixed = target_styles()
            .replace("<styleSheet ", "<x:styleSheet ")
            .replace("</styleSheet>", "</x:styleSheet>");
        assert!(
            rebase_style_xml(prefixed.as_bytes(), target_styles().as_bytes(), b"<worksheet/>")
                .is_err()
        );
    }

    #[test]
    #[ignore = "requires LibreOffice; set MINIEXCEL_TEST_SOFFICE when it is not installed in the standard Windows path"]
    fn rebased_styles_survive_libreoffice_roundtrip() {
        let soffice = std::env::var_os("MINIEXCEL_TEST_SOFFICE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.exe"));
        assert!(soffice.is_file(), "LibreOffice was not found at {}", soffice.display());

        let (donor, donor_package) = formatted_donor();
        let rebased = rebase_styles(target_styles().as_bytes(), &donor).unwrap();
        let dynamic_package =
            replace_donor_parts(&donor_package, &rebased.styles_xml, &rebased.worksheet_xml);
        let dynamic_package = libreoffice_roundtrip(&soffice, &dynamic_package, "dynamic");
        let dynamic_rows = structured_rows(&dynamic_package);
        assert_eq!(normalized_format(&dynamic_rows[1].cells()[0]), Some("yyyy-mm-dd".to_owned()));
        assert_eq!(normalized_format(&dynamic_rows[1].cells()[1]), Some("hh:mm:ss".to_owned()));
        assert_eq!(normalized_format(&dynamic_rows[1].cells()[2]), Some("[h]:mm:ss".to_owned()));

        let (custom_donor, custom_package) = custom_format_donor();
        let custom_rebased = rebase_styles(&rebased.styles_xml, &custom_donor).unwrap();
        let custom_package = replace_donor_parts(
            &custom_package,
            &custom_rebased.styles_xml,
            &custom_rebased.worksheet_xml,
        );
        let custom_package = libreoffice_roundtrip(&soffice, &custom_package, "custom");
        let custom_rows = structured_rows(&custom_package);
        assert_eq!(normalized_format(&custom_rows[1].cells()[0]), Some("0.0000".to_owned()));
    }

    fn formatted_donor() -> (DonorWorksheet, Vec<u8>) {
        let mut row = DynamicRow::new();
        row.insert(
            "Date".to_owned(),
            CellValue::Date(NaiveDate::from_ymd_opt(2026, 8, 24).unwrap()),
        );
        row.insert("Time".to_owned(), CellValue::Time(NaiveTime::from_hms_opt(12, 30, 0).unwrap()));
        row.insert("Duration".to_owned(), CellValue::Duration(Duration::hours(27)));
        let options = WriteOptions::new().with_auto_filter(false);
        let donor = DonorBuilder::from_dynamic(&[row.clone()], &options).unwrap();
        let mut writer = XlsxWriter::new();
        writer.add_rows(&[row], &options).unwrap();
        let package = writer.save_to_bytes().unwrap();
        let extracted = extract_donor(package.clone(), 1, SheetVisibility::Visible).unwrap();
        assert_eq!(donor, extracted);
        (donor, package)
    }

    fn custom_format_donor() -> (DonorWorksheet, Vec<u8>) {
        let rows = [CustomFormatRow { custom: 12.3456 }];
        let options =
            WriteOptions::new().with_auto_filter(false).with_column_format("Custom", "0.0000");
        let donor = DonorBuilder::from_serialized(&rows, &options).unwrap();
        let mut writer = XlsxWriter::new();
        writer.add_serialized(&rows, &options).unwrap();
        let package = writer.save_to_bytes().unwrap();
        (donor, package)
    }

    fn target_styles() -> String {
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="200" formatCode="existing-only"/></numFmts><fonts count="1"><font><name val="Target"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="200" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><dxfs count="0"/><extLst><ext uri="preserve-me"><future value="unchanged"/></ext></extLst></styleSheet>"#.to_owned()
    }

    fn number_format_ids(xml: &[u8]) -> Vec<u32> {
        StyleDocument::parse(xml)
            .unwrap()
            .nodes(StyleSection::NumFmts)
            .unwrap()
            .iter()
            .map(|node| numeric_attribute(node.root().unwrap(), b"numFmtId").unwrap().unwrap())
            .collect()
    }

    fn worksheet_style_ids(xml: &[u8]) -> Vec<u32> {
        let mut reader = Reader::from_reader(xml);
        let mut styles = Vec::new();
        loop {
            match reader.read_event().unwrap() {
                Event::Start(cell) | Event::Empty(cell)
                    if local_name(cell.name().as_ref()) == b"c" =>
                {
                    styles.push(numeric_attribute(&cell, b"s").unwrap().unwrap_or(0));
                }
                Event::Eof => break,
                _ => {}
            }
        }
        styles
    }

    fn replace_donor_parts(package: &[u8], styles: &[u8], worksheet: &[u8]) -> Vec<u8> {
        let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let mut payload = Vec::new();
            entry.read_to_end(&mut payload).unwrap();
            if name == "xl/styles.xml" {
                payload = styles.to_vec();
            } else if name == "xl/worksheets/sheet1.xml" {
                payload = worksheet.to_vec();
            }
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .last_modified_time(entry.last_modified().unwrap_or_default());
            writer.start_file(name, options).unwrap();
            writer.write_all(&payload).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn structured_rows(package: &[u8]) -> Vec<crate::StructuredRow> {
        let mut rows = Vec::new();
        MiniExcel::visit_structured_rows_from_reader(
            &mut Cursor::new(package),
            &ReadOptions::new(),
            |row| {
                rows.push(row.clone());
                Ok(true)
            },
        )
        .unwrap();
        rows
    }

    fn normalized_format(cell: &crate::StructuredCell) -> Option<String> {
        cell.number_format()
            .map(|format| format.replace('\\', "").to_ascii_lowercase().replace("[hh]", "[h]"))
    }

    fn libreoffice_roundtrip(soffice: &Path, package: &[u8], name: &str) -> Vec<u8> {
        let root = tempfile::tempdir().unwrap();
        let source_directory = root.path().join("source");
        let output_directory = root.path().join("output");
        let profile_directory = root.path().join("profile");
        std::fs::create_dir_all(&source_directory).unwrap();
        std::fs::create_dir_all(&output_directory).unwrap();
        std::fs::create_dir_all(&profile_directory).unwrap();
        let source = source_directory.join(format!("{name}.xlsx"));
        std::fs::write(&source, package).unwrap();
        let profile_uri = format!(
            "-env:UserInstallation=file:///{}",
            profile_directory.to_string_lossy().replace('\\', "/")
        );
        let output = Command::new(soffice)
            .args(["--headless", "--nologo", "--nodefault", "--nofirststartwizard"])
            .arg(profile_uri)
            .arg("--convert-to")
            .arg("xlsx:Calc MS Excel 2007 XML")
            .arg("--outdir")
            .arg(&output_directory)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "LibreOffice failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let converted = output_directory.join(format!("{name}.xlsx"));
        assert!(converted.is_file(), "LibreOffice did not create {}", converted.display());
        std::fs::read(converted).unwrap()
    }
}
