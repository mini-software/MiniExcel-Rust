# MiniExcel .NET Feature Gap Analysis

[简体中文](dotnet-feature-gaps.zh-CN.md)

## Scope

This report compares observable public capabilities in the local checkouts below. It is a point-in-time implementation backlog, not a claim that every .NET API should be copied into Rust.

| Project | Revision |
| --- | --- |
| MiniExcel-Rust | working tree based on `b166b708606a2046b62eefafa7893944bb3a2251` (`0.3.0`) |
| MiniExcel .NET | `b9a76d7af62142e0e38545b6905b01a06e8d160e` |

The comparison uses the .NET public APIs, their controlling implementations, and focused tests under the sibling `../MiniExcel` checkout. Rust status is based on the public `MiniExcel` facade, options, integration tests, and [compatibility boundary](compatibility.md).

Status meanings:

- **Implemented**: the useful observable XLSX behavior is covered by a Rust-native API and focused cross-project evidence.
- **Partial**: Rust covers the core scenario but not the full observable .NET behavior.
- **Missing**: no equivalent public Rust capability exists.
- **Different by design**: Rust provides a native alternative, but it is not API or behavior parity.

## Implemented Baseline

Rust already implements dynamic and Serde-typed XLSX path queries, inclusive A1 ranges, sheet selection, header and empty-row options, sheet names/information/dimensions, bounded-memory row iteration, structured cell provenance, byte-oriented browser reads, and creation of new one- or multi-sheet XLSX workbooks. Those capabilities are not listed as missing below unless the .NET surface is broader.

## Gap Matrix

| Area | Status | Capability available in .NET but not fully implemented in Rust |
| --- | --- | --- |
| Named tables | Implemented | Dynamic/typed path queries, byte queries, and borrowed-reader visitors use table metadata headers and bounds with case-insensitive table-name matching. |
| DataReader and DataTable | Different by design | Rust exposes iterators and borrowed visitors instead of .NET tabular interfaces. A Rust-native Arrow/record-batch adapter is deferred until a concrete integration requires it and does not block parity completion. |
| Caller-owned streams | Partial | Borrowed synchronous dynamic/typed/structured visitors, metadata reads, dynamic/schema/typed/multi-sheet writers, and separate reader-to-writer Insert are implemented with leave-open semantics. Borrowed lazy iterators, async streams, and template streams remain unsupported. |
| Async and cancellation | Partial | Optional runtime-neutral dynamic/Serde path queries and explicit-schema Insert support bounded streams and cooperative cancellation. Async export/template operations, async write sources, borrowed async readers, and progress callbacks remain unsupported. ZIP and filesystem work remains blocking on dedicated workers. |
| General save inputs | Partial | Export from general objects/enumerables, dictionaries, `DataTable`, `IDataReader`, and async enumerables, with progress. Rust accepts dynamic or same-type Serde slices and reports per-sheet row counts. |
| Multi-sheet export | Partial | Rust creates ordered visible, hidden, and very-hidden worksheets, but does not yet accept heterogeneous Serde row types in one call. |
| Existing-workbook operations | Partial | Rust appends or strictly replaces worksheets through atomic path APIs or separate borrowed streams, preserving unrelated package parts and worksheet identity. Copy-and-add, rename, reorder, and standalone visibility mutation remain unsupported. |
| Templates | Partial | Rust fills path/byte templates with scalar values and single-row arrays while preserving package parts. Streams, grouping, conditions, parametrized sheets, `$=` formulas, formula-reference updates, and calculation-chain handling remain unsupported. |
| Pictures and merge processing | Missing | Add anchored pictures and merge adjacent identical cells through the templater surface. Structured reads do not provide authoring parity. |
| CSV | Implemented | Dynamic/Serde path, byte, and borrowed query/save APIs; column discovery; inferred/explicit-schema append; delimiter/newline/encoding/BOM/null/quoting configuration; and `query-csv` CLI. DataReader/DataTable are replaced by Rust iterators. Async/progress APIs and a one-call CSV/XLSX converter are not exposed. |
| Comments and notes | Implemented | Path/bytes/borrowed APIs return threaded roots, replies, unresolved person IDs, people/provider/user IDs, resolution state, typed timestamps, and legacy notes. |
| Fluent mapping | Missing | Address-based object mapping, formula/format mappings, collection start cells and spacing, nested collections, and mapped import/export/template APIs. |
| Attribute-based mapping | Partial | Column index/name attributes, localized headers, formula metadata, custom dynamic formatters, field mapping, and dynamic column ordering/filtering remain. Serde covers rename, alias, defaults, skips, options, and custom serializers; `WriteOptions` covers width/hidden layout by final header name. |
| Read configuration | Partial | Culture-aware conversion, buffer/fast modes, and some null/empty-string behavior. Merged-cell filling and shared-string disk caching are implemented. |
| Write configuration and style | Partial | OOXML tables, shared versus inline strings, and broader cell styling remain. Rust exposes default/minimal cell style modes, header output/style, AutoFilter, right-to-left views, frozen rows/columns, bounded AutoWidth, body wrapping/alignment, and number formats. |
| Sheet metadata/workflow | Partial | Dynamic sheet aliases, class-level sheet selection, and traversing all sheets through one reader remain. Rust covers names, order, dimensions, visibility, active state, table metadata, and comments/notes. |
| Provider/package model | Different by design | .NET composes OpenXML, CSV, templating, and fluent-mapping providers. Rust exposes XLSX and CSV through one crate plus CLI and WASM adapters; package boundaries are not parity requirements. |

