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
| MiniExcel Rust | Source tree containing this document; optimization baseline `808828f3ce892dad8b00bda1cee370fae6451e1c` |

## Summary

Both implementations returned exactly 100,000 rows and 1,000,000 cells per pass in every measured process.

| Scenario | Runtime | Median Query time | Throughput | Median process time | Median peak working set | Maximum peak working set |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | .NET v1 | 1,731.33 ms | 57,759 rows/s | 1,785.67 ms | 66.81 MB | 67.45 MB |
| Cold | Rust | 660.42 ms | 151,419 rows/s | 666.39 ms | 7.23 MB | 7.32 MB |
| Steady | .NET v1 | 2,405.57 ms | 124,711 rows/s | 4,119.75 ms | 79.58 MB | 81.49 MB |
| Steady | Rust | 2,051.91 ms | 146,205 rows/s | 2,749.84 ms | 7.36 MB | 8.74 MB |

For the first Query in a fresh process, Rust delivered 2.62x the .NET v1 throughput, completed in 61.9% less Query time, and used 89.2% less peak working set.

After an in-process warm-up, Rust delivered 1.17x the .NET v1 throughput and completed in 14.7% less Query time. Its median peak working set was 90.8% lower, at 7.36 MB versus 79.58 MB.

Relative to the Rust optimization baseline, median Query time decreased by 54.6% in the Cold scenario and 51.3% in the Steady scenario while peak working set remained bounded below 9 MB.

## Cold Results

| Iteration | .NET v1 elapsed | .NET v1 peak | Rust elapsed | Rust peak |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1,697.58 ms | 66.09 MB | 670.55 ms | 6.59 MB |
| 2 | 1,737.33 ms | 66.81 MB | 646.70 ms | 7.30 MB |
| 3 | 1,626.67 ms | 66.16 MB | 660.42 ms | 7.32 MB |
| 4 | 1,753.76 ms | 67.45 MB | 674.44 ms | 7.04 MB |
| 5 | 1,731.33 ms | 67.40 MB | 659.06 ms | 7.23 MB |

## Steady Results

Each timed value below covers three complete Query passes after one untimed warm-up pass.

| Iteration | .NET v1 elapsed | .NET v1 peak | Rust elapsed | Rust peak |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,405.57 ms | 79.58 MB | 2,051.91 ms | 7.13 MB |
| 2 | 2,420.05 ms | 79.01 MB | 1,977.10 ms | 8.74 MB |
| 3 | 2,394.17 ms | 78.71 MB | 2,053.29 ms | 7.39 MB |
| 4 | 2,389.88 ms | 81.49 MB | 2,014.16 ms | 7.27 MB |
| 5 | 2,462.29 ms | 80.27 MB | 2,109.63 ms | 7.36 MB |

## Interpretation

The benchmark workbook contains 1,000,000 shared-string cells, 100,000 unique strings, and no merged ranges. Its worksheet XML expands to 39,167,847 bytes.

Rust now uses a valid `<dimension>` as the query extent when merged-cell filling does not require a complete worksheet scan. An explicit end cell skips the preliminary extent read as well. The parser reads hot row and cell attributes in one borrowed pass, parses shared-string indices without temporary `String` allocations, preallocates bounded row storage from the selected width, and moves owned string data from the intermediate `Data::String` into the public `CellValue::String`.

Memory-resident shared strings now use one contiguous UTF-8 buffer plus an end-offset table instead of one heap allocation per unique string. The offset table is preallocated from a file-size-bounded `uniqueCount`, and strings without Excel `_xHHHH_` escapes reuse their existing allocation. Compared with the preceding optimized result, this reduced median peak working set by 25.0% in Cold and 25.5% in Steady while reducing median Query time by another 2.1% and 1.3%, respectively.

The remaining per-row work includes cloning owned dynamic column names and transferring parsed rows through a bounded synchronous channel. These costs preserve the current `IndexMap<String, CellValue>` API, bounded memory, and iterator ownership. In this workbook, the optimized Rust path leads .NET v1 in both first-call latency and warmed sustained throughput while retaining substantially lower peak memory.

These numbers describe one workbook and one machine, not a general performance guarantee. The harness does not pin CPU affinity or suppress all operating-system noise; the median limits the effect of outliers. Use representative workbooks before drawing application-specific conclusions.

## Reproduce

Keep the Rust and .NET repositories in sibling directories, then run from the Rust repository:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

The script reads `v1.x-maintenance` from the local .NET Git repository without changing its checkout. It builds both runners in Release mode, verifies matching row and cell counts, prints both scenarios, and writes the full machine-readable report to `target/benchmarks/dotnet-v1-vs-rust.json`.

Use `-Scenario Cold` or `-Scenario Steady` to run one scenario. `-Passes` and `-WarmupPasses` configure the steady scenario; `-Iterations` controls the number of fresh measured processes.