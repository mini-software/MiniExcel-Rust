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
| MiniExcel Rust 基准 revision | `808828f3ce892dad8b00bda1cee370fae6451e1c` |

## 汇总结果

每个计量进程中，两种实现每次读取都准确返回 100,000 行、1,000,000 个单元格。

| 场景 | Runtime | Query 耗时中位数 | 吞吐量 | 总进程耗时中位数 | 峰值工作集中位数 | 最大峰值工作集 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | .NET v1 | 1,890.76 ms | 52,889 行/秒 | 1,963.50 ms | 67.22 MB | 68.12 MB |
| Cold | Rust | 1,455.79 ms | 68,691 行/秒 | 1,473.66 ms | 9.71 MB | 9.77 MB |
| Steady | .NET v1 | 2,585.18 ms | 116,046 行/秒 | 4,499.20 ms | 78.75 MB | 82.07 MB |
| Steady | Rust | 4,209.62 ms | 71,265 行/秒 | 5,728.73 ms | 9.91 MB | 9.94 MB |

对于新进程中的首次 Query，Rust 吞吐量为 .NET v1 的 1.30 倍，Query 耗时少 23.0%，峰值工作集少 85.6%。

进程内 warm-up 后，Rust 吞吐量为 .NET v1 的 0.61 倍，Query 耗时多 62.8%；峰值工作集中位数仍低 87.4%，为 9.91 MB 对 78.75 MB。

因此结论取决于工作负载：在本次测试中，Rust 的首次调用延迟更低且内存明显更少；.NET v1 在 JIT 预热后的持续吞吐更高。

## Cold 逐轮结果

| 轮次 | .NET v1 耗时 | .NET v1 峰值 | Rust 耗时 | Rust 峰值 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,063.54 ms | 65.97 MB | 1,571.51 ms | 9.66 MB |
| 2 | 1,950.17 ms | 67.22 MB | 1,397.59 ms | 9.75 MB |
| 3 | 1,890.76 ms | 68.12 MB | 1,461.67 ms | 9.77 MB |
| 4 | 1,816.06 ms | 67.63 MB | 1,394.83 ms | 9.71 MB |
| 5 | 1,793.67 ms | 66.70 MB | 1,455.79 ms | 9.67 MB |

## Steady 逐轮结果

以下每个计时值都包含一次不计时 warm-up 后的三次完整 Query。

| 轮次 | .NET v1 耗时 | .NET v1 峰值 | Rust 耗时 | Rust 峰值 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,401.10 ms | 79.22 MB | 4,209.62 ms | 9.92 MB |
| 2 | 2,602.98 ms | 78.44 MB | 4,128.13 ms | 9.91 MB |
| 3 | 2,554.63 ms | 82.07 MB | 4,357.14 ms | 9.89 MB |
| 4 | 2,766.50 ms | 78.75 MB | 4,205.49 ms | 9.94 MB |
| 5 | 2,585.18 ms | 78.27 MB | 4,254.68 ms | 9.80 MB |

## 结果解释

测试工作簿包含 1,000,000 个 shared-string 单元格、100,000 个唯一字符串，且没有 merged range；worksheet XML 解压后为 39,167,847 bytes。

Rust 当前会在正式输出前完整扫描一次 worksheet，即使文件包含有效 `<dimension>` 且未启用 merged-cell 填充。读取 shared string 时，它还会先将值克隆为中间 `Data::String`，再克隆为公开 `CellValue::String`；动态列名也会逐行克隆，并且每行通过有界同步 channel 传递。这些设计保持了有界内存和 iterator 所有权安全，但增加了稳态工作量。

.NET v1 在读取 `<dimension ref="A1:J100000">` 后即可结束范围预检查。首次 Query 的 JIT 成本使它在 Cold 场景较慢，但后续 Query 会受益于已编译及动态优化的热点路径。因此 Rust 在 Cold 场景领先，而 .NET v1 在 Steady 场景领先。

这些数字只代表一份工作簿和一台机器，不是普遍性能保证。测试没有固定 CPU affinity，也无法消除所有操作系统噪声；使用中位数可以减弱异常值影响。针对具体应用做判断前，应使用有代表性的工作簿重新运行测试。

## 重现方法

将 Rust 与 .NET 仓库放在同级目录，然后从 Rust 仓库运行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

脚本会从本地 .NET Git 仓库读取 `v1.x-maintenance`，不会切换其当前 checkout。它会以 Release 模式构建两个 runner、校验行数与单元格数一致、打印两个场景，并将完整机器可读报告写入 `target/benchmarks/dotnet-v1-vs-rust.json`。

使用 `-Scenario Cold` 或 `-Scenario Steady` 可只运行一个场景。`-Passes` 与 `-WarmupPasses` 用于配置 Steady 场景，`-Iterations` 控制正式计量的新进程数量。