## Evidence Map

| Area | Rust evidence | .NET evidence in `../MiniExcel` |
| --- | --- | --- |
| Public read/write boundary | `miniexcel/src/facade.rs`, `miniexcel/src/options.rs` | `src/MiniExcel.OpenXml/Api/OpenXmlImporter.cs`, `OpenXmlExporter.cs` |
| Async query | `MiniExcel::query_async*` and `query_as_async*`; Rust focused parity/cancellation/error/cleanup tests | `OpenXmlImporter.QueryAsync`; `MiniExcelOpenXmlImporterAsyncTests` |
| Tables | `MiniExcel::query_table*`; Rust focused tests use the exact `TestQueryTable.xlsx` fixture (SHA-256 `04F719BF9F9E99D9B437A8FB32F8111FD92580A1D29ACAD10B6ED128C0564501`) | `OpenXmlImporter.QueryTableAsync`; `tests/MiniExcel.OpenXml.Tests/Tables/` |
| Comments | `MiniExcel::get_comments*`; Rust focused tests use `TestCommentsAndNotes.xlsx` (SHA-256 `3A855CE896ED62DC27C91797432DD89EE081F07CD03AB05BF1B0CD745543A3FC`) | `OpenXmlImporter.RetrieveCommentsAsync`; `tests/MiniExcel.OpenXml.Tests/Comments/` |
| DataReader/DataTable | Rust iterators and borrowed visitors are the native abstraction; no literal .NET tabular adapter is planned | `OpenXmlImporter.GetDataReader`, `GetAsyncDataReader`, `QueryAsDataTableAsync`; `tests/MiniExcel.OpenXml.Tests/DataReader/` |
| Multi-sheet and workbook edits | Writer creates multiple sheets; existing workbooks support append and strict replacement with package preservation and bounded explicit-schema producers | `OpenXmlExporter.InsertSheetAsync`, `CopyAndAddSheetAsync`, `AlterSheetAsync`; `tests/MiniExcel.OpenXml.Tests/MultipleSheets/` and `AlterSheets/` |
| Templates/pictures/merges | Basic template fill implemented; advanced directives and authoring remain deferred | `src/MiniExcel.OpenXml/Api/OpenXmlTemplater.cs`; `tests/MiniExcel.OpenXml.Tests/Templates/` |
| CSV/conversion | `MiniExcel::query_csv*`, `save_csv*`, and `append_csv*`; Rust tests use exact `TestHeader.csv` (`6C2FC27FCA2876F1ECCA17061B8EE23E133ECDB726F8E0B84167E58D86234432`) and GB2312 (`BA8A2505AB271D5575C58CC1FCBE5A5002CEB9E2F43CB95412246E25A50E8B5A`) fixtures | `src/MiniExcel.Csv/Api/`; `src/MiniExcel/MiniExcelConverter.cs`; `tests/MiniExcel.Csv.Tests/` |
| Mapping | Serde mapping only | `src/MiniExcel.Core/Attributes/MiniExcelColumnAttribute.cs`; `src/MiniExcel.OpenXml.FluentMapping/`; mapping tests |
| Comments | `MiniExcel::get_comments*`; Rust focused tests use `TestCommentsAndNotes.xlsx` (SHA-256 `3A855CE896ED62DC27C91797432DD89EE081F07CD03AB05BF1B0CD745543A3FC`) | `OpenXmlImporter.RetrieveCommentsAsync`; `src/MiniExcel.OpenXml/Models/Comments.cs`; comment tests |
| Configuration/style | Narrow `ReadOptions` and `WriteOptions` | `MiniExcelBaseConfiguration`, `OpenXmlConfiguration`, `OpenXmlStyleOptions`; exporter tests |

The .NET APIs marked with `Async` also have generated synchronous counterparts through the repository's sync-version generation. The gap therefore concerns capability, not only method naming.

## Suggested Implementation Order

1. **Async export/template APIs**: extend runtime-neutral producer/cancellation patterns without presenting blocking ZIP work as async I/O.
2. **Advanced templates and Fluent Mapping**: add grouped/conditional templates, parametrized sheets, and mapping through separate compatibility milestones.
3. **Remaining workbook edits**: copy/add, rename, reorder, and standalone visibility mutation require their own preservation contracts.

DataReader/DataTable are .NET ecosystem abstractions and are intentionally not literal Rust parity requirements. A Rust-native record-batch or tabular adapter is appropriate only when a concrete integration requires it.

## Not Counted as Gaps

- Rust analytics and RAG exports are Rust extensions, not .NET parity claims.
- The .NET numeric `QueryRange` overload is not counted separately because Rust A1 `CellReference` bounds express the same selection.
- Internal implementation classes and .NET-specific dependency-injection mechanics are excluded unless they expose observable behavior.
- CSV DataReader/DataTable and a dedicated CSV/XLSX converter are excluded because Rust iterators and composed query/save calls provide the native equivalents.
- Legacy binary Excel formats are excluded because this comparison found no current .NET V2 public provider that would make them a parity requirement.

Re-run this comparison whenever either pinned revision changes. New parity claims should be added to the shared contract only after both public adapters and focused tests cover the behavior.