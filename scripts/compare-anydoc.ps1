[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Workbook,
    [string]$OutputDirectory = "target/markdown-comparison",
    [ValidateRange(1, 100)]
    [int]$Iterations = 3,
    [ValidateRange(1, 10000)]
    [int]$ChunkRows = 25,
    [bool]$Header = $true,
    [string]$AnyDocVersion = "0.1.9"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$workbookPath = (Resolve-Path $Workbook).Path
$outputPath = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory
} else {
    Join-Path $repoRoot $OutputDirectory
}
New-Item -ItemType Directory -Force -Path $outputPath | Out-Null

function Assert-LastExitCode([string]$label) {
    if ($LASTEXITCODE -ne 0) {
        throw "$label failed with exit code $LASTEXITCODE"
    }
}

function Invoke-MeasuredProcess {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.WorkingDirectory = $repoRoot
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }

    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    if (-not $process.Start()) {
        throw "Could not start $FilePath"
    }
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $peakWorkingSet = 0L
    while ($true) {
        $process.Refresh()
        $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.WorkingSet64)
        $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.PeakWorkingSet64)
        if ($process.WaitForExit(1)) {
            break
        }
    }
    $process.WaitForExit()
    $stopwatch.Stop()
    $process.Refresh()
    $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.PeakWorkingSet64)
    if ($peakWorkingSet -le 0) {
        throw "Could not sample peak working set for $FilePath"
    }
    $result = [pscustomobject]@{
        ExitCode = $process.ExitCode
        WallMilliseconds = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 2)
        CpuMilliseconds = [Math]::Round($process.TotalProcessorTime.TotalMilliseconds, 2)
        PeakWorkingSetBytes = $peakWorkingSet
        StandardOutput = $stdout.Result
        StandardError = $stderr.Result
    }
    $process.Dispose()
    if ($result.ExitCode -ne 0) {
        throw "$FilePath failed with exit code $($result.ExitCode): $($result.StandardError)"
    }
    return $result
}

function Get-Median([double[]]$values) {
    $sorted = @($values | Sort-Object)
    $middle = [Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) {
        return $sorted[$middle]
    }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

function Get-MarkdownMetrics([string]$path) {
    $lines = [IO.File]::ReadAllLines($path)
    $tableRows = @($lines | Where-Object {
        $_ -match '^\s*\|.*\|\s*$' -and
        $_ -notmatch '^\s*\|(?:\s*:?-{3,}:?\s*\|)+\s*$'
    }).Count
    return [pscustomobject]@{
        Bytes = (Get-Item $path).Length
        Lines = $lines.Count
        Headings = @($lines | Where-Object { $_ -match '^#{1,6}\s' }).Count
        TableContentRows = $tableRows
        Chunks = @($lines | Where-Object { $_ -match 'miniexcel:chunk-start' }).Count
    }
}

$cargo = (Get-Command cargo).Source
$node = (Get-Command node).Source
$npm = (Get-Command npm.cmd).Source

Write-Host "Building MiniExcel release CLI..."
& $cargo build --release -p miniexcel-cli --locked
Assert-LastExitCode "cargo build"
$miniExcel = Join-Path $repoRoot "target/release/miniexcel.exe"

$anyDocRoot = Join-Path $outputPath ".anydoc"
$anyDocCli = Join-Path $anyDocRoot "node_modules/@firecrawl/anydoc/cli.js"
if (-not (Test-Path $anyDocCli)) {
    Write-Host "Installing @firecrawl/anydoc@$AnyDocVersion outside the source tree..."
    & $npm install --prefix $anyDocRoot --no-save --no-package-lock "@firecrawl/anydoc@$AnyDocVersion"
    Assert-LastExitCode "anydoc install"
}
$resolvedAnyDocVersion = (& $node $anyDocCli --version).Trim()
Assert-LastExitCode "anydoc version"

function Invoke-MiniExcel([string]$name) {
    $prefix = Join-Path $outputPath $name
    Remove-Item "$prefix.*" -Force -ErrorAction SilentlyContinue
    $arguments = @(
        "rag-export", $workbookPath,
        "--output-prefix", $prefix,
        "--format", "markdown",
        "--chunk-rows", $ChunkRows.ToString()
    )
    if ($Header) {
        $arguments += "--header"
    }
    $measurement = Invoke-MeasuredProcess $miniExcel $arguments
    return [pscustomobject]@{
        Measurement = $measurement
        Output = "$prefix.chunks.md"
    }
}

function Invoke-AnyDoc([string]$name) {
    $output = Join-Path $outputPath "$name.md"
    Remove-Item $output -Force -ErrorAction SilentlyContinue
    $measurement = Invoke-MeasuredProcess $node @($anyDocCli, $workbookPath, "-o", $output)
    return [pscustomobject]@{
        Measurement = $measurement
        Output = $output
    }
}

Write-Host "Warming file-system and runtime caches..."
$null = Invoke-MiniExcel "warmup-miniexcel"
$null = Invoke-AnyDoc "warmup-anydoc"

$results = @()
for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
    $order = if ($iteration % 2 -eq 1) { @("MiniExcel", "anydoc") } else { @("anydoc", "MiniExcel") }
    foreach ($tool in $order) {
        Write-Host "Iteration $iteration/${Iterations}: $tool"
        $run = if ($tool -eq "MiniExcel") {
            Invoke-MiniExcel "miniexcel-$iteration"
        } else {
            Invoke-AnyDoc "anydoc-$iteration"
        }
        $results += [pscustomobject]@{
            Tool = $tool
            Iteration = $iteration
            WallMilliseconds = $run.Measurement.WallMilliseconds
            CpuMilliseconds = $run.Measurement.CpuMilliseconds
            PeakWorkingSetBytes = $run.Measurement.PeakWorkingSetBytes
            OutputBytes = (Get-Item $run.Output).Length
            Output = $run.Output
        }
    }
}

