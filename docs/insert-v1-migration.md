# Migrating MiniExcel v1 Insert Workflows To Rust

[简体中文](insert-v1-migration.zh-CN.md)

This guide covers migration from MiniExcel .NET v1 `Insert`/`InsertSheet` workflows to the Rust existing-workbook APIs. It describes observable behavior, not an internal port of the .NET implementation.

## API Mapping

| .NET v1 workflow | Rust API |
| --- | --- |
| Insert dynamic rows into a path | `MiniExcel::insert()` |
| Insert a fallible or streaming row source | `MiniExcel::insert_with_schema()` |
| Insert Serde structs | `MiniExcel::insert_serialized()` |
| Insert from one borrowed stream into another | `insert*_from_reader_to_writer()` |
| Insert from an async row producer | `insert_with_schema_async*()` with the optional `async` feature |
| Overwrite an existing sheet | `InsertOptions::with_existing_sheet_policy(ExistingSheetPolicy::Replace)` |

Rust returns the number of data rows written. Missing paths create a new XLSX workbook. Existing `.xlsx` paths use package preflight, a sibling temporary file, validation, source-conflict detection, and atomic replacement.

## Deliberate Differences

### Worksheet Lookup

Rust worksheet-name matching is case-insensitive, following Excel worksheet identity semantics. The .NET v1 Insert path uses exact-case lookup in some flows. Do not rely on creating case-only duplicate worksheet names.

### Atomicity

Rust path Insert does not mutate the source ZIP in place. It holds an advisory path lock, rewrites into a sibling temporary file, validates the package, verifies the source SHA-256 fingerprint, and then atomically replaces the path. A concurrent Insert returns a deterministic conflict instead of silently losing an update.

The existing-path guarantee does not apply to separate borrowed output streams. Those APIs require an empty `Write + Seek` sink, do not truncate or roll back it, and cannot provide post-write validation. Source and destination must not alias the same stream.

Missing-path creation produces a new workbook but is not an edit of an existing source package.

### Replacement

Rust strict replacement preserves workbook order, sheet ID, relationship ID/path, visibility, and active state. The default relationship policy rejects worksheets that own relationships. `TargetRelationshipPolicy::RemoveSupported` may remove target-owned tables, drawings with exclusively owned images, comments, VML drawings, and external hyperlinks. Unknown, shared, pivot, and external-link structures are rejected or preserved conservatively.

Replacement removes a stale calculation chain and requests full recalculation on the next application open. Rust does not calculate formulas or rewrite formula references.

### Memory And Async Production

Explicit-schema rows are consumed once. Rows and worksheet XML are disk-spooled; shared-string conversion, style rebasing, and ZIP writing remain bounded-memory streams.

The optional `async` feature makes row production asynchronous through a bounded channel. ZIP, XML, validation, and filesystem operations remain blocking on a dedicated worker thread. Explicit cancellation waits for cleanup. Dropping the future requests cancellation, with cleanup completing in the background. Cancellation cannot revoke an atomic replacement that has already crossed the commit boundary.

### Unsupported Operations

Insert creates, appends, or strictly replaces complete worksheets. It does not append rows to an existing worksheet, edit macros, calculate formulas, clone an arbitrary selected sheet, or provide a general workbook editor. Separate atomic APIs rename, reorder, change visibility, and copy a complete source workbook while adding row data without becoming part of the in-place Insert operation. `.xlsm` packages and Strict OOXML packages are rejected.

## Validation

Insert behavior is covered by focused Rust integration tests rather than the shared query compatibility contract.

```powershell
cargo +1.85.0 test -p miniexcel --test insert --locked
```

Case-insensitive matching, byte-identical rejection, bounded resources, staged atomic replacement, relationship cleanup, and cancellation phase behavior remain in focused Rust tests.
