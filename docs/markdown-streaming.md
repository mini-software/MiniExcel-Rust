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
for chunk in export.by_ref() {
    chunk?.write_markdown(&mut output)?;
}
export.manifest().write_markdown_stream_end(&mut output)?;
# Ok::<(), miniexcel::Error>(())
```

A chunk contains a start marker, a worksheet/range heading, an addressed GFM
table, and an end marker. The first selected row is repeated as table context
when header mode is enabled. Newlines and Markdown control characters from
cells are escaped so source text cannot create rows, columns, headings, or raw
HTML. Formula cells show both the cached value and formula text.

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
document/table model, then renders one `String`. MiniExcel supports XLSX only
but streams a selected worksheet and preserves row/A1/formula provenance for
RAG ingestion.

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