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
    $startInfo.FileName = 'dotnet'
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @(
        $runner,
        $Runtime,
        $workbook,
        "$($BenchmarkScenario.Passes)",
        "$($BenchmarkScenario.WarmupPasses)"
    )) {
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
        AllocatedMB = [Math]::Round($measurement.AllocatedBytes / 1MB, 2)
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

foreach ($runtime in @('managed', 'rust')) {
    $null = Invoke-MeasuredProcess -Runtime $runtime `
        -BenchmarkScenario ([pscustomobject]@{ Name = 'Preflight'; Passes = 1; WarmupPasses = 0 }) `
        -Iteration 0
}

$results = [Collections.Generic.List[object]]::new()
for ($scenarioIndex = 0; $scenarioIndex -lt $scenarios.Count; $scenarioIndex++) {
    $benchmarkScenario = $scenarios[$scenarioIndex]
    foreach ($iteration in 1..$Iterations) {
        $order = if (($iteration + $scenarioIndex) % 2 -eq 1) { @('managed', 'rust') } else { @('rust', 'managed') }
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
        throw "$($benchmarkScenario.Name): MiniExcel and MiniExcel.Rust returned different results."
    }
}

$summary = foreach ($benchmarkScenario in $scenarios) {
    foreach ($runtime in @('MiniExcel', 'MiniExcel.Rust')) {
        $group = @($results | Where-Object { $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq $runtime })
        $medianElapsed = Get-Median ([double[]]$group.ElapsedMs)
        [pscustomobject]@{
            Scenario = $benchmarkScenario.Name
            Runtime = $runtime
            MedianElapsedMs = [Math]::Round($medianElapsed, 2)
            RowsPerSecond = [Math]::Round($group[0].Rows / ($medianElapsed / 1000), 0)
            MedianFirstRowMs = [Math]::Round((Get-Median ([double[]]$group.FirstRowMs)), 2)
            MedianAllocatedMB = [Math]::Round((Get-Median ([double[]]$group.AllocatedMB)), 2)
            MedianPeakWorkingSetMB = [Math]::Round((Get-Median ([double[]]$group.PeakWorkingSetMB)), 2)
            MedianPeakPrivateMB = [Math]::Round((Get-Median ([double[]]$group.PeakPrivateMB)), 2)
        }
    }
}

$comparison = foreach ($benchmarkScenario in $scenarios) {
    $managed = $summary | Where-Object { $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq 'MiniExcel' }
    $rust = $summary | Where-Object { $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq 'MiniExcel.Rust' }
    [pscustomobject]@{
        Scenario = $benchmarkScenario.Name
        RustSpeedup = [Math]::Round($managed.MedianElapsedMs / $rust.MedianElapsedMs, 2)
        AllocationReductionPercent = [Math]::Round((1 - $rust.MedianAllocatedMB / $managed.MedianAllocatedMB) * 100, 1)
        WorkingSetReductionPercent = [Math]::Round((1 - $rust.MedianPeakWorkingSetMB / $managed.MedianPeakWorkingSetMB) * 100, 1)
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
$markdown.Add("# MiniExcel v1 vs MiniExcel.Rust ($Rid)")
$markdown.Add('')
$markdown.Add("- Date (UTC): $($report.TimestampUtc)")
$markdown.Add("- MiniExcel: $MiniExcelVersion")
$markdown.Add("- MiniExcel.Rust: $MiniExcelRustVersion")
$markdown.Add("- Workbook: $Rows rows x $Columns columns")
$markdown.Add("- Iterations: $Iterations fresh processes per runtime and scenario")
$markdown.Add('')
$markdown.Add('| Scenario | Runtime | Median elapsed (ms) | Rows/s | First row (ms) | Allocated (MB) | Peak working set (MB) |')
$markdown.Add('| --- | --- | ---: | ---: | ---: | ---: | ---: |')
foreach ($item in $summary) {
    $markdown.Add("| $($item.Scenario) | $($item.Runtime) | $($item.MedianElapsedMs) | $($item.RowsPerSecond) | $($item.MedianFirstRowMs) | $($item.MedianAllocatedMB) | $($item.MedianPeakWorkingSetMB) |")
}
$markdown.Add('')
foreach ($item in $comparison) {
    $markdown.Add("- $($item.Scenario): Rust speedup $($item.RustSpeedup)x; allocation reduction $($item.AllocationReductionPercent)%; working-set reduction $($item.WorkingSetReductionPercent)%.")
}
$markdown | Set-Content $markdownPath
Write-Host "Report: $jsonPath"
Write-Host "Markdown: $markdownPath"