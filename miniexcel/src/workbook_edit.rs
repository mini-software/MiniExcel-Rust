use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{CellReference, Result, RgbColor};

/// A deferred set of edits for one worksheet in an existing XLSX workbook.
#[derive(Debug)]
pub struct WorkbookEditor {
    path: PathBuf,
    sheet_name: String,
    font_colors: Vec<(CellReference, RgbColor)>,
}

impl WorkbookEditor {
    pub(crate) fn new(path: PathBuf, sheet_name: String) -> Self {
        Self { path, sheet_name, font_colors: Vec::new() }
    }

    /// Sets a cell's font color while preserving its other style properties.
    #[must_use]
    pub fn set_font_color(mut self, cell: CellReference, color: RgbColor) -> Self {
        self.font_colors.push((cell, color));
        self
    }

    /// Orders, deduplicates, and atomically applies all deferred edits.
    pub fn save(self) -> Result<()> {
        let mut font_colors = BTreeMap::new();
        for (cell, color) in self.font_colors {
            font_colors.insert(cell, color);
        }
        crate::insert::atomic::update_font_colors_to_path(
            &self.path,
            &self.sheet_name,
            &font_colors,
        )
    }
}
