# .NET v1 与 Rust Query 压力测试

[English](dotnet-v1-query-benchmark.md)

## 测试范围

本测试使用同一份工作簿，对比无表头动态 XLSX 流式读取：

- .NET MiniExcel v1：`MiniExcel.Query(path, useHeaderRow: false)`
- MiniExcel Rust：`MiniExcel::query(path)`

两个 runner 都会遍历所有返回行，但不会把完整 worksheet 保留在内存中。本测试不包含 Save、类型映射、公式及其他 API。

每个计量轮次都会启动新进程，每个进程完整读取工作簿三次。因此结果包含进程启动及 .NET JIT 成本，但这些成本会分摊到 300,000 行。正式计量前会另启进程预热操作系统文件缓存，五个计量轮次会交替两种 runtime 的运行顺序。峰值工作集约每 10 ms 采样一次。

## 测试环境

以下结果采集于 2026-08-26：

| 项目 | 值 |
| --- | --- |
| 操作系统 | Windows 10 Pro 10.0.19045，64 位 |
| 处理器 | AMD Ryzen 5 5600X 6-Core Processor，12 个逻辑处理器 |
| 工作簿 | 100,000 行 x 10 列，3,563,449 bytes |
| 工作簿 SHA-256 | `5F0997993785630C7307811387A1F6D1B07534D0A88D922B377A64E472583ED5` |
| 每进程读取次数 | 3 |
| 正式计量轮数 | 5 |
| .NET SDK | 10.0.103 |
| .NET MiniExcel | 1.46.1，commit `8b6feb87cfd00d0802de91bfca5616ec2dd744b7` |
| Rust toolchain | rustc 1.85.0（`4d91de4e4`，2025-02-17） |
| MiniExcel Rust 基准 revision | `36d2d27f8c078181ae46c383e47321dd3a8256bc` |

## 测试结果

每个计量进程中，两种实现都准确返回 300,000 行。

| Runtime | 耗时中位数 | 吞吐量 | 峰值工作集中位数 | 最大峰值工作集 |
| --- | ---: | ---: | ---: | ---: |
| .NET v1 | 3,369.14 ms | 89,043 行/秒 | 79.91 MB | 81.49 MB |
| Rust | 3,955.76 ms | 75,839 行/秒 | 9.86 MB | 9.91 MB |

| 轮次 | .NET v1 耗时 | .NET v1 峰值 | Rust 耗时 | Rust 峰值 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 3,513.19 ms | 81.49 MB | 3,948.10 ms | 9.79 MB |
| 2 | 3,294.73 ms | 79.91 MB | 4,139.42 ms | 9.84 MB |
| 3 | 3,249.22 ms | 79.73 MB | 3,939.59 ms | 9.90 MB |
| 4 | 3,369.14 ms | 79.99 MB | 3,962.12 ms | 9.91 MB |
| 5 | 3,513.73 ms | 79.85 MB | 3,955.76 ms | 9.86 MB |

在这个工作负载下，Rust 耗时比 .NET v1 多 17.4%，吞吐量低 14.8%；Rust 的峰值工作集中位数低 87.7%，为 9.86 MB 对 79.91 MB，约为 .NET v1 的八分之一。

这些数字只代表一份工作簿和一台机器，不是普遍性能保证。尤其需要注意，进程级计时包含 runtime 启动成本，采样工作集也包含 runtime 自身开销。针对具体应用做判断前，应使用有代表性的工作簿重新运行测试。

## 重现方法

将 Rust 与 .NET 仓库放在同级目录，然后从 Rust 仓库运行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

脚本会从本地 .NET Git 仓库读取 `v1.x-maintenance`，不会切换其当前 checkout。它会以 Release 模式构建两个 runner、校验读取行数一致、打印比较结果，并将完整机器可读报告写入 `target/benchmarks/dotnet-v1-vs-rust.json`。