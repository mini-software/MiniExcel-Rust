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
    id: u32,
    index: usize,
    name: String,
    sheet_type: SheetType,
    visibility: SheetVisibility,
    is_active: bool,
}

impl SheetInfo {
    pub(crate) fn new(
        id: u32,
        index: usize,
        name: String,
        sheet_type: SheetType,
        visibility: SheetVisibility,
        is_active: bool,
    ) -> Self {
        Self { id, index, name, sheet_type, visibility, is_active }
    }

    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
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

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }
}
