# 流式分析与 RAG 导出

[English](analytics-and-rag.md)

MiniExcel Rust 提供两个相关但有意分离的工作流：

- **分析**在 XLSX 行流经时执行版本化 query plan。它保留聚合状态，而不是源数据行。
- **RAG 导出**以 JSONL chunk 发出带地址的源证据，并在 JSON manifest 中记录提取来源。

这一边界遵循文档智能领域在表格推理、结构感知分块、检索增强生成、长上下文成本、证据落地、内存管理和隐私方面的研究。MiniExcel 负责准备确定性证据；它不内嵌 LLM、向量数据库、retriever 或公式引擎。

## 分组分析

`QueryPlan` 是 Rust、CLI、WebAssembly 和 Browser Lab 的规范契约。当前 schema 版本为 `miniexcel.query-plan/v1`。

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

等价的 JSON 契约如下：

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

### 支持的表达式

- 布尔组合：`and`、`or` 和 `not`。
- 比较：`eq`、`notEq`、`lt`、`le`、`gt`、`ge`，以及区分大小写的 `contains`。
- 空值检查：`isEmpty` 和 `isNotEmpty`。
- 按零个或多个 column 分组。
- 聚合：`count`、`sum`、`average`、`min` 和 `max`。

`count(*)` 统计匹配行数。`count(column)` 统计非空值数量。值聚合会忽略空值。整数和浮点数可进行数值互操作；无关类型不会被强制转换。遇到被引用的 Excel error、不兼容值或整数求和溢出时，query 会停止并报告 worksheet、row 和 column 上下文。

group 保持其在 workbook 中首次出现的顺序。即使没有行匹配，全局聚合也会输出一个结果行；没有行匹配时，分组聚合不输出任何行。结果 limit 只截断返回的 group，不会减少扫描期间所需的聚合状态。

### 内存契约

基于路径的分析使用现有有界 row stream。近似内存为：

```text
O(shared strings + styles + parser buffers + row channel
  + distinct groups * aggregate state
  + distinct groups * bounded evidence rows)
```

因此，`maxGroups` 是必要的安全边界，而不是装饰性的结果 limit。任何会超过它的新 group 都会产生确定性错误。版本 1 不会将 group spill 到磁盘，也不声称分组使用常量内存。

`MiniExcel::analyze_bytes` 使用相同 evaluator，且不会收集源数据行。浏览器文件上传无法提供原生 query 所用、由路径拥有的 ZIP stream，因此浏览器输入仍会在 WebAssembly 内存中保留压缩后的 XLSX 字节数组。

## RAG 证据导出

RAG 导出采用 `query_structured` 语义。它只保留 worksheet XML 中明确表示的 cell，并保留从 1 开始的 row/column 坐标、A1 地址、带类型的缓存值、公式文本、style ID 和 number format。

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

每条 `miniexcel.rag-chunk/v1` JSONL 记录包含：

- 由 workbook hash、sheet index 和 data range 推导出的稳定 chunk ID。
- sheet 身份和精确 data range。
- 启用 header mode 时，作为上下文重复出现且带地址的 header row。
- 带类型值和 cell 级 provenance 的稀疏源 row 与 cell。
- 与缓存值分离的公式文本，以及 `cachedOnly` 计算状态。

`miniexcel.rag-manifest/v1` JSON 文件记录：

- 源名称和 SHA-256 内容 hash。
- sheet 名称、index 和 visibility。
- 所选 start/end cell 和 header 解释方式。
- chunk 和最大 row 策略。
- 已发出的 row/chunk 数量、精确 JSONL UTF-8 byte 数，以及有明确说明的 `bytes / 4` 近似 token 估计。
- 截断状态和公式计算限制。

路径导出实现 `Iterator<Item = Result<RagChunk>>`；内存中只需保留一个结果 chunk、重复的 header 上下文和 parser 状态。`visit_rag_chunks_from_bytes` 为浏览器和内存调用方提供等价的 callback 形式。

`RagManifest::write_markdown_stream_start` 会在迭代前写入 source、worksheet、range 和 export provenance。随后，`RagChunk::write_markdown` 将独立 GFM 表格以及有界的 formula/style/number-format metadata 直接写入 I/O sink。迭代器耗尽后，`RagManifest::write_markdown_stream_end` 可以附加可选完成标记。Markdown 可追加且便于 LLM 阅读；JSONL 仍是精确类型化 metadata 的规范表示。格式、内存边界和基准 harness 见[流式 Markdown 与 anydoc 对比](markdown-streaming.zh-CN.md)。

hidden 和 very-hidden sheet 默认会被拒绝。只有在作出明确隐私决定后，才调用 `with_allow_hidden_sheets(true)`。Browser Lab 提供相同的 opt-in，并在 Web Worker 中本地完成所有工作。它绝不会将 workbook 内容发送给外部模型。

## CLI

从 plan 文件或 stdin 运行分析：

```bash
cargo +1.85.0 run -p miniexcel-cli -- \
  analyze book.xlsx --header --plan plan.json --format json

cat plan.json | cargo +1.85.0 run -p miniexcel-cli -- \
  analyze book.xlsx --header --plan - --format jsonl
```

增量写入 JSONL chunk 和 manifest，或在同一次扫描中写入 Markdown 与 JSONL：

```bash
cargo +1.85.0 run -p miniexcel-cli -- \
  rag-export book.xlsx --header --chunk-rows 25 --max-rows 500 \
  --output-prefix ./out/book

cargo +1.85.0 run -p miniexcel-cli -- \
  rag-export book.xlsx --header --chunk-rows 25 --format both \
  --output-prefix ./out/book
```

`--format` 接受 `jsonl`、`markdown` 或 `both`，默认值为 `jsonl`。只有 stream 和序列化全部完成后，命令才会发布所选 chunk 文件和 `book.manifest.json`。要导出 hidden sheet，使用 `--allow-hidden-sheets` 明确授权。

## 范围限制

版本 1 有意排除 SQL 文本解析、`HAVING`、`ORDER BY`、join、window、pivot、磁盘 spill、向量索引、embedding、检索排名、模型调用、答案生成、公式重算、语义表格检测、merged-cell 解释以及图像/布局理解。这些能力需要独立的契约与评估，不能因为已有 row-to-JSON 转换就暗示它们受到支持。