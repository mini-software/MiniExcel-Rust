# .NET v1 And Rust Query Benchmark

[简体中文](dotnet-v1-query-benchmark.zh-CN.md)

## Scope

This benchmark compares dynamic, headerless XLSX streaming over the same workbook:

- .NET MiniExcel v1: `MiniExcel.Query(path, useHeaderRow: false)`
- MiniExcel Rust: `MiniExcel::query(path)`

Both runners enumerate every returned row without retaining the complete worksheet. Save performance, typed mapping, formulas, and other APIs are outside this comparison.

The harness starts a fresh process for each measured iteration. Each process reads the workbook three times, so process startup and .NET JIT are included but amortized across 300,000 rows. A separate warm-up process runs first to populate operating-system file caches. The five measured iterations alternate runtime order. Peak working set is sampled approximately every 10 ms.

## Environment

The following result was captured on 2026-08-26:

| Item | Value |
| --- | --- |
| Operating system | Windows 10 Pro 10.0.19045, 64-bit |
| Processor | AMD Ryzen 5 5600X 6-Core Processor, 12 logical processors |
| Workbook | 100,000 rows x 10 columns, 3,563,449 bytes |
| Workbook SHA-256 | `5F0997993785630C7307811387A1F6D1B07534D0A88D922B377A64E472583ED5` |
| Passes per process | 3 |
| Measured iterations | 5 |
| .NET SDK | 10.0.103 |
| .NET MiniExcel | 1.46.1, commit `8b6feb87cfd00d0802de91bfca5616ec2dd744b7` |
| Rust toolchain | rustc 1.85.0 (`4d91de4e4`, 2025-02-17) |
| MiniExcel Rust base revision | `36d2d27f8c078181ae46c383e47321dd3a8256bc` |

## Results

Both implementations returned exactly 300,000 rows in every measured process.

| Runtime | Median elapsed | Throughput | Median peak working set | Maximum peak working set |
| --- | ---: | ---: | ---: | ---: |
| .NET v1 | 3,369.14 ms | 89,043 rows/s | 79.91 MB | 81.49 MB |
| Rust | 3,955.76 ms | 75,839 rows/s | 9.86 MB | 9.91 MB |

| Iteration | .NET v1 elapsed | .NET v1 peak | Rust elapsed | Rust peak |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 3,513.19 ms | 81.49 MB | 3,948.10 ms | 9.79 MB |
| 2 | 3,294.73 ms | 79.91 MB | 4,139.42 ms | 9.84 MB |
| 3 | 3,249.22 ms | 79.73 MB | 3,939.59 ms | 9.90 MB |
| 4 | 3,369.14 ms | 79.99 MB | 3,962.12 ms | 9.91 MB |
| 5 | 3,513.73 ms | 79.85 MB | 3,955.76 ms | 9.86 MB |

For this workload, Rust took 17.4% longer and delivered 14.8% lower throughput than .NET v1. Its median peak working set was 87.7% lower: 9.86 MB versus 79.91 MB, or about one eighth of the .NET v1 footprint.

These numbers describe one workbook and one machine, not a general performance guarantee. In particular, the process-level timing includes runtime startup, and the sampled working set includes runtime overhead. Use the harness on representative workbooks before drawing application-specific conclusions.

## Reproduce

Keep the Rust and .NET repositories in sibling directories, then run from the Rust repository:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

The script reads `v1.x-maintenance` from the local .NET Git repository without changing its checkout. It builds both runners in Release mode, verifies matching row counts, prints the comparison, and writes the full machine-readable report to `target/benchmarks/dotnet-v1-vs-rust.json`.