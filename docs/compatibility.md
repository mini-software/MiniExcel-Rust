# Rust XLSX Compatibility Notes

[简体中文](compatibility.zh-CN.md)

## Goal

The Rust MVP implements the smallest useful MiniExcel-style XLSX read/write surface behind one `MiniExcel` facade. It uses a focused OOXML pull parser for bounded-memory path queries, calamine data and Serde conversion internally, and rust_xlsxwriter for workbook generation.

## Dependency Baseline

| Dependency | Locked API line | Role | License | MSRV note |
| --- | --- | --- | --- | --- |
| `calamine` | 0.35 | XLSX parsing and Serde row deserialization | MIT | 0.35 declares Rust 1.83 |
| `clap` | 4.6 | Local CLI argument parsing | MIT OR Apache-2.0 | 4.6 declares Rust 1.85 |
| `rust_xlsxwriter` | 0.96 | New XLSX workbook generation and Serde serialization | MIT OR Apache-2.0 | 0.96 declares Rust 1.83 |
| `serde` | 1.x | Typed mapping | MIT OR Apache-2.0 | Resolved by the workspace lockfile |
| `chrono` | 0.4 | Timezone-free Excel date/time values | MIT OR Apache-2.0 | Resolved by the workspace lockfile |
| `indexmap` | 2.x | Stable dynamic column ordering | MIT OR Apache-2.0 | Resolved by the workspace lockfile |
| `quick-xml` | 0.39 | Incremental OOXML parsing | MIT | Locked and checked with Rust 1.85 |
| `serde_json` | 1.x | Query plans, analytics/RAG output, parity contracts, and CLI JSON | MIT OR Apache-2.0 | Checked with Rust 1.85 |
| `sha2` | 0.10 | Streaming SHA-256 source identity for RAG manifests | MIT OR Apache-2.0 | Checked with Rust 1.85 |
| `thiserror` | 2.x | Public error composition | MIT OR Apache-2.0 | Resolved by the workspace lockfile |
| `zip` | 7.2 | Incremental worksheet entry decompression | MIT | Locked and checked with Rust 1.85 |

The latest `calamine 0.36` and `rust_xlsxwriter 0.97` require Rust 1.88. The MVP pins the preceding API lines so the declared Rust 1.85 MSRV is executable rather than aspirational.

## API Mapping

| MiniExcel V2 concept | Rust MVP | Notes |
| --- | --- | --- |
| OpenXML importer | `MiniExcel` | Concrete reader/parser types are internal |
| Dynamic `Query` | `MiniExcel::query()` | Streams owned `IndexMap<String, CellValue>` rows with bounded buffering |
| Typed `Query<T>` | `MiniExcel::query_as<T>()` | Streams rows and applies Serde mapping one row at a time |
| Structure-preserving query | `MiniExcel::query_structured()` | Streams sparse rows with one-based coordinates, formulas, style IDs, and number formats |
| Group/filter analytics | `MiniExcel::analyze_with_options()` | Versioned Rust extension; streams rows and retains only bounded group/evidence state |
| RAG evidence export | `MiniExcel::export_rag()` | Versioned Rust extension; streams addressed JSONL-ready chunks, enriched GFM Markdown, and a source manifest |
| `QueryRange` | `ReadOptions::with_start_cell()` / `with_end_cell()` | Inclusive A1 range for dynamic and typed reads |
| `GetSheetNames` | `MiniExcel::get_sheet_names()` | Workbook order is preserved |
| `GetSheetInformations` | `MiniExcel::get_sheet_info()` | Includes OOXML ID, order, name, type, visibility, and active state |
| `GetSheetDimensions` | `MiniExcel::get_sheet_dimensions()` | Returns used ranges in workbook order with 1-based indices |
| `GetColumns` | `MiniExcel::get_columns()` | Returns selected dynamic keys or an empty vector |
| `startCell` | `ReadOptions::with_start_cell()` | A1 start coordinate |
| `IgnoreEmptyRows` | `ReadOptions::with_ignore_empty_rows()` | Defaults to `false` for MiniExcel compatibility |
| `FillMergedCells` | `ReadOptions::with_fill_merged_cells()` | Defaults to `false`; applies to dynamic, typed, and byte queries |
| OpenXML exporter | `MiniExcel::save_as*()` | Concrete writer type is internal; creates new workbooks only |
| Dynamic export | `save_as()` / `save_as_with_schema()` | Map serialization is implemented internally |
| Typed export | `save_as_serialized<T>()` | Uses Serde mapping internally |
| Multi-sheet export | `save_as_sheets()` / `save_as_serialized_sheets()` | Preserves input sheet order and returns data-row counts |
| `overwriteFile` | `WriteOptions::with_overwrite_file()` | Defaults to `false`; existing paths require explicit opt-in |
| `FreezeRowCount` / `FreezeColumnCount` | `WriteOptions::with_freeze_row_count()` / `with_freeze_column_count()` | Defaults to one frozen row and zero frozen columns |
| `AutoFilter` | `WriteOptions::with_auto_filter()` | Defaults to `true`; covers the complete written range |
| `RightToLeft` | `WriteOptions::with_right_to_left()` | Defaults to `false`; changes worksheet view only |
| Basic template fill | `save_as_template()` / `save_as_template_bytes()` | Scalar placeholders and single-row array expansion; preserves package parts |

