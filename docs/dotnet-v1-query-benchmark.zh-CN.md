# .NET v1 与 Rust Query 压力测试

[English](dotnet-v1-query-benchmark.md)

## 测试范围

本测试使用同一份工作簿，对比无表头动态 XLSX 流式读取：

- .NET MiniExcel v1：`MiniExcel.Query(path, useHeaderRow: false)`
- MiniExcel Rust：`MiniExcel::query(path)`

两个 runner 都会遍历所有返回行，但不会把完整 worksheet 保留在内存中。本测试不包含 Save、类型映射、公式及其他 API。

## 公平性控制

- 两个 runner 都使用 Release 构建、同一份工作簿和语义等价的公开动态 Query API。
- 两端同时统计行数和单元格数；任一轮结果不一致，测试立即失败。
- 每个 runtime 会先运行一个不计时的独立进程，以预热操作系统文件缓存。
- Query 耗时由 runner 内部计量，不包含进程启动和结果序列化。
- 五个正式轮次各使用新进程，并交替两种 runtime 的运行顺序；汇总采用中位数。
- 不启用自定义 PGO、native CPU target、CPU affinity 或 runtime 调优。

测试分为两个场景，避免混淆首次调用和持续吞吐：

- **Cold：**进程内不预热，只计量一次 Query。它包含首次调用的库初始化和 .NET JIT，但不包含进程启动。由于外部 preflight 已预热操作系统缓存，因此这不是冷磁盘测试。
- **Steady：**每个进程先完整执行一次不计时的 Query，再计量三次 Query。.NET runner 会在 warm-up 后、正式计时前执行完整垃圾回收，避免把 warm-up 产生的垃圾计入正式 Query。该场景衡量 .NET JIT 和 runtime cache 就绪后的持续吞吐。

外层进程约每 10 ms 采样一次峰值工作集，因此内存数据包含 runtime 启动；Steady 场景还包含 warm-up pass。总进程耗时作为辅助指标保留，但不参与 Query 吞吐量计算。

## 测试环境

以下结果采集于 2026-08-26：

| 项目 | 值 |
| --- | --- |
| 操作系统 | Windows 10 Pro 10.0.19045，64 位 |
| 处理器 | AMD Ryzen 5 5600X 6-Core Processor，12 个逻辑处理器 |
| 工作簿 | 100,000 行 x 10 列，3,563,449 bytes |
| 工作簿 SHA-256 | `5F0997993785630C7307811387A1F6D1B07534D0A88D922B377A64E472583ED5` |
| Cold 场景 | 0 次 warm-up，1 次正式读取 |
| Steady 场景 | 1 次 warm-up，3 次正式读取 |
| 正式计量轮数 | 5 |
| .NET SDK | 10.0.103 |
| .NET MiniExcel | 1.46.1，commit `8b6feb87cfd00d0802de91bfca5616ec2dd744b7` |
| Rust toolchain | rustc 1.85.0（`4d91de4e4`，2025-02-17） |
| MiniExcel Rust | 包含本文档的源码树；优化基线为 `808828f3ce892dad8b00bda1cee370fae6451e1c` |

## 汇总结果

每个计量进程中，两种实现每次读取都准确返回 100,000 行、1,000,000 个单元格。

| 场景 | Runtime | Query 耗时中位数 | 吞吐量 | 总进程耗时中位数 | 峰值工作集中位数 | 最大峰值工作集 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | .NET v1 | 1,731.33 ms | 57,759 行/秒 | 1,785.67 ms | 66.81 MB | 67.45 MB |
| Cold | Rust | 660.42 ms | 151,419 行/秒 | 666.39 ms | 7.23 MB | 7.32 MB |
| Steady | .NET v1 | 2,405.57 ms | 124,711 行/秒 | 4,119.75 ms | 79.58 MB | 81.49 MB |
| Steady | Rust | 2,051.91 ms | 146,205 行/秒 | 2,749.84 ms | 7.36 MB | 8.74 MB |

对于新进程中的首次 Query，Rust 吞吐量为 .NET v1 的 2.62 倍，Query 耗时少 61.9%，峰值工作集少 89.2%。

