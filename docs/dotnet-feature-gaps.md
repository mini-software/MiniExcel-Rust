# MiniExcel .NET Feature Gap Analysis

[简体中文](dotnet-feature-gaps.zh-CN.md)

## Scope

This report compares observable public capabilities in the local checkouts below. It is a point-in-time implementation backlog, not a claim that every .NET API should be copied into Rust.

| Project | Revision |
| --- | --- |
| MiniExcel-Rust | `f8f3edcdceb977f10f86b07ce3fee6789e97eb7e` plus the current Save working-tree changes (`0.1.0`) |
| MiniExcel .NET | `5beb8b6986e93213af0b7ad8f0f1f6351b505d7e` (`2.0.0-preview.4-23-g5beb8b6`) |

The comparison uses the .NET public APIs, their controlling implementations, and focused tests under the sibling `../MiniExcel` checkout. Rust status is based on the public `MiniExcel` facade, options, integration tests, and [compatibility boundary](compatibility.md).

Status meanings:

- **Partial**: Rust covers the core scenario but not the full observable .NET behavior.
- **Missing**: no equivalent public Rust capability exists.
- **Different by design**: Rust provides a native alternative, but it is not API or behavior parity.

## Implemented Baseline

Rust already implements dynamic and Serde-typed XLSX path queries, inclusive A1 ranges, sheet selection, header and empty-row options, sheet names/information/dimensions, bounded-memory row iteration, structured cell provenance, byte-oriented browser reads, and creation of new one- or multi-sheet XLSX workbooks. Those capabilities are not listed as missing below unless the .NET surface is broader.

## Gap Matrix

| Area | Status | Capability available in .NET but not fully implemented in Rust |
| --- | --- | --- |
| Named tables | Missing | Query an OpenXML table by table name (`QueryTable`), including table-specific headers and bounds. |
| DataReader and DataTable | Missing | `IDataReader`/async reader access, schema tables, typed getters, `NextResult` sheet traversal, and `DataTable` materialization. |
| Caller-owned streams | Missing | Read from and write to caller-owned streams, leave-open behavior, and stream ownership controls. Rust path APIs own their files; byte APIs are separate. |
| Async and cancellation | Missing | Async query/export/template operations, async row sources, cancellation tokens, and progress callbacks. Rust iterators are synchronous even though path reads use a worker thread. |
| General save inputs | Partial | Export from general objects/enumerables, dictionaries, `DataTable`, `IDataReader`, and async enumerables, with progress. Rust accepts dynamic or same-type Serde slices and reports per-sheet row counts. |
| Multi-sheet export | Partial | Rust creates multiple named worksheets in input order, but does not yet create hidden sheets or accept heterogeneous Serde row types in one call. |
| Existing-workbook operations | Missing | Insert or replace a sheet, copy and add a sheet, and rename, reorder, or change visibility of sheets. Rust always creates a new XLSX package. |
| Templates | Missing | Fill templates from paths, bytes, or streams; scalar/list expansion, grouping, nested values, and calculation-chain handling. |
| Pictures and merge processing | Missing | Add anchored pictures and merge adjacent identical cells through the templater surface. Structured reads do not provide authoring parity. |
| CSV | Missing | Dynamic/typed CSV query and save, append, columns, DataReader/DataTable, delimiter/newline/encoding/quoting configuration, and CSV/XLSX conversion. |
| Comments and notes | Missing | Retrieve threaded comments, replies, people/authors, resolution state, timestamps, and legacy notes. |
| Fluent mapping | Missing | Address-based object mapping, formula/format mappings, collection start cells and spacing, nested collections, and mapped import/export/template APIs. |
| Attribute-based mapping | Partial | Column index/name attributes, localized headers, width/hidden/formula metadata, custom dynamic formatters, field mapping, and dynamic column ordering/filtering. Serde covers rename, alias, defaults, skips, options, and custom serializers, but not these Excel-specific contracts. |
| Read configuration | Partial | Culture-aware conversion, merged-cell value filling, shared-string cache controls, buffer/fast modes, and some null/empty-string behavior. |
| Write configuration and style | Partial | Tables/autofilters, right-to-left sheets, frozen rows/columns, auto width, header style/alignment/wrapping, shared versus inline strings, and broader cell styling. Rust currently exposes sheet name, header output, and number formats. |
| Sheet metadata/workflow | Partial | Table metadata, comment metadata, dynamic sheet aliases, class-level sheet selection, and traversing all sheets through one reader. Rust already covers names, order, dimensions, visibility, and active state. |
| Provider/package model | Different by design | .NET composes OpenXML, CSV, templating, and fluent-mapping providers. Rust has a single XLSX crate plus CLI and WASM adapters; those adapters do not replace the missing provider capabilities. |

