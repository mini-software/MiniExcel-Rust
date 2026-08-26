# .NET v1 And Rust Query Benchmark

[简体中文](dotnet-v1-query-benchmark.zh-CN.md)

## Scope

This benchmark compares dynamic, headerless XLSX streaming over the same workbook:

- .NET MiniExcel v1: `MiniExcel.Query(path, useHeaderRow: false)`
- MiniExcel Rust: `MiniExcel::query(path)`

Both runners enumerate every returned row without retaining the complete worksheet. Save performance, typed mapping, formulas, and other APIs are outside this comparison.

## Fairness Controls

- Both runners use Release builds, the same workbook, and equivalent public dynamic Query APIs.
- Both runners count rows and cells. A result is rejected unless both counts match in every iteration.
- An untimed preflight process for each runtime populates operating-system file caches.
- Query time is measured inside each runner, excluding process launch and result serialization.
- Five measured iterations use fresh processes and alternate runtime order. The median is reported.
- No custom PGO, native CPU target, CPU affinity, or runtime tuning is applied.

Two scenarios keep startup and sustained throughput separate:

- **Cold:** one measured Query with no in-process warm-up. It includes first-call library initialization and .NET JIT, but not process launch. This is not a cold-disk test because the preflight has warmed the operating-system cache.
- **Steady:** one complete Query warm-up inside each process, followed by three measured Queries. The warm-up is outside Query timing. The .NET runner performs a full garbage collection after warm-up and before timing so warm-up garbage is not charged to the measured queries. This measures sustained in-process throughput after .NET JIT and runtime caches are warm.

Peak working set is sampled approximately every 10 ms over the whole process. It therefore includes runtime startup and, for the steady scenario, the warm-up pass. Total process elapsed time is retained as a secondary metric but is not used to calculate Query throughput.

## Environment

The following result was captured on 2026-08-26:

| Item | Value |
| --- | --- |
| Operating system | Windows 10 Pro 10.0.19045, 64-bit |
| Processor | AMD Ryzen 5 5600X 6-Core Processor, 12 logical processors |
| Workbook | 100,000 rows x 10 columns, 3,563,449 bytes |
| Workbook SHA-256 | `5F0997993785630C7307811387A1F6D1B07534D0A88D922B377A64E472583ED5` |
| Cold scenario | 0 warm-up passes, 1 measured pass |
| Steady scenario | 1 warm-up pass, 3 measured passes |
| Measured iterations | 5 |
| .NET SDK | 10.0.103 |
| .NET MiniExcel | 1.46.1, commit `8b6feb87cfd00d0802de91bfca5616ec2dd744b7` |
| Rust toolchain | rustc 1.85.0 (`4d91de4e4`, 2025-02-17) |
| MiniExcel Rust base revision | `808828f3ce892dad8b00bda1cee370fae6451e1c` |

## Summary

Both implementations returned exactly 100,000 rows and 1,000,000 cells per pass in every measured process.

| Scenario | Runtime | Median Query time | Throughput | Median process time | Median peak working set | Maximum peak working set |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | .NET v1 | 1,890.76 ms | 52,889 rows/s | 1,963.50 ms | 67.22 MB | 68.12 MB |
| Cold | Rust | 1,455.79 ms | 68,691 rows/s | 1,473.66 ms | 9.71 MB | 9.77 MB |
| Steady | .NET v1 | 2,585.18 ms | 116,046 rows/s | 4,499.20 ms | 78.75 MB | 82.07 MB |
| Steady | Rust | 4,209.62 ms | 71,265 rows/s | 5,728.73 ms | 9.91 MB | 9.94 MB |

For the first Query in a fresh process, Rust delivered 1.30x the .NET v1 throughput, completed in 23.0% less Query time, and used 85.6% less peak working set.

After an in-process warm-up, Rust delivered 0.61x the .NET v1 throughput and took 62.8% more Query time. Its median peak working set remained 87.4% lower, at 9.91 MB versus 78.75 MB.

The result is therefore workload-dependent: Rust has lower first-call latency and substantially lower memory use here, while .NET v1 has higher sustained throughput after JIT warm-up.

## Cold Results

| Iteration | .NET v1 elapsed | .NET v1 peak | Rust elapsed | Rust peak |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,063.54 ms | 65.97 MB | 1,571.51 ms | 9.66 MB |
| 2 | 1,950.17 ms | 67.22 MB | 1,397.59 ms | 9.75 MB |
| 3 | 1,890.76 ms | 68.12 MB | 1,461.67 ms | 9.77 MB |
| 4 | 1,816.06 ms | 67.63 MB | 1,394.83 ms | 9.71 MB |
| 5 | 1,793.67 ms | 66.70 MB | 1,455.79 ms | 9.67 MB |

## Steady Results

Each timed value below covers three complete Query passes after one untimed warm-up pass.

| Iteration | .NET v1 elapsed | .NET v1 peak | Rust elapsed | Rust peak |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,401.10 ms | 79.22 MB | 4,209.62 ms | 9.92 MB |
| 2 | 2,602.98 ms | 78.44 MB | 4,128.13 ms | 9.91 MB |
| 3 | 2,554.63 ms | 82.07 MB | 4,357.14 ms | 9.89 MB |
| 4 | 2,766.50 ms | 78.75 MB | 4,205.49 ms | 9.94 MB |
| 5 | 2,585.18 ms | 78.27 MB | 4,254.68 ms | 9.80 MB |

## Interpretation

The benchmark workbook contains 1,000,000 shared-string cells, 100,000 unique strings, and no merged ranges. Its worksheet XML expands to 39,167,847 bytes.

Rust currently performs a complete worksheet scan before the emitting pass, even though this workbook has a valid `<dimension>` and merged-cell filling is disabled. It also clones each shared-string value into an intermediate `Data::String` and then again into the public `CellValue::String`, clones dynamic column names for every row, and transfers each parsed row through a bounded synchronous channel. These choices preserve bounded memory and iterator ownership but add steady-state work.

.NET v1 can stop its preliminary extent check as soon as it reads `<dimension ref="A1:J100000">`. Its JIT cost makes the first Query slower, but later Query passes benefit from already compiled and dynamically optimized hot paths. This explains why Rust leads in the cold scenario while .NET v1 leads in the steady scenario.

These numbers describe one workbook and one machine, not a general performance guarantee. The harness does not pin CPU affinity or suppress all operating-system noise; the median limits the effect of outliers. Use representative workbooks before drawing application-specific conclusions.

## Reproduce

Keep the Rust and .NET repositories in sibling directories, then run from the Rust repository:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

The script reads `v1.x-maintenance` from the local .NET Git repository without changing its checkout. It builds both runners in Release mode, verifies matching row and cell counts, prints both scenarios, and writes the full machine-readable report to `target/benchmarks/dotnet-v1-vs-rust.json`.

Use `-Scenario Cold` or `-Scenario Steady` to run one scenario. `-Passes` and `-WarmupPasses` configure the steady scenario; `-Iterations` controls the number of fresh measured processes.