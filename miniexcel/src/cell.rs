use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub(crate) const MAX_EXCEL_COLUMN: usize = 16_383;
const MAX_EXCEL_ROW: usize = 1_048_575;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CellValue {
    Empty,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
    Duration(Duration),
    Error(String),
}

impl CellValue {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

pub type DynamicRow = IndexMap<String, CellValue>;

/// A cell emitted by the structure-preserving XLSX stream.
#[derive(Clone, Debug, PartialEq)]
pub struct StructuredCell {
    row: u32,
    column: u32,
    value: CellValue,
    formula: Option<String>,
    style_id: u32,
    number_format: Option<Arc<str>>,
}

impl StructuredCell {
    pub(crate) fn new(
        row: usize,
        column: usize,
        value: CellValue,
        formula: Option<String>,
        style_id: u32,
        number_format: Option<Arc<str>>,
    ) -> Self {
        Self {
            row: (row + 1) as u32,
            column: (column + 1) as u32,
            value,
            formula,
            style_id,
            number_format,
        }
    }

    /// Returns the one-based Excel row index.
    #[must_use]
    pub const fn row_index(&self) -> u32 {
        self.row
    }

    /// Returns the one-based Excel column index.
    #[must_use]
    pub const fn column_index(&self) -> u32 {
        self.column
    }

    /// Returns the A1 cell address.
    #[must_use]
    pub fn address(&self) -> String {
        CellReference { row: self.row as usize - 1, column: self.column as usize - 1 }.to_string()
    }

    #[must_use]
    pub const fn value(&self) -> &CellValue {
        &self.value
    }

    /// Returns the raw OOXML formula text without evaluating it.
    #[must_use]
    pub fn formula(&self) -> Option<&str> {
        self.formula.as_deref()
    }

    #[must_use]
    pub const fn style_id(&self) -> u32 {
        self.style_id
    }

    /// Returns the OOXML number format associated with the cell style when known.
    #[must_use]
    pub fn number_format(&self) -> Option<&str> {
        self.number_format.as_deref()
    }
}

/// A sparse, structure-preserving row from an XLSX worksheet.
#[derive(Clone, Debug, PartialEq)]
pub struct StructuredRow {
    sheet_name: Arc<str>,
    row: u32,
    cells: Vec<StructuredCell>,
}

impl StructuredRow {
    pub(crate) fn new(sheet_name: Arc<str>, row: usize, cells: Vec<StructuredCell>) -> Self {
        Self { sheet_name, row: (row + 1) as u32, cells }
    }

    #[must_use]
    pub fn sheet_name(&self) -> &str {
        &self.sheet_name
    }

    /// Returns the one-based Excel row index.
    #[must_use]
    pub const fn row_index(&self) -> u32 {
        self.row
    }

    /// Returns only cells explicitly represented in the worksheet XML.
    #[must_use]
    pub fn cells(&self) -> &[StructuredCell] {
        &self.cells
    }

    #[must_use]
    pub fn into_cells(self) -> Vec<StructuredCell> {
        self.cells
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CellReference {
    row: usize,
    column: usize,
}

impl CellReference {
    pub const A1: Self = Self { row: 0, column: 0 };

    pub fn new(row: usize, column: usize) -> Result<Self> {
        if row > MAX_EXCEL_ROW || column > MAX_EXCEL_COLUMN {
            return Err(Error::invalid_cell_reference(format!(
                "row {}, column {}",
                row + 1,
                column + 1
            )));
        }

        Ok(Self { row, column })
    }

    #[must_use]
    pub(crate) const fn row(self) -> usize {
        self.row
    }

    #[must_use]
    pub(crate) const fn column(self) -> usize {
        self.column
    }
}

impl Default for CellReference {
    fn default() -> Self {
        Self::A1
    }
}

impl fmt::Display for CellReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut column = self.column + 1;
        let mut letters = [0_u8; 3];
        let mut length = 0;

        while column > 0 {
            column -= 1;
            letters[length] = b'A' + (column % 26) as u8;
            length += 1;
            column /= 26;
        }

        for letter in letters[..length].iter().rev() {
            formatter.write_str(char::from(*letter).encode_utf8(&mut [0; 4]))?;
        }

        write!(formatter, "{}", self.row + 1)
    }
}

impl FromStr for CellReference {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let reference = value.trim();
        let bytes = reference.as_bytes();
        let mut index = 0;

