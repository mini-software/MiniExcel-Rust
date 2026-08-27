<div align="center">

# MiniExcel for Rust

[English](README.md) | [繁體中文](README.zh-TW.md) | [Français](README.fr.md) | [日本語](README.ja.md) | [Español](README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![下载量](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![文档](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![许可证](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

**快速、低内存的 XLSX 与 CSV 处理。**

</div>

---

<div align="center">

本项目属于 [MiniExcel](https://github.com/mini-software/MiniExcel) 项目家族，并以 .NET 版本作为兼容性参考。

</div>

---

<div align="center">

**[打开 Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)**，可在浏览器本地检查或生成 XLSX；工作簿数据不会离开浏览器。

</div>

---

## 简介

MiniExcel for Rust 是支持有界内存流、Serde、数据分析与 RAG 导出的 XLSX/CSV 读写库。

## 安装

```bash
cargo add miniexcel
```

最低支持 Rust 1.85.0。

## 快速开始

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    println!("{:?}", row?["A"]);
}
```

```rust
use miniexcel::{CellValue, DynamicRow, MiniExcel};

let mut row = DynamicRow::new();
row.insert("Name".into(), CellValue::String("MiniExcel".into()));
MiniExcel::save_as("book.xlsx", &[row])?;
```

## 主要能力

- 动态、类型化、结构化、Table 与 CSV 有界内存查询。
- 支持路径、字节数组及借用 reader/writer。
- Serde 读写、日期时间 helper 和精确 cell mapping。
- 多工作表创建、格式选项和工作表可见性。
- 原子新增/替换、重命名、排序、复制工作表及修改可见性。
- 模板渲染、条件/分组块和 marker 驱动的单元格合并。
- 带显式限制的流式分组分析。
- 面向 LLM/RAG 的来源地址 JSONL 与 Markdown 导出。
- 可选的 runtime-neutral async stream；ZIP/XML/文件系统操作仍为 blocking。

## 关键语义

- 路径查询流式读取 worksheet XML，不保留全部行。
- 默认工作表是第一张 worksheet，而不是 active tab。
- 普通读取返回公式缓存值；结构化读取还提供公式文本和格式。MiniExcel 不计算公式。
- Save 创建新 workbook，默认拒绝已有路径；Insert API 验证后原子修改 `.xlsx`。
- 大型 shared-string table 可写入索引临时文件；byte/WASM 查询保留在内存中。
- 暂不支持 `.xls`、`.xlsb`、`.ods`、宏、图片创建、公式计算和通用样式系统。

参见[兼容性矩阵](docs/compatibility.zh-CN.md)、[分析与 RAG 契约](docs/analytics-and-rag.zh-CN.md)和 [Insert 迁移指南](docs/insert-v1-migration.zh-CN.md)。

## Rust 与 .NET 性能对比

将本仓库与 [.NET MiniExcel](https://github.com/mini-software/MiniExcel) 放在同级目录后运行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

报告写入 `target/benchmarks/dotnet-v1-vs-rust.json`。只比较同一台机器产生的结果；详见[测试方法](docs/dotnet-v1-query-benchmark.zh-CN.md)。
