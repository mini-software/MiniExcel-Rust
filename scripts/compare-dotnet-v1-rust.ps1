param(
    [string]$DotNetRepository,
    [string]$DotNetRevision = "v1.x-maintenance",
    [string]$Workbook,
    [ValidateRange(1, 100)]
    [int]$Passes = 3,
    [ValidateRange(1, 100)]
    [int]$Iterations = 5,
    [string]$OutputJson,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path $PSScriptRoot -Parent

if ([string]::IsNullOrWhiteSpace($DotNetRepository)) {
    $DotNetRepository = Join-Path (Split-Path $repositoryRoot -Parent) "MiniExcel"
}

$DotNetRepository = [IO.Path]::GetFullPath($DotNetRepository)
$rustManifest = Join-Path $repositoryRoot "Cargo.toml"
$dotnetRunnerProject = Join-Path $repositoryRoot "benchmarks\dotnet-v1-query\DotNetV1Query.csproj"
$benchmarkRoot = Join-Path $repositoryRoot "target\benchmarks"

foreach ($requiredPath in @($DotNetRepository, $rustManifest, $dotnetRunnerProject)) {
    if (-not (Test-Path $requiredPath)) {
        throw "Required path not found: $requiredPath"
    }
}

$dotnetCommit = (& git -C $DotNetRepository rev-parse "$DotNetRevision^{commit}").Trim()
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($dotnetCommit)) {
    throw "Unable to resolve .NET revision '$DotNetRevision'."
}

$dotnetSource = Join-Path $benchmarkRoot "dotnet-v1-$($dotnetCommit.Substring(0, 12))"
if (-not (Test-Path $dotnetSource)) {
    New-Item -ItemType Directory -Force -Path $benchmarkRoot | Out-Null
    $archive = Join-Path $benchmarkRoot "dotnet-v1-$($dotnetCommit.Substring(0, 12)).zip"
    & git -C $DotNetRepository archive --format=zip --output=$archive $dotnetCommit
    if ($LASTEXITCODE -ne 0) { throw "Unable to archive .NET revision '$DotNetRevision'." }
    Expand-Archive -Path $archive -DestinationPath $dotnetSource
    Remove-Item $archive
}

$dotnetV1Project = Join-Path $dotnetSource "src\MiniExcel\MiniExcelLibs.csproj"
if (-not (Test-Path $dotnetV1Project)) {
    throw "Revision '$DotNetRevision' is not a MiniExcel v1 source tree."
}

[xml]$dotnetProjectXml = Get-Content $dotnetV1Project
$dotnetVersion = ([string]$dotnetProjectXml.Project.PropertyGroup.Version).Trim()
if (-not $dotnetVersion.StartsWith("1.")) {
    throw "Revision '$DotNetRevision' reports MiniExcel $dotnetVersion, not v1."
}

if ([string]::IsNullOrWhiteSpace($Workbook)) {
    $Workbook = Join-Path $dotnetSource "benchmarks\MiniExcel.Benchmarks\Test100,000x10.xlsx"
}
$Workbook = [IO.Path]::GetFullPath($Workbook)
if (-not (Test-Path $Workbook)) {
    throw "Workbook not found: $Workbook"
}

$dotnetRunner = Join-Path $repositoryRoot "benchmarks\dotnet-v1-query\bin\Release\net10.0\DotNetV1Query.dll"
$executableSuffix = if ($env:OS -eq "Windows_NT") { ".exe" } else { "" }
$rustRunner = Join-Path $repositoryRoot "target\release\examples\stress_query$executableSuffix"

if (-not $SkipBuild) {
    & dotnet build $dotnetRunnerProject -c Release --nologo --verbosity:quiet `
        -p:MiniExcelV1Project=$dotnetV1Project
    if ($LASTEXITCODE -ne 0) { throw ".NET v1 stress runner build failed." }

    & cargo +1.85.0 build --manifest-path $rustManifest --release `
        -p miniexcel --example stress_query --locked
    if ($LASTEXITCODE -ne 0) { throw "Rust stress runner build failed." }
}

foreach ($runner in @($dotnetRunner, $rustRunner)) {
    if (-not (Test-Path $runner)) {
        throw "Runner not found: $runner. Run without -SkipBuild first."
    }
}

