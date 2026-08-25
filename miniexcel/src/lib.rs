#![forbid(unsafe_code)]

//! Experimental Rust XLSX support for MiniExcel.

mod analytics;
#[cfg(all(feature = "async", not(target_arch = "wasm32")))]
mod cancellation;
mod cell;
mod comments;
mod error;
mod facade;
#[cfg(not(target_arch = "wasm32"))]
mod insert;
mod options;
mod rag;
mod reader;
pub mod serde_helpers;
mod sheet;
mod streaming;
mod template;
mod writer;

pub use analytics::{
    AggregateOp, AggregateSpec, AnalysisResult, AnalysisRow, AnalysisStats, ComparisonOp,
    FilterExpr, QueryLiteral, QueryPlan,
};
#[cfg(all(feature = "async", not(target_arch = "wasm32")))]
pub use cancellation::CancellationToken;
pub use cell::{CellReference, CellValue, DynamicRow, ExcelRange, StructuredCell, StructuredRow};
pub use comments::{
    CommentPerson, CommentTimestamp, NoteComment, SheetComments, ThreadedComment,
    ThreadedCommentReply,
};
pub use error::{Error, Result};
pub use facade::MiniExcel;
pub use options::{
    ExistingSheetPolicy, HeaderMode, HeaderStyle, HorizontalAlignment, InsertOptions, ReadOptions,
    RgbColor, TableStyle, TargetRelationshipPolicy, TemplateOptions, VerticalAlignment,
    WriteOptions,
};
pub use rag::{
    FormulaCalculationStatus, RagCell, RagChunk, RagExport, RagExportOptions, RagManifest, RagRow,
    RagValue,
};
pub use sheet::{SheetInfo, SheetType, SheetVisibility};
pub use streaming::{ByteQuerySummary, QuerySummary};