        if bytes.get(index) == Some(&b'$') {
            index += 1;
        }

        let column_start = index;
        let mut column = 0_usize;
        while let Some(byte) = bytes.get(index).copied() {
            if !byte.is_ascii_alphabetic() {
                break;
            }

            column = column
                .checked_mul(26)
                .and_then(|current| {
                    current.checked_add(usize::from(byte.to_ascii_uppercase() - b'A' + 1))
                })
                .ok_or_else(|| Error::invalid_cell_reference(reference))?;
            index += 1;
        }

        if index == column_start || column == 0 || column - 1 > MAX_EXCEL_COLUMN {
            return Err(Error::invalid_cell_reference(reference));
        }

        if bytes.get(index) == Some(&b'$') {
            index += 1;
        }

        let row_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }

        if index == row_start || index != bytes.len() {
            return Err(Error::invalid_cell_reference(reference));
        }

        let row = reference[row_start..]
            .parse::<usize>()
            .map_err(|_| Error::invalid_cell_reference(reference))?;
        if row == 0 || row - 1 > MAX_EXCEL_ROW {
            return Err(Error::invalid_cell_reference(reference));
        }

        Ok(Self { row: row - 1, column: column - 1 })
    }
}

impl TryFrom<&str> for CellReference {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        value.parse()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExcelRange {
    start_cell: Option<CellReference>,
    end_cell: Option<CellReference>,
}

impl ExcelRange {
    pub(crate) fn from_bounds(
        start_row: Option<usize>,
        start_column: Option<usize>,
        end_row: Option<usize>,
        end_column: Option<usize>,
    ) -> Self {
        let start_cell =
            start_row.zip(start_column).map(|(row, column)| CellReference { row, column });
        let end_cell = end_row.zip(end_column).map(|(row, column)| CellReference { row, column });
        Self { start_cell, end_cell }
    }

    #[must_use]
    pub const fn start_cell(&self) -> Option<CellReference> {
        self.start_cell
    }

    #[must_use]
    pub const fn end_cell(&self) -> Option<CellReference> {
        self.end_cell
    }

    #[must_use]
    pub const fn start_row_index(&self) -> Option<usize> {
        match self.start_cell {
            Some(cell) => Some(cell.row + 1),
            None => None,
        }
    }

    #[must_use]
    pub const fn end_row_index(&self) -> Option<usize> {
        match self.end_cell {
            Some(cell) => Some(cell.row + 1),
            None => None,
        }
    }

    #[must_use]
    pub const fn start_column_index(&self) -> Option<usize> {
        match self.start_cell {
            Some(cell) => Some(cell.column + 1),
            None => None,
        }
    }

    #[must_use]
    pub const fn end_column_index(&self) -> Option<usize> {
        match self.end_cell {
            Some(cell) => Some(cell.column + 1),
            None => None,
        }
    }

    #[must_use]
    pub const fn row_count(&self) -> usize {
        match (self.start_cell, self.end_cell) {
            (Some(start), Some(end)) => end.row - start.row + 1,
            _ => 0,
        }
    }

    #[must_use]
    pub const fn column_count(&self) -> usize {
        match (self.start_cell, self.end_cell) {
            (Some(start), Some(end)) => end.column - start.column + 1,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CellReference;

    #[test]
    fn parses_and_formats_a1_references() {
        for (value, row, column, canonical) in [
            ("A1", 0, 0, "A1"),
            ("$b$2", 1, 1, "B2"),
            ("AA10", 9, 26, "AA10"),
            ("XFD1048576", 1_048_575, 16_383, "XFD1048576"),
        ] {
            let reference: CellReference = value.parse().expect("valid cell reference");
            assert_eq!(reference.row(), row);
            assert_eq!(reference.column(), column);
            assert_eq!(reference.to_string(), canonical);
        }
    }

    #[test]
    fn rejects_invalid_a1_references() {
        for value in ["", "A", "1", "A0", "XFE1", "A1048577", "A1x", "$$A1"] {
            assert!(value.parse::<CellReference>().is_err(), "{value} should be invalid");
        }
    }
}
