use std::collections::HashSet;
use std::io::{Cursor, Read, Seek};
use std::path::Path;

use calamine::{Data, RangeDeserializerBuilder};
use serde::de::DeserializeOwned;

use crate::reader::row_to_range;
use crate::{CellReference, Error, ReadOptions, Result};

/// An ordered mapping from Serde field names to exact worksheet cells.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CellMap {
    sheet_name: Option<String>,
    bindings: Vec<(String, CellReference)>,
}

impl CellMap {
    #[must_use]
    pub const fn new() -> Self {
        Self { sheet_name: None, bindings: Vec::new() }
    }

    #[must_use]
    pub fn with_sheet_name(mut self, sheet_name: impl Into<String>) -> Self {
        self.sheet_name = Some(sheet_name.into());
        self
    }

    #[must_use]
    pub fn with_cell(mut self, field: impl Into<String>, cell: CellReference) -> Self {
        self.bindings.push((field.into(), cell));
        self
    }

    #[must_use]
    pub fn sheet_name(&self) -> Option<&str> {
        self.sheet_name.as_deref()
    }

    pub fn cells(&self) -> impl Iterator<Item = (&str, CellReference)> {
        self.bindings.iter().map(|(field, cell)| (field.as_str(), *cell))
    }
}

pub(crate) fn read_path<T>(path: impl AsRef<Path>, mapping: &CellMap) -> Result<T>
where
    T: DeserializeOwned,
{
    validate(mapping)?;
    let mut file = std::fs::File::open(path)?;
    read_from_reader_validated(&mut file, mapping)
}

pub(crate) fn read_bytes<T>(bytes: &[u8], mapping: &CellMap) -> Result<T>
where
    T: DeserializeOwned,
{
    validate(mapping)?;
    read_from_reader_validated(&mut Cursor::new(bytes), mapping)
}

pub(crate) fn read_from_reader<T, R>(reader: &mut R, mapping: &CellMap) -> Result<T>
where
    T: DeserializeOwned,
    R: Read + Seek,
{
    validate(mapping)?;
    read_from_reader_validated(reader, mapping)
}

fn read_from_reader_validated<T, R>(reader: &mut R, mapping: &CellMap) -> Result<T>
where
    T: DeserializeOwned,
    R: Read + Seek,
{
    let min_row = mapping.bindings.iter().map(|(_, cell)| cell.row()).min().unwrap_or(0);
    let max_row = mapping.bindings.iter().map(|(_, cell)| cell.row()).max().unwrap_or(0);
    let min_column = mapping.bindings.iter().map(|(_, cell)| cell.column()).min().unwrap_or(0);
    let max_column = mapping.bindings.iter().map(|(_, cell)| cell.column()).max().unwrap_or(0);
    let mut options = ReadOptions::new()
        .with_start_cell(CellReference::new(min_row, min_column)?)
        .with_end_cell(CellReference::new(max_row, max_column)?)
        .with_ignore_empty_rows(true);
    if let Some(sheet_name) = mapping.sheet_name() {
        options = options.with_sheet_name(sheet_name);
    }

    let positions =
        mapping.bindings.iter().map(|(_, cell)| (cell.row(), cell.column())).collect::<Vec<_>>();
    let (resolved_sheet, values) =
        crate::streaming::read_mapped_values_from_reader(reader, &options, &positions)?;

    let headers =
        mapping.bindings.iter().map(|(field, _)| Data::String(field.clone())).collect::<Vec<_>>();
    let range = row_to_range(Some(&headers), &values);
    let mut builder = RangeDeserializerBuilder::new();
    builder.has_headers(true);
    builder
        .from_range::<Data, T>(&range)
        .and_then(|mut rows| {
            rows.next().unwrap_or_else(|| {
                Err(calamine::DeError::Custom("mapped cells did not produce a value".to_owned()))
            })
        })
        .map_err(|source| {
            Error::mapped_deserialize(
                resolved_sheet,
                mapping
                    .bindings
                    .iter()
                    .map(|(field, cell)| format!("{field}={cell}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                source,
            )
        })
}

fn validate(mapping: &CellMap) -> Result<()> {
    if mapping.bindings.is_empty() {
        return Err(Error::invalid_cell_map("mapping must contain at least one cell"));
    }
    if mapping.sheet_name.as_ref().is_some_and(|name| name.trim().is_empty()) {
        return Err(Error::invalid_cell_map("worksheet name cannot be blank"));
    }
    let mut fields = HashSet::new();
    let mut cells = HashSet::new();
    for (field, cell) in &mapping.bindings {
        if field.trim().is_empty() {
            return Err(Error::invalid_cell_map("mapped field name cannot be blank"));
        }
        if !fields.insert(field) {
            return Err(Error::invalid_cell_map(format!(
                "mapped field '{field}' appears more than once"
            )));
        }
        if !cells.insert(*cell) {
            return Err(Error::invalid_cell_map(format!("cell '{cell}' is mapped more than once")));
        }
    }
    Ok(())
}
