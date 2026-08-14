#![forbid(unsafe_code)]

//! Experimental Rust XLSX support for MiniExcel.

mod cell;
mod error;
mod facade;
mod options;
mod reader;
pub mod serde_helpers;
mod sheet;
mod streaming;
mod writer;

pub use cell::{CellReference, CellValue, DynamicRow, ExcelRange};
pub use error::{Error, Result};
pub use facade::MiniExcel;
pub use options::{HeaderMode, ReadOptions, WriteOptions};
pub use sheet::{SheetInfo, SheetType, SheetVisibility};
