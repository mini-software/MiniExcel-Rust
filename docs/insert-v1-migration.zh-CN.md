# 将 MiniExcel v1 Insert 工作流迁移到 Rust

[English](insert-v1-migration.md)

本文说明如何把 MiniExcel .NET v1 的 `Insert`/`InsertSheet` 工作流迁移到 Rust 的现有工作簿 API。这里对齐的是可观察行为，不是逐字翻译 .NET 内部实现。

## API 映射

| .NET v1 工作流 | Rust API |
| --- | --- |
| 向路径插入动态 row | `MiniExcel::insert()` |
| 插入可失败或流式 row source | `MiniExcel::insert_with_schema()` |
| 插入 Serde struct | `MiniExcel::insert_serialized()` |
| 从一个 borrowed stream 插入到另一个 | `insert*_from_reader_to_writer()` |
| 从 async row producer 插入 | 启用可选 `async` feature 后使用 `insert_with_schema_async*()` |
| 覆盖现有 sheet | `InsertOptions::with_existing_sheet_policy(ExistingSheetPolicy::Replace)` |

Rust 返回写入的数据 row 数。路径不存在时创建新 XLSX workbook。已有 `.xlsx` path 会经过 package preflight、同目录临时文件写入、package validation、source conflict 检测和原子替换。

## 有意保留的差异

### Worksheet 查找

Rust 不区分 worksheet name 大小写，以符合 Excel worksheet identity 语义。.NET v1 的部分 Insert 流程使用精确大小写查找。不要依赖创建仅大小写不同的重复 worksheet name。

### 原子性

Rust path Insert 不会原位修改 source ZIP。它持有 advisory path lock，在同目录临时文件中重写，验证 package，校验 source SHA-256 fingerprint，最后原子替换 path。并发 Insert 会返回确定性 conflict，不会静默丢失更新。

已有路径的保证不适用于独立 borrowed output stream。这些 API 要求空的 `Write + Seek` sink，不会 truncate 或 rollback，也不能提供写后验证。Source 与 destination 不得指向同一个 stream。

缺失路径创建会生成新 workbook，但不属于对现有 source package 的编辑。

### Replacement

Rust strict replacement 会保留 workbook order、sheet ID、relationship ID/path、visibility 和 active state。默认 relationship policy 会拒绝拥有 relationship 的 worksheet。`TargetRelationshipPolicy::RemoveSupported` 可删除 target-owned table、含独占 image 的 drawing、comment、VML drawing 和 external hyperlink。Unknown、shared、pivot 与 external-link 结构会被拒绝或保守保留。

Replacement 会删除 stale calculation chain，并要求应用下次打开时执行完整重算。Rust 不计算公式，也不改写公式引用。

### 内存与 Async Producer

显式 schema row 只消费一次。Row 与 worksheet XML 会落盘 spool；shared-string conversion、style rebase 和 ZIP write 均为有界内存 stream。

可选 `async` feature 通过 bounded channel 让 row production 异步执行。ZIP、XML、validation 和 filesystem 操作仍在专用 blocking worker thread 中运行。显式 cancellation 会等待 cleanup；drop future 会请求 cancellation，cleanup 在后台完成。Atomic replacement 一旦越过 commit boundary，cancellation 不能撤销。

### 不支持的操作

Insert 创建、append 或严格 replace 完整 worksheet。它不会向现有 worksheet 追加 row，不编辑 macro，不计算公式，不复制任意 sheet，也不是通用 workbook editor。独立原子 API 可执行 worksheet rename、reorder 与 visibility 修改，但这些操作不属于 Insert。`.xlsm` package 与 Strict OOXML package 会被拒绝。

## 验证

Insert 行为由聚焦的 Rust integration test 覆盖，不扩展共享 query compatibility contract。

```powershell
cargo +1.85.0 test -p miniexcel --test insert --locked
```

大小写不敏感匹配、拒绝时 byte-identical、资源边界、staged atomic replacement、relationship cleanup 和 cancellation phase behavior 均保留在 Rust 聚焦测试中。