## Evidence Map

| Area | Rust evidence | .NET evidence in `../MiniExcel` |
| --- | --- | --- |
| Public read/write boundary | `miniexcel/src/facade.rs`, `miniexcel/src/options.rs` | `src/MiniExcel.OpenXml/Api/OpenXmlImporter.cs`, `OpenXmlExporter.cs` |
| Tables | No public table API | `OpenXmlImporter.QueryTableAsync`; `tests/MiniExcel.OpenXml.Tests/Tables/` |
| DataReader/DataTable | No public tabular adapter | `OpenXmlImporter.GetDataReader`, `GetAsyncDataReader`, `QueryAsDataTableAsync`; `tests/MiniExcel.OpenXml.Tests/DataReader/` |
| Multi-sheet and workbook edits | Writer creates multiple sheets in a new workbook; existing-workbook edits remain unsupported | `OpenXmlExporter.InsertSheetAsync`, `CopyAndAddSheetAsync`, `AlterSheetAsync`; `tests/MiniExcel.OpenXml.Tests/MultipleSheets/` and `AlterSheets/` |
| Templates/pictures/merges | Listed as deferred in `docs/compatibility.md` | `src/MiniExcel.OpenXml/Api/OpenXmlTemplater.cs`; `tests/MiniExcel.OpenXml.Tests/Templates/` |
| CSV/conversion | XLSX-only core | `src/MiniExcel.Csv/Api/`; `src/MiniExcel/MiniExcelConverter.cs`; `tests/MiniExcel.Csv.Tests/` |
| Mapping | Serde mapping only | `src/MiniExcel.Core/Attributes/MiniExcelColumnAttribute.cs`; `src/MiniExcel.OpenXml.FluentMapping/`; mapping tests |
| Comments | No public comment model/API | `OpenXmlImporter.RetrieveCommentsAsync`; `src/MiniExcel.OpenXml/Models/Comments.cs`; comment tests |
| Configuration/style | Narrow `ReadOptions` and `WriteOptions` | `MiniExcelBaseConfiguration`, `OpenXmlConfiguration`, `OpenXmlStyleOptions`; exporter tests |

The .NET APIs marked with `Async` also have generated synchronous counterparts through the repository's sync-version generation. The gap therefore concerns capability, not only method naming.

## Suggested Implementation Order

1. **Richer write options**: add hidden sheets, tables/autofilters, frozen panes, and style controls without changing the bounded-memory read architecture.
2. **Named-table query and comments**: focused OpenXML read features with clear fixtures and public result models.
3. **Caller-owned `Read`/`Write` APIs**: establish ownership and seekability contracts before adding async wrappers.
4. **Existing-workbook sheet operations**: requires a deliberate package-rewrite design and stronger corruption/atomicity tests.
5. **CSV provider**: should be a separate format boundary rather than conditionals inside the XLSX parser.
6. **Templates and Fluent Mapping**: largest independent surfaces; define compatibility milestones separately.

DataReader/DataTable are .NET ecosystem abstractions and should not be ported literally. A Rust-native record-batch or tabular adapter is appropriate only when a concrete integration requires it.

## Not Counted as Gaps

- Rust analytics and RAG exports are Rust extensions, not .NET parity claims.
- The .NET numeric `QueryRange` overload is not counted separately because Rust A1 `CellReference` bounds express the same selection.
- Internal implementation classes and .NET-specific dependency-injection mechanics are excluded unless they expose observable behavior.
- Legacy binary Excel formats are excluded because this comparison found no current .NET V2 public provider that would make them a parity requirement.

Re-run this comparison whenever either pinned revision changes. New parity claims should be added to the shared contract only after both public adapters and focused tests cover the behavior.