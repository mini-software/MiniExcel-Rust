# 流式 Markdown 与 anydoc 对比

[English](markdown-streaming.md)

MiniExcel 将 Markdown 输出为一系列彼此独立的 GitHub-Flavored Markdown 表格。每个 `RagChunk` 直接写入 `std::io::Write`；serializer 不会保留先前的 chunk，也不要求最终 JSON 结束分隔符。

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

stream 开头会输出源文件名和 SHA-256、worksheet 名称、从 1 开始的 worksheet 顺序、可见性、选定范围、header mode、chunk 限制、row 限制和公式计算策略。每个 chunk 包含开始标记、worksheet/range 标题、带地址的 GFM 表格和结束标记。启用 header mode 时，首个选中 row 会在每个 chunk 中作为表格上下文重复出现。来自 cell 的换行和 Markdown 控制字符会被转义，因此源文本无法创建额外 row、column、标题或 raw HTML。公式 cell 会同时显示缓存值和公式文本。

当 chunk 包含公式、非零 style ID 或非 `General` number format 时，还会附加 `Cell metadata` 表。每一行记录 A1 cell、value type、公式、源 style ID 和 number format。这些是源 metadata，而不是视觉重建：GFM 无法还原 Excel 的字体、填充、边框、颜色或对齐方式，MiniExcel 也不会在 Markdown 中把 style table 展开成这些属性。

最终的 `miniexcel:stream-end` 标记是可选的。它存在时，可以证明 stream 正常完成，并记录已发出的 row、chunk 和有意截断状态；缺少该标记不会使此前完整的 chunk 失效。JSONL 仍是精确 value type、style 和 number format 的规范机器表示；Markdown 是便于 LLM 阅读的配套格式。

## 内存边界

路径导出的内存受 parser 状态、workbook metadata、shared string、一个已配置 chunk 和单个异常大 row 的大小约束。每个 chunk 到达时就写出 Markdown 字节，因此保留内存不会随已发出 row 数增长。CLI 的 `--format both` 在同一次 worksheet 扫描中写入 JSONL 和 Markdown。

浏览器输入会在 WebAssembly 内存中保留压缩后的 XLSX。当前 Browser Lab 还会在构造 Blob 前物化完整下载字符串，因此它的输出端尚不是端到端常量内存文件流。解析和 chunk 生成仍在 Web Worker 中进行。

## anydoc 对比

[firecrawl/anydoc](https://github.com/firecrawl/anydoc) 的目标更广：将多种 office 格式转换成一致、紧凑的单个 Markdown 文档。对于 Excel，anydoc 通过 calamine 读取 workbook 字节，构建完整 document/table model，再渲染为一个 `String`。它会把 merged range 映射为 table span，在 GFM 中将被覆盖的位置留空，并根据有限的 body sample 启发式推断 header row。其 spreadsheet Markdown 不保留公式、Excel style ID、number format、workbook properties、comment、hyperlink 或精确视觉样式。MiniExcel 只支持 XLSX，但它会流式处理选中的 worksheet，并为 RAG 摄取保留 source/workbook/sheet、row/A1、formula、style ID 和 number format provenance。Header 通过 `HeaderMode` 明确指定；RAG Markdown 尚未表示 merged-cell span。

从仓库根目录运行 Windows 对比 harness：

```powershell
pwsh ./scripts/new-markdown-benchmark-xlsx.ps1 `
  -Output ./target/markdown-comparison/synthetic.xlsx -Rows 100000

pwsh ./scripts/compare-anydoc.ps1 `
  -Workbook ./target/markdown-comparison/synthetic.xlsx `
  -Iterations 5 -ChunkRows 500
```

生成器会先流式写入 worksheet XML，再将其打包，并支持最多一百万行。harness 会构建 release CLI，在所选输出目录下安装固定版本的 anydoc package，执行一次 warm-up，交替调整工具运行顺序，并记录 wall time、CPU time、进程 peak working set、输出 byte 数和基础 Markdown 结构计数。它会保存双方完整输出、`report.md` 和 `measurements.json` 以供检查。

要获得最接近的输出对比，应使用单 sheet workbook。评估伸缩性时，应使用结构相同但逐步增大的真实 workbook；小型 fixture 可以验证行为，但无法建立内存增长曲线。结果包含 CLI 启动和文件 I/O，因此不应直接与 anydoc 发布的进程内 warm timing 比较。