$summaries = foreach ($tool in @("MiniExcel", "anydoc")) {
    $toolResults = @($results | Where-Object Tool -eq $tool)
    [pscustomobject]@{
        Tool = $tool
        MedianWallMilliseconds = [Math]::Round((Get-Median $toolResults.WallMilliseconds), 2)
        MedianCpuMilliseconds = [Math]::Round((Get-Median $toolResults.CpuMilliseconds), 2)
        MedianPeakWorkingSetMiB = [Math]::Round((Get-Median (($toolResults.PeakWorkingSetBytes | ForEach-Object { $_ / 1MB }))), 2)
        OutputBytes = $toolResults[-1].OutputBytes
    }
}

$miniOutput = ($results | Where-Object Tool -eq "MiniExcel")[-1].Output
$anyDocOutput = ($results | Where-Object Tool -eq "anydoc")[-1].Output
$miniMetrics = Get-MarkdownMetrics $miniOutput
$anyDocMetrics = Get-MarkdownMetrics $anyDocOutput
$inputBytes = (Get-Item $workbookPath).Length
$timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss K"

$report = @"
# MiniExcel / anydoc Markdown comparison

- Timestamp: $timestamp
- Workbook: ``$workbookPath`` ($inputBytes bytes)
- MiniExcel chunk rows: $ChunkRows
- Header mode: $Header
- anydoc version: $resolvedAnyDocVersion
- Iterations: $Iterations after one unmeasured warm-up
- Timing scope: direct CLI process start through file completion

## Resources

| Tool | Median wall ms | Median CPU ms | Median peak working set MiB | Output bytes |
| --- | ---: | ---: | ---: | ---: |
| MiniExcel | $($summaries[0].MedianWallMilliseconds) | $($summaries[0].MedianCpuMilliseconds) | $($summaries[0].MedianPeakWorkingSetMiB) | $($summaries[0].OutputBytes) |
| anydoc | $($summaries[1].MedianWallMilliseconds) | $($summaries[1].MedianCpuMilliseconds) | $($summaries[1].MedianPeakWorkingSetMiB) | $($summaries[1].OutputBytes) |

Peak working set is the operating system's per-process value. It excludes the
one-time Cargo build and anydoc package installation. Alternate run order and
the warm-up reduce file-cache bias but do not make this a controlled lab result.

## Output structure

| Tool | Bytes | Lines | Headings | GFM content rows | Independent chunks |
| --- | ---: | ---: | ---: | ---: | ---: |
| MiniExcel | $($miniMetrics.Bytes) | $($miniMetrics.Lines) | $($miniMetrics.Headings) | $($miniMetrics.TableContentRows) | $($miniMetrics.Chunks) |
| anydoc | $($anyDocMetrics.Bytes) | $($anyDocMetrics.Lines) | $($anyDocMetrics.Headings) | $($anyDocMetrics.TableContentRows) | $($anyDocMetrics.Chunks) |

MiniExcel targets selected-sheet RAG evidence: chunks repeat headers, retain
source row coordinates and formulas, and remain valid when a stream ends early.
anydoc targets compact whole-document conversion: it builds one document model
and renders a conventional GFM table. Use a one-sheet workbook for the closest
content comparison. Inspect ``$miniOutput`` and ``$anyDocOutput`` for value-level
differences.
"@

$reportPath = Join-Path $outputPath "report.md"
$jsonPath = Join-Path $outputPath "measurements.json"
[IO.File]::WriteAllText($reportPath, $report, [Text.UTF8Encoding]::new($false))
$results | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 $jsonPath
Write-Host "Report: $reportPath"
Write-Host "Measurements: $jsonPath"
Write-Output $report