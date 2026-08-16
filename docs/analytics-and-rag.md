# Streaming Analytics and RAG Export

[简体中文](analytics-and-rag.zh-CN.md)

MiniExcel Rust provides two related but deliberately separate workflows:

- **Analytics** evaluates a versioned query plan while XLSX rows stream past. It retains aggregate state, not source rows.
- **RAG export** emits addressed source evidence as JSONL chunks and records extraction provenance in a JSON manifest.

This boundary follows document-intelligence work on table reasoning, structure-aware chunking, retrieval-augmented generation, long-context cost, grounding, memory management, and privacy. MiniExcel prepares deterministic evidence; it does not embed an LLM, vector database, retriever, or formula engine.

## Grouped Analytics

`QueryPlan` is the canonical contract for Rust, CLI, WebAssembly, and Browser Lab. The current schema version is `miniexcel.query-plan/v1`.

```rust
use miniexcel::{
    AggregateOp, AggregateSpec, ComparisonOp, FilterExpr, HeaderMode,
    MiniExcel, QueryLiteral, QueryPlan, ReadOptions,
};

let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let plan = QueryPlan::new([
    AggregateSpec::count_all("rows"),
    AggregateSpec::column(AggregateOp::Sum, "Amount", "totalAmount"),
    AggregateSpec::column(AggregateOp::Average, "Amount", "averageAmount"),
])
.with_filter(FilterExpr::compare(
    "Status",
    ComparisonOp::Eq,
    QueryLiteral::String("Ready".to_owned()),
))
.with_group_by(["Category", "Region"])
.with_max_groups(10_000)
.with_limit(200);

let result = MiniExcel::analyze_with_options("book.xlsx", &options, &plan)?;
# Ok::<(), miniexcel::Error>(())
```

The equivalent JSON contract is:

```json
{
  "version": "miniexcel.query-plan/v1",
  "filter": {
    "kind": "compare",
    "column": "Status",
    "op": "eq",
    "value": { "type": "string", "value": "Ready" }
  },
  "groupBy": ["Category", "Region"],
  "aggregates": [
    { "op": "count", "column": null, "alias": "rows" },
    { "op": "sum", "column": "Amount", "alias": "totalAmount" }
  ],
  "maxGroups": 10000,
  "limit": 200,
  "evidenceRowsPerGroup": 5
}
```

### Supported expressions

- Boolean composition: `and`, `or`, and `not`.
- Comparisons: `eq`, `notEq`, `lt`, `le`, `gt`, `ge`, and case-sensitive `contains`.
- Empty checks: `isEmpty` and `isNotEmpty`.
- Grouping by zero or more columns.
- Aggregates: `count`, `sum`, `average`, `min`, and `max`.

`count(*)` counts matched rows. `count(column)` counts non-empty values. Empty values are ignored by value aggregates. Integer and floating-point values interoperate numerically; unrelated types are not coerced. A referenced Excel error, incompatible value, or integer sum overflow stops the query with worksheet, row, and column context.

Groups preserve first-seen workbook order. Global aggregation emits one result row even when no rows match. Grouped aggregation emits no rows when no rows match. The result limit truncates returned groups only; it does not reduce the aggregate state required while scanning.

### Memory contract

Path analytics consume the existing bounded row stream. Approximate memory is:

```text
O(shared strings + styles + parser buffers + row channel
  + distinct groups * aggregate state
  + distinct groups * bounded evidence rows)
```

`maxGroups` is therefore a required safety boundary, not a cosmetic result limit. A group that would exceed it produces a deterministic error. Version 1 does not spill groups to disk and does not claim constant-memory grouping.

`MiniExcel::analyze_bytes` uses the same evaluator without collecting source rows. Browser input still keeps the compressed XLSX byte array in WebAssembly memory because browser file uploads do not provide the path-owned ZIP stream used by native queries.

## RAG Evidence Export

RAG export uses `query_structured` semantics. It retains only cells explicitly represented in worksheet XML and preserves one-based row/column coordinates, A1 addresses, typed cached values, formula text, style IDs, and number formats.

```rust
use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let export_options = RagExportOptions::new()
    .with_chunk_rows(25)
    .with_max_rows(500);
let mut export = MiniExcel::export_rag("book.xlsx", &options, &export_options)?;

for chunk in export.by_ref() {
    let chunk = chunk?;
    println!("{} {}", chunk.chunk_id(), chunk.data_range());
}
println!("{}", export.manifest().source_sha256());
# Ok::<(), miniexcel::Error>(())
```

Each `miniexcel.rag-chunk/v1` JSONL record contains:

- A stable chunk ID derived from the workbook hash, sheet index, and data range.
- Sheet identity and exact data range.
- An addressed header row repeated as context when header mode is enabled.
- Sparse source rows and cells with typed values and cell-level provenance.
- Formula text separately from cached values, with `cachedOnly` calculation status.

The `miniexcel.rag-manifest/v1` JSON file records:

- Source name and SHA-256 content hash.
- Sheet name, index, and visibility.
- Selected start/end cells and header interpretation.
- Chunk and maximum-row policies.
- Emitted row/chunk counts, exact JSONL UTF-8 bytes, and a documented `bytes / 4` approximate token estimate.
- Truncation state and formula-calculation limitations.

Path exports implement `Iterator<Item = Result<RagChunk>>`; only one result chunk, repeated header context, and parser state need to be resident. `visit_rag_chunks_from_bytes` provides the equivalent callback form for browser and in-memory callers.

`RagChunk::write_markdown` writes an independent GFM table directly to an I/O sink. `RagManifest::write_markdown_stream_end` can append an optional completion marker after the iterator is exhausted. Markdown is appendable and LLM-readable; JSONL remains canonical for exact typed metadata. See [Streaming Markdown and anydoc comparison](markdown-streaming.md) for the format, memory boundary, and benchmark harness.

Hidden and very-hidden sheets are rejected by default. Call `with_allow_hidden_sheets(true)` only after an explicit privacy decision. Browser Lab exposes the same opt-in and performs all work locally in a Web Worker. It never sends workbook content to an external model.

## CLI

Run analytics from a plan file or stdin:

```bash
cargo +1.85.0 run -p miniexcel-cli -- \
  analyze book.xlsx --header --plan plan.json --format json

cat plan.json | cargo +1.85.0 run -p miniexcel-cli -- \
  analyze book.xlsx --header --plan - --format jsonl
```

Write JSONL chunks and a manifest incrementally, or write Markdown and JSONL during the same pass:

```bash
cargo +1.85.0 run -p miniexcel-cli -- \
  rag-export book.xlsx --header --chunk-rows 25 --max-rows 500 \
  --output-prefix ./out/book

cargo +1.85.0 run -p miniexcel-cli -- \
  rag-export book.xlsx --header --chunk-rows 25 --format both \
  --output-prefix ./out/book
```

`--format` accepts `jsonl`, `markdown`, or `both` and defaults to `jsonl`. The command publishes selected chunk files and `book.manifest.json` only after the stream and serialization complete. Use `--allow-hidden-sheets` for explicit hidden-sheet export.

## Scope limits

Version 1 intentionally excludes SQL text parsing, `HAVING`, `ORDER BY`, joins, windows, pivots, disk spill, vector indexing, embeddings, retrieval ranking, model calls, answer generation, formula recalculation, semantic table detection, merged-cell interpretation, and image/layout understanding. These require separate contracts and evaluation rather than being implied by row-to-JSON conversion.
