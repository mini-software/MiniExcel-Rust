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

The following result was captured on 2026-08-28:

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
| Cold | .NET v1 | 1,731.77 ms | 57,744 rows/s | 1,784.10 ms | 67.75 MB | 68.13 MB |
| Cold | Rust | 670.45 ms | 149,154 rows/s | 675.49 ms | 6.86 MB | 7.08 MB |
| Steady | .NET v1 | 2,230.25 ms | 134,514 rows/s | 4,044.79 ms | 80.62 MB | 83.86 MB |
| Steady | Rust | 2,081.60 ms | 144,120 rows/s | 2,815.70 ms | 6.71 MB | 7.20 MB |

For the first Query in a fresh process, Rust delivered 2.58x the .NET v1 throughput, completed in 61.3% less Query time, and used 89.9% less peak working set.

After an in-process warm-up, Rust delivered 1.07x the .NET v1 throughput and completed in 6.7% less Query time. Its median peak working set was 91.7% lower, at 6.71 MB versus 80.62 MB.

Relative to the Rust optimization baseline, median Query time decreased by 54.0% in the Cold scenario and 50.5% in the Steady scenario while peak working set remained bounded below 8 MB.

## Cold Results

| Iteration | .NET v1 elapsed | .NET v1 peak | Rust elapsed | Rust peak |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1,745.97 ms | 68.13 MB | 670.45 ms | 6.86 MB |
| 2 | 1,679.53 ms | 66.18 MB | 642.60 ms | 6.68 MB |
| 3 | 1,734.04 ms | 67.75 MB | 663.40 ms | 5.98 MB |
| 4 | 1,731.77 ms | 67.93 MB | 670.64 ms | 7.08 MB |
| 5 | 1,702.00 ms | 67.32 MB | 679.42 ms | 7.02 MB |

## Steady Results

Each timed value below covers three complete Query passes after one untimed warm-up pass.

| Iteration | .NET v1 elapsed | .NET v1 peak | Rust elapsed | Rust peak |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,447.41 ms | 80.35 MB | 2,081.60 ms | 6.46 MB |
| 2 | 2,380.37 ms | 80.24 MB | 2,016.59 ms | 6.55 MB |
| 3 | 2,188.67 ms | 83.86 MB | 1,909.48 ms | 7.04 MB |
| 4 | 2,160.40 ms | 80.62 MB | 2,356.20 ms | 6.71 MB |
| 5 | 2,230.25 ms | 80.81 MB | 2,426.91 ms | 7.20 MB |

## Interpretation

The benchmark workbook contains 1,000,000 shared-string cells, 100,000 unique strings, and no merged ranges. Its worksheet XML expands to 39,167,847 bytes.

Rust now uses a valid `<dimension>` as the query extent when merged-cell filling does not require a complete worksheet scan. An explicit end cell skips the preliminary extent read as well. The parser reads hot row and cell attributes in one borrowed pass, parses shared-string indices without temporary `String` allocations, preallocates bounded row storage from the selected width, and moves owned string data from the intermediate `Data::String` into the public `CellValue::String`.

Memory-resident shared strings now use one contiguous UTF-8 buffer plus an end-offset table instead of one heap allocation per unique string. The offset table is preallocated from a file-size-bounded `uniqueCount`, and strings without Excel `_xHHHH_` escapes reuse their existing allocation. Offsets use `u32` while the combined string data fits within 4 GiB and automatically widen to `usize` beyond that boundary, preserving large-input behavior.

Compared with the preceding optimized result, the adaptive offset representation reduced median peak working set by another 5.1% in Cold and 8.8% in Steady. Two final same-period alternating A/B runs put Query timing within approximately 1% of the `usize` baseline, with no consistent regression; cross-run timing differences in the tables above remain subject to normal machine load variation.

The remaining per-row work includes cloning owned dynamic column names and transferring parsed rows through a bounded synchronous channel. These costs preserve the current `IndexMap<String, CellValue>` API, bounded memory, and iterator ownership. In this workbook, the optimized Rust path leads .NET v1 in both first-call latency and warmed sustained throughput while retaining substantially lower peak memory.

These numbers describe one workbook and one machine, not a general performance guarantee. The harness does not pin CPU affinity or suppress all operating-system noise; the median limits the effect of outliers. Use representative workbooks before drawing application-specific conclusions.

## Reproduce

Keep the Rust and .NET repositories in sibling directories, then run from the Rust repository:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

The script reads `v1.x-maintenance` from the local .NET Git repository without changing its checkout. It builds both runners in Release mode, verifies matching row and cell counts, prints both scenarios, and writes the full machine-readable report to `target/benchmarks/dotnet-v1-vs-rust.json`.

Use `-Scenario Cold` or `-Scenario Steady` to run one scenario. `-Passes` and `-WarmupPasses` configure the steady scenario; `-Iterations` controls the number of fresh measured processes.