进程内 warm-up 后，Rust 吞吐量为 .NET v1 的 1.17 倍，Query 耗时少 14.7%；峰值工作集中位数低 90.8%，为 7.36 MB 对 79.58 MB。

与 Rust 优化基线相比，Cold 场景的 Query 耗时中位数下降 54.6%，Steady 场景下降 51.3%，峰值工作集保持在 9 MB 以下。

## Cold 逐轮结果

| 轮次 | .NET v1 耗时 | .NET v1 峰值 | Rust 耗时 | Rust 峰值 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1,697.58 ms | 66.09 MB | 670.55 ms | 6.59 MB |
| 2 | 1,737.33 ms | 66.81 MB | 646.70 ms | 7.30 MB |
| 3 | 1,626.67 ms | 66.16 MB | 660.42 ms | 7.32 MB |
| 4 | 1,753.76 ms | 67.45 MB | 674.44 ms | 7.04 MB |
| 5 | 1,731.33 ms | 67.40 MB | 659.06 ms | 7.23 MB |

## Steady 逐轮结果

以下每个计时值都包含一次不计时 warm-up 后的三次完整 Query。

| 轮次 | .NET v1 耗时 | .NET v1 峰值 | Rust 耗时 | Rust 峰值 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,405.57 ms | 79.58 MB | 2,051.91 ms | 7.13 MB |
| 2 | 2,420.05 ms | 79.01 MB | 1,977.10 ms | 8.74 MB |
| 3 | 2,394.17 ms | 78.71 MB | 2,053.29 ms | 7.39 MB |
| 4 | 2,389.88 ms | 81.49 MB | 2,014.16 ms | 7.27 MB |
| 5 | 2,462.29 ms | 80.27 MB | 2,109.63 ms | 7.36 MB |

## 结果解释

测试工作簿包含 1,000,000 个 shared-string 单元格、100,000 个唯一字符串，且没有 merged range；worksheet XML 解压后为 39,167,847 bytes。

Rust 现在会在不需要 merged-cell 填充时使用有效 `<dimension>` 作为 Query 范围；显式指定结束单元格时还会跳过初步范围读取。解析器以单次借用扫描读取高频 row/cell 属性，无临时 `String` 分配地解析 shared-string 索引，按所选列宽进行有上限的行存储预分配，并将中间 `Data::String` 的所有权直接移动到公开 `CellValue::String`。

内存型 shared strings 现在使用一个连续 UTF-8 缓冲区和结束偏移表，不再为每个唯一字符串分别进行堆分配。偏移表根据受文件大小约束的 `uniqueCount` 预分配；不含 Excel `_xHHHH_` 转义的字符串会直接复用原有分配。与上一轮优化结果相比，Cold 和 Steady 的峰值工作集中位数分别再下降 25.0% 和 25.5%，Query 耗时中位数则分别再下降 2.1% 和 1.3%。

剩余的逐行工作包括克隆动态列名，以及通过有界同步 channel 传递已解析行。这些成本维持了当前 `IndexMap<String, CellValue>` API、有界内存和 iterator 所有权语义。在该工作簿上，优化后的 Rust 在首次调用延迟和预热后的持续吞吐两方面均领先 .NET v1，同时保持明显更低的峰值内存。

这些数字只代表一份工作簿和一台机器，不是普遍性能保证。测试没有固定 CPU affinity，也无法消除所有操作系统噪声；使用中位数可以减弱异常值影响。针对具体应用做判断前，应使用有代表性的工作簿重新运行测试。

## 重现方法

将 Rust 与 .NET 仓库放在同级目录，然后从 Rust 仓库运行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

脚本会从本地 .NET Git 仓库读取 `v1.x-maintenance`，不会切换其当前 checkout。它会以 Release 模式构建两个 runner、校验行数与单元格数一致、打印两个场景，并将完整机器可读报告写入 `target/benchmarks/dotnet-v1-vs-rust.json`。

使用 `-Scenario Cold` 或 `-Scenario Steady` 可只运行一个场景。`-Passes` 与 `-WarmupPasses` 用于配置 Steady 场景，`-Iterations` 控制正式计量的新进程数量。