use calamine::{Sheet, SheetType as CalamineSheetType, SheetVisible};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SheetType {
    Worksheet,
    DialogSheet,
    MacroSheet,
    ChartSheet,
    Vba,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SheetVisibility {
    Visible,
    Hidden,
    VeryHidden,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetInfo {
    index: usize,
    name: String,
    sheet_type: SheetType,
    visibility: SheetVisibility,
}

impl SheetInfo {
    pub(crate) fn from_calamine(index: usize, sheet: &Sheet) -> Self {
        Self {
            index,
            name: sheet.name.clone(),
            sheet_type: match sheet.typ {
                CalamineSheetType::WorkSheet => SheetType::Worksheet,
                CalamineSheetType::DialogSheet => SheetType::DialogSheet,
                CalamineSheetType::MacroSheet => SheetType::MacroSheet,
                CalamineSheetType::ChartSheet => SheetType::ChartSheet,
                CalamineSheetType::Vba => SheetType::Vba,
            },
            visibility: match sheet.visible {
                SheetVisible::Visible => SheetVisibility::Visible,
                SheetVisible::Hidden => SheetVisibility::Hidden,
                SheetVisible::VeryHidden => SheetVisibility::VeryHidden,
            },
        }
    }

    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn sheet_type(&self) -> SheetType {
        self.sheet_type
    }

    #[must_use]
    pub const fn visibility(&self) -> SheetVisibility {
        self.visibility
    }
}