function Invoke-MeasuredProcess {
    param(
        [string]$Runtime,
        [string]$Executable,
        [string[]]$Arguments,
        [int]$Iteration
    )

    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }

    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $process = [Diagnostics.Process]::Start($startInfo)
    $peakWorkingSet = 0L
    while (-not $process.WaitForExit(10)) {
        $process.Refresh()
        $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.WorkingSet64)
    }
    $stopwatch.Stop()

    $standardOutput = $process.StandardOutput.ReadToEnd().Trim()
    $standardError = $process.StandardError.ReadToEnd().Trim()
    if ($process.ExitCode -ne 0) {
        throw "$Runtime runner failed with exit code $($process.ExitCode): $standardError"
    }

    $rowCount = 0L
    if (-not [long]::TryParse($standardOutput, [ref]$rowCount)) {
        throw "$Runtime runner returned an invalid row count: $standardOutput"
    }

    [pscustomobject]@{
        Runtime = $Runtime
        Iteration = $Iteration
        Rows = $rowCount
        ElapsedMs = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 2)
        PeakWorkingSetMB = [Math]::Round($peakWorkingSet / 1MB, 2)
    }
}

function Get-Median {
    param([double[]]$Values)

    $sorted = @($Values | Sort-Object)
    $middle = [Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) {
        return $sorted[$middle]
    }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

$runners = @(
    @{
        Runtime = ".NET v1"
        Executable = "dotnet"
        Arguments = @($dotnetRunner, $Workbook, $Passes.ToString())
    },
    @{
        Runtime = "Rust"
        Executable = $rustRunner
        Arguments = @($Workbook, $Passes.ToString())
    }
)

foreach ($runner in $runners) {
    $null = Invoke-MeasuredProcess @runner -Iteration 0
}

$results = @()
foreach ($iteration in 1..$Iterations) {
    $orderedRunners = if ($iteration % 2 -eq 1) { $runners } else { @($runners[1], $runners[0]) }
    foreach ($runner in $orderedRunners) {
        $results += Invoke-MeasuredProcess @runner -Iteration $iteration
    }
}

$expectedRows = $results[0].Rows
if (@($results | Where-Object Rows -ne $expectedRows).Count -ne 0) {
    throw "The runners returned different row counts."
}

$summary = @($results | Group-Object Runtime | ForEach-Object {
    $medianElapsedMs = Get-Median ([double[]]$_.Group.ElapsedMs)
    $medianPeakWorkingSetMB = Get-Median ([double[]]$_.Group.PeakWorkingSetMB)
    [pscustomobject]@{
        Runtime = $_.Name
        MedianElapsedMs = [Math]::Round($medianElapsedMs, 2)
        RowsPerSecond = [Math]::Round($expectedRows / ($medianElapsedMs / 1000), 0)
        MedianPeakWorkingSetMB = [Math]::Round($medianPeakWorkingSetMB, 2)
        MaximumPeakWorkingSetMB = [Math]::Round(($_.Group.PeakWorkingSetMB | Measure-Object -Maximum).Maximum, 2)
    }
})

$dotnetSummary = $summary | Where-Object Runtime -eq ".NET v1"
$rustSummary = $summary | Where-Object Runtime -eq "Rust"
$comparison = [pscustomobject]@{
    RustSpeedup = [Math]::Round($dotnetSummary.MedianElapsedMs / $rustSummary.MedianElapsedMs, 2)
    RustPeakMemoryRatio = if ($dotnetSummary.MedianPeakWorkingSetMB -gt 0) {
        [Math]::Round($rustSummary.MedianPeakWorkingSetMB / $dotnetSummary.MedianPeakWorkingSetMB, 2)
    } else {
        $null
    }
}

$results | Sort-Object Iteration, Runtime | Format-Table -AutoSize
"Summary"
$summary | Format-Table -AutoSize
"Rust speed: $($comparison.RustSpeedup)x .NET v1; Rust peak memory: $($comparison.RustPeakMemoryRatio)x .NET v1"

if ([string]::IsNullOrWhiteSpace($OutputJson)) {
    $OutputJson = Join-Path $benchmarkRoot "dotnet-v1-vs-rust.json"
}
$OutputJson = [IO.Path]::GetFullPath($OutputJson)
New-Item -ItemType Directory -Force -Path (Split-Path $OutputJson -Parent) | Out-Null

$report = [ordered]@{
    TimestampUtc = [DateTime]::UtcNow.ToString("O")
    Machine = $env:COMPUTERNAME
    ProcessorCount = [Environment]::ProcessorCount
    Workbook = $Workbook
    WorkbookBytes = (Get-Item $Workbook).Length
    WorkbookSha256 = (Get-FileHash $Workbook -Algorithm SHA256).Hash
    Passes = $Passes
    Iterations = $Iterations
    RowsPerIteration = $expectedRows
    DotNetVersion = (& dotnet --version).Trim()
    DotNetMiniExcelVersion = $dotnetVersion
    DotNetRevision = $dotnetCommit
    RustVersion = (& rustc +1.85.0 --version).Trim()
    RustRevision = (& git -C $repositoryRoot rev-parse HEAD).Trim()
    Results = $results
    Summary = $summary
    Comparison = $comparison
}
$report | ConvertTo-Json -Depth 5 | Set-Content -Path $OutputJson -Encoding utf8
"JSON report: $OutputJson"