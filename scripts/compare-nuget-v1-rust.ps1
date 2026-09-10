[CmdletBinding()]
param(
    [ValidateSet('win-x64', 'win-arm64', 'linux-x64', 'linux-arm64', 'osx-x64', 'osx-arm64')]
    [string] $Rid = 'win-x64',

    [ValidateRange(1, 20)]
    [int] $Iterations = 5,

    [ValidateRange(1, 1000000)]
    [int] $Rows = 100000,

    [ValidateRange(1, 26)]
    [int] $Columns = 10,

    [ValidateRange(1, 100)]
    [int] $Passes = 3,

    [ValidateRange(0, 100)]
    [int] $WarmupPasses = 1,

    [ValidateSet('Cold', 'Steady', 'Both')]
    [string] $Scenario = 'Both',

    [string] $MiniExcelVersion,

    [string] $MiniExcelRustVersion = '0.1.0-benchmark',

    [switch] $SkipPackageBuild,

    [string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path $PSScriptRoot -Parent
$packageDirectory = Join-Path $repositoryRoot 'target/nuget/packages'
$packageCache = Join-Path $repositoryRoot 'target/nuget/benchmark-packages'
$restoreDirectory = Join-Path $repositoryRoot 'target/nuget/benchmark-restore'
$restoreConfig = Join-Path $restoreDirectory 'nuget.config'
$project = Join-Path $repositoryRoot 'benchmarks/nuget-v1-query/NuGetV1Query.csproj'
$runner = Join-Path $repositoryRoot 'benchmarks/nuget-v1-query/bin/Release/net8.0/NuGetV1Query.dll'
$nativeSuffix = if ($env:OS -eq 'Windows_NT') { '.exe' } else { '' }
$nativeRunner = Join-Path $repositoryRoot "target/release/examples/stress_query$nativeSuffix"
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repositoryRoot 'target/benchmarks/nuget-v1'
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$workbook = Join-Path $OutputDirectory "benchmark-$Rows`x$Columns.xlsx"

if ([string]::IsNullOrWhiteSpace($MiniExcelVersion)) {
    $versionIndex = Invoke-RestMethod 'https://api.nuget.org/v3-flatcontainer/miniexcel/index.json'
    $MiniExcelVersion = @(
        $versionIndex.versions |
            Where-Object { $_ -match '^1\.\d+\.\d+$' } |
            Sort-Object { [version]$_ } -Descending
    )[0]
}
if ($MiniExcelVersion -notmatch '^1\.') {
    throw "MiniExcelVersion must be a stable v1 release, received '$MiniExcelVersion'."
}

if (-not $SkipPackageBuild) {
    & (Join-Path $repositoryRoot 'scripts/dotnet/Test-Package.ps1') `
        -Rid $Rid `
        -Version $MiniExcelRustVersion
    if ($LASTEXITCODE -ne 0) {
        throw 'MiniExcel.Rust package build and smoke test failed.'
    }
}

$package = Join-Path $packageDirectory "MiniExcel.Rust.$MiniExcelRustVersion.nupkg"
if (-not (Test-Path $package)) {
    throw "Candidate package not found: $package"
}

New-Item -ItemType Directory -Path $OutputDirectory, $restoreDirectory -Force | Out-Null
$cachedCandidate = Join-Path $packageCache "miniexcel.rust/$($MiniExcelRustVersion.ToLowerInvariant())"
if (Test-Path $cachedCandidate) {
    Remove-Item $cachedCandidate -Recurse -Force
}
& dotnet new nugetconfig --output $restoreDirectory --force | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'NuGet configuration creation failed.' }
& dotnet nuget add source $packageDirectory --name MiniExcelRustLocal --configfile $restoreConfig | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Local NuGet source configuration failed.' }
& dotnet restore $project `
    --force `
    --no-cache `
    --packages $packageCache `
    --configfile $restoreConfig `
    -p:MiniExcelVersion=$MiniExcelVersion `
    -p:MiniExcelRustPackageVersion=$MiniExcelRustVersion
if ($LASTEXITCODE -ne 0) { throw 'Benchmark restore failed.' }
& dotnet build $project `
    -c Release `
    --no-restore `
    -p:MiniExcelVersion=$MiniExcelVersion `
    -p:MiniExcelRustPackageVersion=$MiniExcelRustVersion
if ($LASTEXITCODE -ne 0) { throw 'Benchmark build failed.' }
& cargo +1.85.0 build --release -p miniexcel --example stress_query --locked
if ($LASTEXITCODE -ne 0) { throw 'Native Rust benchmark build failed.' }

& dotnet $runner generate $workbook $Rows $Columns
if ($LASTEXITCODE -ne 0) { throw 'Benchmark workbook generation failed.' }
& dotnet $runner verify $workbook
if ($LASTEXITCODE -ne 0) { throw 'MiniExcel and MiniExcel.Rust returned different data.' }

function Invoke-MeasuredProcess {
    param(
        [string] $Runtime,
        [pscustomobject] $BenchmarkScenario,
        [int] $Iteration
    )

    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = if ($Runtime -eq 'rust-native') { $nativeRunner } else { 'dotnet' }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $arguments = if ($Runtime -eq 'rust-native') {
        @($workbook, "$($BenchmarkScenario.Passes)", "$($BenchmarkScenario.WarmupPasses)")
    } else {
        @($runner, $Runtime, $workbook, "$($BenchmarkScenario.Passes)", "$($BenchmarkScenario.WarmupPasses)")
    }
    foreach ($argument in $arguments) {
        $startInfo.ArgumentList.Add($argument)
    }

    $processStopwatch = [Diagnostics.Stopwatch]::StartNew()
    $process = [Diagnostics.Process]::Start($startInfo)
    $peakWorkingSet = 0L
    $peakPrivateBytes = 0L
    while (-not $process.WaitForExit(10)) {
        $process.Refresh()
        $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.WorkingSet64)
        $peakPrivateBytes = [Math]::Max($peakPrivateBytes, $process.PrivateMemorySize64)
    }
    $processStopwatch.Stop()
    $output = $process.StandardOutput.ReadToEnd().Trim()
    $errors = $process.StandardError.ReadToEnd().Trim()
    if ($process.ExitCode -ne 0) {
        throw "$Runtime $($BenchmarkScenario.Name) failed: $errors"
    }
    $measurement = $output | ConvertFrom-Json
    [pscustomobject]@{
        Scenario = $BenchmarkScenario.Name
        Runtime = $measurement.Runtime
        Iteration = $Iteration
        Passes = $measurement.Passes
        Rows = $measurement.Rows
        Cells = $measurement.Cells
        ContentHash = $measurement.ContentHash
        ElapsedMs = [Math]::Round($measurement.ElapsedMilliseconds, 2)
        FirstRowMs = [Math]::Round($measurement.FirstRowMilliseconds, 2)
        AllocatedMB = if ($null -eq $measurement.AllocatedBytes) {
            $null
        } else {
            [Math]::Round($measurement.AllocatedBytes / 1MB, 2)
        }
        ProcessElapsedMs = [Math]::Round($processStopwatch.Elapsed.TotalMilliseconds, 2)
        PeakWorkingSetMB = [Math]::Round($peakWorkingSet / 1MB, 2)
        PeakPrivateMB = [Math]::Round($peakPrivateBytes / 1MB, 2)
    }
}