`MiniExcel` is the only public behavior entry point. Reader, writer, parser, and concrete iterator types are crate-internal. Public supporting types are limited to row/cell values, structured provenance rows, options, errors/results, and Serde date/time helpers.

## Compatibility Defaults

- `MiniExcel::query()` with `HeaderMode::Auto` uses column letters and treats the first row as data.
- `MiniExcel::query_as()` with `HeaderMode::Auto` consumes the first selected row as headers.
- `MiniExcel::query_structured()` never consumes a header row and emits only cells explicitly represented in worksheet XML.
- The first worksheet in workbook order is selected when no name is supplied.
- Empty rows between the selected start and last used cell are retained by default.
- Merged ranges expose only their physical top-left value unless `fill_merged_cells` is enabled. Structured queries never synthesize merged cells.
- Typed header strings are trimmed by default. Dynamic headers follow the .NET behavior and retain non-blank text as stored.
- Blank dynamic headers are omitted. Duplicate dynamic headers retain their first key position while later columns overwrite the value.
- A missing dynamic cell is represented by `CellValue::Empty`, not by omission from a known schema.
- Writer row counts exclude the header row.

## Type Mapping

| XLSX value | Dynamic Rust value |
| --- | --- |
| Empty | `CellValue::Empty` |
| Boolean | `CellValue::Bool` |
| Exact integral number in `i64` range | `CellValue::Int` |
| Other number | `CellValue::Float` |
| Shared/inline string | `CellValue::String` |
| Excel serial date/time | `CellValue::DateTime` |
| Excel duration | `CellValue::Duration` |
| ISO date/time | `Date`, `Time`, or `DateTime` when parseable |
| Cell error | `CellValue::Error` |
| Formula through dynamic/typed query | Cached result value only |
| Formula through structured query | Raw formula text and cached result value; no calculation |

Typed conversions are delegated to calamine's Serde deserializer. The public `serde_helpers` module adds strict chrono helpers that convert an invalid value into the library's contextual `Error::Deserialize` path.

For typed writing, chrono values must use the matching MiniExcel helper (`serialize_date_to_excel`, `serialize_datetime_to_excel`, or `serialize_time_to_excel`) and a corresponding `WriteOptions::with_column_format()` entry. Otherwise standard chrono Serde behavior writes text rather than an Excel serial value.

## Memory And I/O Model

`MiniExcel::query()` and `query_as()` use a dedicated path-streaming backend. A worker owns the ZIP archive, reads workbook relationships, styles, and shared strings, then processes worksheet XML with quick-xml. A bounded channel holds at most eight parsed rows. Dropping the public iterator disconnects the channel and joins the worker, so an early `take` or `find` stops further work.

