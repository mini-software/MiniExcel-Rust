# Streaming Markdown and anydoc comparison

[简体中文](markdown-streaming.zh-CN.md)

MiniExcel emits Markdown as a sequence of independent GitHub-Flavored Markdown
tables. Each `RagChunk` writes directly to `std::io::Write`; the serializer does
not retain earlier chunks or require a final JSON delimiter.

```rust
use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let mut export = MiniExcel::export_rag("book.xlsx", &options, &RagExportOptions::new())?;
let stdout = std::io::stdout();
let mut output = stdout.lock();
export.manifest().write_markdown_stream_start(&mut output)?;
for chunk in export.by_ref() {
    chunk?.write_markdown(&mut output)?;
}
export.manifest().write_markdown_stream_end(&mut output)?;
# Ok::<(), miniexcel::Error>(())
```

A stream starts with source file name and SHA-256, worksheet name, one-based
worksheet order, visibility, selected range, header mode, chunk limit, row
limit, and formula-calculation policy. A chunk contains a start marker, a
worksheet/range heading, an addressed GFM table, and an end marker. The first
selected row is repeated as table context when header mode is enabled. Newlines
and Markdown control characters from cells are escaped so source text cannot
create rows, columns, headings, or raw HTML. Formula cells show both the cached
value and formula text.

Each chunk also includes a `Cell metadata` table when it contains formulas,
nonzero style IDs, or non-`General` number formats. Rows identify the A1 cell,
typed value category, formula, source style ID, and number format. This is
source metadata rather than a visual reconstruction: GFM cannot reproduce
Excel fonts, fills, borders, colors, or alignment, and MiniExcel does not expand
the style table into those properties in Markdown.

The final `miniexcel:stream-end` marker is optional. Its presence proves normal
completion and records emitted rows, chunks, and deliberate truncation. Its
absence does not invalidate complete preceding chunks. JSONL remains the
canonical machine representation for exact value types, styles, and number
formats; Markdown is the LLM-readable companion.

## Memory boundary

Path export memory is bounded by parser state, workbook metadata, shared
strings, one configured chunk, and one unusually large row. Markdown bytes are
written as each chunk arrives, so retained memory does not grow with emitted
row count. The CLI `--format both` writes JSONL and Markdown during the same
worksheet pass.

Browser input retains the compressed XLSX in WebAssembly memory. The current
Browser Lab also materializes completed download strings before constructing a
Blob, so its output side is not yet an end-to-end constant-memory file stream.
Parsing and chunk production still occur in a Web Worker.

## anydoc comparison

[firecrawl/anydoc](https://github.com/firecrawl/anydoc) has a broader goal: it
converts many office formats into one consistent, compact Markdown document.
For Excel, anydoc reads workbook bytes through calamine, constructs a complete
document/table model, then renders one `String`. It maps merged ranges into
table spans, leaves covered GFM positions blank, and heuristically infers header
rows from a bounded body sample. Its spreadsheet Markdown does not preserve
formulas, Excel style IDs, number formats, workbook properties, comments,
hyperlinks, or exact visual styling. MiniExcel supports XLSX only but streams a
selected worksheet and preserves source/workbook/sheet, row/A1, formula, style
ID, and number-format provenance for RAG ingestion. Header handling is explicit
through `HeaderMode`; merged-cell spans are not yet represented in RAG
Markdown.

Run the Windows comparison harness from the repository root:

```powershell
pwsh ./scripts/new-markdown-benchmark-xlsx.ps1 `
  -Output ./target/markdown-comparison/synthetic.xlsx -Rows 100000

pwsh ./scripts/compare-anydoc.ps1 `
  -Workbook ./target/markdown-comparison/synthetic.xlsx `
  -Iterations 5 -ChunkRows 500
```

The generator streams worksheet XML before packaging it and supports up to one
million rows. The harness builds the release CLI, installs a pinned anydoc package under the
chosen output directory, performs one warm-up, alternates tool order, and
records wall time, CPU time, peak process working set, output bytes, and basic
Markdown structure counts. It saves both complete outputs, `report.md`, and
`measurements.json` for inspection.

Use a one-sheet workbook for the closest output comparison. Use progressively
larger real workbooks with equivalent shape to assess scaling; tiny fixtures
validate behavior but cannot establish a memory-growth curve. Results include
CLI startup and file I/O and should not be compared directly with anydoc's
published in-process warm timing.