function Get-Median {
    param([double[]] $Values)

    $sorted = @($Values | Sort-Object)
    $middle = [Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) {
        return $sorted[$middle]
    }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

$scenarios = @()
if ($Scenario -in @('Cold', 'Both')) {
    $scenarios += [pscustomobject]@{ Name = 'Cold'; Passes = 1; WarmupPasses = 0 }
}
if ($Scenario -in @('Steady', 'Both')) {
    $scenarios += [pscustomobject]@{ Name = 'Steady'; Passes = $Passes; WarmupPasses = $WarmupPasses }
}

foreach ($runtime in @('managed', 'rust-dotnet', 'rust-native')) {
    $null = Invoke-MeasuredProcess -Runtime $runtime `
        -BenchmarkScenario ([pscustomobject]@{ Name = 'Preflight'; Passes = 1; WarmupPasses = 0 }) `
        -Iteration 0
}

$results = [Collections.Generic.List[object]]::new()
$runtimeKeys = @('managed', 'rust-dotnet', 'rust-native')
for ($scenarioIndex = 0; $scenarioIndex -lt $scenarios.Count; $scenarioIndex++) {
    $benchmarkScenario = $scenarios[$scenarioIndex]
    foreach ($iteration in 1..$Iterations) {
        $offset = ($iteration + $scenarioIndex - 1) % $runtimeKeys.Count
        $order = @(0..($runtimeKeys.Count - 1) | ForEach-Object {
            $runtimeKeys[($_ + $offset) % $runtimeKeys.Count]
        })
        foreach ($runtime in $order) {
            $results.Add((Invoke-MeasuredProcess -Runtime $runtime -BenchmarkScenario $benchmarkScenario -Iteration $iteration))
        }
    }
}

foreach ($benchmarkScenario in $scenarios) {
    $scenarioResults = @($results | Where-Object Scenario -eq $benchmarkScenario.Name)
    if (($scenarioResults.Rows | Select-Object -Unique).Count -ne 1 -or
        ($scenarioResults.Cells | Select-Object -Unique).Count -ne 1 -or
        ($scenarioResults.ContentHash | Select-Object -Unique).Count -ne 1) {
        throw "$($benchmarkScenario.Name): the three runners returned different results."
    }
}

$summary = foreach ($benchmarkScenario in $scenarios) {
    foreach ($runtime in @('MiniExcel', 'MiniExcel.Rust (.NET)', 'MiniExcel.Rust')) {
        $group = @($results | Where-Object { $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq $runtime })
        $medianElapsed = Get-Median ([double[]]$group.ElapsedMs)
        $allocatedValues = @($group.AllocatedMB | Where-Object { $null -ne $_ })
        [pscustomobject]@{
            Scenario = $benchmarkScenario.Name
            Runtime = $runtime
            MedianElapsedMs = [Math]::Round($medianElapsed, 2)
            RowsPerSecond = [Math]::Round($group[0].Rows / ($medianElapsed / 1000), 0)
            MedianFirstRowMs = [Math]::Round((Get-Median ([double[]]$group.FirstRowMs)), 2)
            MedianAllocatedMB = if ($allocatedValues.Count -eq 0) {
                $null
            } else {
                [Math]::Round((Get-Median ([double[]]$allocatedValues)), 2)
            }
            MedianPeakWorkingSetMB = [Math]::Round((Get-Median ([double[]]$group.PeakWorkingSetMB)), 2)
            MedianPeakPrivateMB = [Math]::Round((Get-Median ([double[]]$group.PeakPrivateMB)), 2)
        }
    }
}

$comparison = foreach ($benchmarkScenario in $scenarios) {
    $managed = $summary | Where-Object { $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq 'MiniExcel' }
    foreach ($runtime in @('MiniExcel.Rust (.NET)', 'MiniExcel.Rust')) {
        $rust = $summary | Where-Object { $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq $runtime }
        [pscustomobject]@{
            Scenario = $benchmarkScenario.Name
            Runtime = $runtime
            RustSpeedup = [Math]::Round($managed.MedianElapsedMs / $rust.MedianElapsedMs, 2)
            AllocationReductionPercent = if ($null -eq $rust.MedianAllocatedMB) {
                $null
            } else {
                [Math]::Round((1 - $rust.MedianAllocatedMB / $managed.MedianAllocatedMB) * 100, 1)
            }
            WorkingSetReductionPercent = [Math]::Round((1 - $rust.MedianPeakWorkingSetMB / $managed.MedianPeakWorkingSetMB) * 100, 1)
        }
    }
}

$report = [ordered]@{
    TimestampUtc = [DateTime]::UtcNow.ToString('O')
    OperatingSystem = [Runtime.InteropServices.RuntimeInformation]::OSDescription
    Architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    Rid = $Rid
    DotNetSdk = (& dotnet --version).Trim()
    MiniExcelVersion = $MiniExcelVersion
    MiniExcelRustVersion = $MiniExcelRustVersion
    MiniExcelRustNativeVersion = '0.4.0'
    RustRevision = (& git -C $repositoryRoot rev-parse HEAD).Trim()
    Rows = $Rows
    Columns = $Columns
    WorkbookSha256 = (Get-FileHash $workbook -Algorithm SHA256).Hash.ToLowerInvariant()
    PackageSha256 = (Get-FileHash $package -Algorithm SHA256).Hash.ToLowerInvariant()
    Iterations = $Iterations
    Results = $results
    Summary = $summary
    Comparison = $comparison
}
$jsonPath = Join-Path $OutputDirectory "benchmark-$Rid.json"
$markdownPath = Join-Path $OutputDirectory "benchmark-$Rid.md"
$report | ConvertTo-Json -Depth 6 | Set-Content $jsonPath
$results | Format-Table Scenario,Runtime,Iteration,ElapsedMs,FirstRowMs,AllocatedMB,PeakWorkingSetMB,PeakPrivateMB -AutoSize
$summary | Format-Table Scenario,Runtime,MedianElapsedMs,RowsPerSecond,MedianFirstRowMs,MedianAllocatedMB,MedianPeakWorkingSetMB -AutoSize

$markdown = [Collections.Generic.List[string]]::new()
$markdown.Add("# MiniExcel v1 vs MiniExcel.Rust (.NET) vs MiniExcel.Rust ($Rid)")
$markdown.Add('')
$markdown.Add("- Date (UTC): $($report.TimestampUtc)")
$markdown.Add("- MiniExcel: $MiniExcelVersion")
$markdown.Add("- MiniExcel.Rust: $MiniExcelRustVersion")
$markdown.Add("- MiniExcel.Rust native: 0.4.0 ($($report.RustRevision))")
$markdown.Add("- Workbook: $Rows rows x $Columns columns")
$markdown.Add("- Iterations: $Iterations fresh processes per runtime and scenario")
$markdown.Add('')
$markdown.Add('| Scenario | Runtime | Median elapsed (ms) | Rows/s | First row (ms) | Allocated (MB) | Peak working set (MB) |')
$markdown.Add('| --- | --- | ---: | ---: | ---: | ---: | ---: |')
foreach ($item in $summary) {
    $allocated = if ($null -eq $item.MedianAllocatedMB) { 'n/a' } else { $item.MedianAllocatedMB }
    $markdown.Add("| $($item.Scenario) | $($item.Runtime) | $($item.MedianElapsedMs) | $($item.RowsPerSecond) | $($item.MedianFirstRowMs) | $allocated | $($item.MedianPeakWorkingSetMB) |")
}
$markdown.Add('')
foreach ($item in $comparison) {
    $allocation = if ($null -eq $item.AllocationReductionPercent) { 'n/a' } else { "$($item.AllocationReductionPercent)%" }
    $markdown.Add("- $($item.Scenario), $($item.Runtime): speedup $($item.RustSpeedup)x; allocation reduction $allocation; working-set reduction $($item.WorkingSetReductionPercent)%.")
}
$markdown | Set-Content $markdownPath
Write-Host "Report: $jsonPath"
Write-Host "Markdown: $markdownPath"