Path queries automatically store `xl/sharedStrings.xml` in indexed temporary files when its uncompressed size is at least 5 MiB. `ReadOptions` can disable the cache, change the threshold, or select an existing cache directory. The index uses fixed-width offset/length records, so lookup metadata does not grow in memory with string count. Normal completion, parser failure, and early iterator drop remove the files through worker-owned RAII cleanup. Byte/WASM queries remain memory-only because they do not own a native temporary-filesystem contract.

`MiniExcel::query_structured()` uses the same bounded pipeline and additionally retains metadata for explicit cells in the current row and channel. Sheet names are shared per row, and number-format strings are shared by style. Missing cells are not expanded into structured cell objects. Formula expressions are preserved exactly as stored, but shared formulas are not expanded and cached values can be stale.

Grouped analytics consume the dynamic row stream without retaining source rows. Memory additionally contains one aggregate state and bounded source-row evidence list per distinct group. `QueryPlan::max_groups` rejects the group that would exceed the configured limit. Result limits do not reduce group-state memory. Version 1 does not implement disk spill, sorted-input aggregation, or constant-memory high-cardinality grouping.

Path RAG exports retain parser state, repeated header context, and one output chunk. Their manifest hashes the source file through a separate bounded read. Markdown includes stream-level source/sheet provenance and chunk-local formula/style/number-format metadata without retaining prior chunks. Byte/WASM workflows avoid collecting source rows, but browser uploads inherently retain compressed XLSX bytes in WebAssembly memory; generated JSONL, Markdown, and Blob downloads also consume output-sized memory. Browser Lab runs these operations in a Web Worker for responsiveness, not as a claim of path-equivalent memory.

The backend makes two sequential, bounded-memory passes over the selected worksheet entry. The first records the used extent and compact merged-cell rectangles. This is required for MiniExcel-compatible stable dynamic schemas when legal files omit `<dimension>`, to preserve style-only row elements like the .NET reader, and to support opt-in merged-cell filling without expanding ranges into an address map. The second pass emits rows and retains only anchor values for currently active merged ranges. Worksheet XML and prior rows are never retained; memory consists primarily of in-memory or disk-indexed shared strings, styles, merge metadata, parser buffers, the current row, and the bounded channel.

The internal writer assembles a new ZIP package with one or more worksheets. Path saves refuse existing files by default and can explicitly replace them, but cannot patch or insert sheets into an existing workbook. Template fills rewrite worksheet XML within a copied package; worksheet styles and unrelated ZIP parts are retained. Array expansion shifts row and cell addresses and updates the worksheet dimension. Formula expressions are preserved but not recalculated, and version 1 does not adjust formula references, merged ranges, tables, drawings, or defined names after inserted rows.

## Test Sources

Rust integration tests reuse the repository's existing files under `tests/data/xlsx`, including:

- Dynamic header and no-header files.
- Center and self-closing empty rows.
- Typed value and trimmed-header mapping.
- Multiple worksheets.
- Cells without explicit `r` attributes.
- A typed conversion failure with a verified Excel row number.
- Strict streaming A1 starts, empty-row filtering, dates, trimmed headers, and early typed errors.
- Opt-in vertical and horizontal merged-cell filling across dynamic, typed, and byte queries.
- Forced shared-string disk spill, indexed lookup, invalid-directory handling, memory-only byte queries, and early-drop cleanup.
- Structured formula text, cached values, A1 addresses, style IDs, built-in/custom number formats, ranges, and early iterator drop.

Writer tests generate temporary workbooks through `MiniExcel::save_as*()` and read them back through `MiniExcel::query*()`, covering dynamic and typed values, dates, multiple worksheets, row counts, empty schemas, default/custom/disabled freeze panes, header/headerless/typed AutoFilter ranges, right-to-left views, explicit path overwrite behavior, and worksheet-name validation. Template tests cover scalar and mixed text, native numbers and booleans, XML escaping, formula-injection protection, missing-variable policy, empty and populated arrays, multiple sheets, style retention, path overwrite, and byte workflows. The WASM adapter has native unit tests, while Browser Lab Playwright tests cover generated-workbook rendering, query controls, inclusive end ranges, and desktop/mobile viewports.

## .NET Parity Contract

Behavior shared by .NET and Rust is defined in `tests/data/contracts/xlsx-parity-v1.json`. This file is the single expected-data source for:

- [`MiniExcel.OpenXml.Tests/Compatibility/RustParityContractTests.cs`](https://github.com/mini-software/MiniExcel/blob/master/tests/MiniExcel.OpenXml.Tests/Compatibility/RustParityContractTests.cs)
- `miniexcel/tests/parity_contract.rs`

Both adapters use their public APIs, query the same XLSX fixtures, normalize language-specific representations, and compare sheet order, row counts, column order, selected values, and common conversion-error context. Normalization maps null/empty cells, booleans, numbers, GUIDs, datetimes, durations, and strings to stable tagged text. In particular, integral .NET `double` and Rust `CellValue::Int` values compare as the same number, and ISO date strings compare with chrono date/time values.

Run both sides from the repository root:

```bash
cargo +1.85.0 test -p miniexcel --test parity_contract --locked
dotnet test ../MiniExcel/tests/MiniExcel.OpenXml.Tests/MiniExcel.OpenXml.Tests.csproj --framework net10.0 --filter "FullyQualifiedName~RustParityContractTests"
```

The Rust workflow runs the Rust contract on Linux and Windows. Its .NET parity job checks out the MiniExcel repository, copies this revision's contract into that checkout, and runs the .NET adapter on Linux. A compatibility change is complete only when the shared contract is updated deliberately and both adapters pass it.

The contract covers only the current common surface: dynamic/typed path queries, inclusive range queries, column-name discovery, header behavior, sheet selection/order, A1 starts, empty/style-only rows, inferred cell references, scalar/date/duration mapping, trimmed typed headers, and conversion-error row/value context. Structured provenance is a Rust research extension and is not a .NET parity claim. Async APIs, DataReader, templates, and writing parity remain outside version 1 and must not be described as equivalent yet.

## .NET Coverage Boundary

| .NET surface | Rust status | Shared contract |
| --- | --- | --- |
| Dynamic and typed XLSX query | Implemented | Yes |
| `QueryRange` with A1 coordinates | Implemented | Yes |
| `GetSheetNames` and `GetColumns` | Implemented | Yes |
| `GetSheetInformations` ID/index/name/type/visibility/active | Implemented | Rust tests against .NET fixtures |
| `GetSheetDimensions` | Implemented | Rust tests against .NET fixtures |
| New-workbook `SaveAs`, including multiple sheets | Implemented and roundtrip-tested | Not yet |
| Basic `SaveAsTemplate` scalar/list fill | Implemented and roundtrip-tested | Not yet |
| Byte-array query/write for WASM | Implemented | Rust/browser tests |
| Versioned grouped analytics | Rust research extension | No |
| Addressed JSONL/Markdown/manifest RAG export | Rust research extension | No |
| Async APIs, DataReader, stream ownership | Deferred | No |
| Insert/edit existing workbooks | Deferred | No |
| CSV and legacy formats | Deferred | No |
| Advanced templates, pictures, merges, comments | Deferred | No |

This matrix is the coverage claim: Rust does not yet provide complete API parity with the current .NET packages.

## Deferred Work

SQL text parsing, `HAVING`, `ORDER BY`, joins, windows, pivots, disk-spill aggregation, vector indexing, model calls, CSV providers, old Excel formats, advanced template directives and sheet cloning, images, merged-cell APIs, formula calculation/dependency expansion, formula authoring, general styling, modifying existing workbooks, async I/O, and streaming from caller-owned readers require separate design and acceptance milestones.