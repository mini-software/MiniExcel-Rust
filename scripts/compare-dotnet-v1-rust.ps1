param(
    [string]$DotNetRepository,
    [string]$DotNetRevision = "v1.x-maintenance",
    [string]$Workbook,
    [ValidateRange(1, 100)]
    [int]$Passes = 3,
    [ValidateRange(0, 100)]
    [int]$WarmupPasses = 1,
    [ValidateRange(1, 100)]
    [int]$Iterations = 5,
    [ValidateSet("Cold", "Steady", "Both")]
    [string]$Scenario = "Both",
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
        [string]$Scenario,
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

    try {
        $runnerResult = $standardOutput | ConvertFrom-Json
    } catch {
        throw "$Runtime runner returned invalid JSON: $standardOutput"
    }
    if ($null -eq $runnerResult.Rows -or $null -eq $runnerResult.Cells -or
        $null -eq $runnerResult.QueryElapsedMs) {
        throw "$Runtime runner result is missing required measurements: $standardOutput"
    }

    [pscustomobject]@{
        Runtime = $Runtime
        Scenario = $Scenario
        Iteration = $Iteration
        Rows = [long]$runnerResult.Rows
        Cells = [long]$runnerResult.Cells
        QueryElapsedMs = [Math]::Round([double]$runnerResult.QueryElapsedMs, 2)
        ProcessElapsedMs = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 2)
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
        BaseArguments = @($dotnetRunner, $Workbook)
    },
    @{
        Runtime = "Rust"
        Executable = $rustRunner
        BaseArguments = @($Workbook)
    }
)

foreach ($runner in $runners) {
    $arguments = @($runner.BaseArguments) + @("1", "0")
    $null = Invoke-MeasuredProcess -Runtime $runner.Runtime -Scenario "Preflight" `
        -Executable $runner.Executable -Arguments $arguments -Iteration 0
}

$scenarios = @()
if ($Scenario -in @("Cold", "Both")) {
    $scenarios += [pscustomobject]@{
        Name = "Cold"
        MeasuredPasses = 1
        WarmupPasses = 0
    }
}
if ($Scenario -in @("Steady", "Both")) {
    $scenarios += [pscustomobject]@{
        Name = "Steady"
        MeasuredPasses = $Passes
        WarmupPasses = $WarmupPasses
    }
}

$results = @()
for ($scenarioIndex = 0; $scenarioIndex -lt $scenarios.Count; $scenarioIndex++) {
    $benchmarkScenario = $scenarios[$scenarioIndex]
    foreach ($iteration in 1..$Iterations) {
        $dotnetFirst = ($iteration + $scenarioIndex) % 2 -eq 1
        $orderedRunners = if ($dotnetFirst) { $runners } else { @($runners[1], $runners[0]) }
        foreach ($runner in $orderedRunners) {
            $arguments = @($runner.BaseArguments) + @(
                $benchmarkScenario.MeasuredPasses.ToString(),
                $benchmarkScenario.WarmupPasses.ToString()
            )
            $results += Invoke-MeasuredProcess -Runtime $runner.Runtime `
                -Scenario $benchmarkScenario.Name -Executable $runner.Executable `
                -Arguments $arguments -Iteration $iteration
        }
    }
}

foreach ($benchmarkScenario in $scenarios) {
    $scenarioResults = @($results | Where-Object Scenario -eq $benchmarkScenario.Name)
    $expectedRows = $scenarioResults[0].Rows
    $expectedCells = $scenarioResults[0].Cells
    if (@($scenarioResults | Where-Object { $_.Rows -ne $expectedRows }).Count -ne 0) {
        throw "The runners returned different row counts for $($benchmarkScenario.Name)."
    }
    if (@($scenarioResults | Where-Object { $_.Cells -ne $expectedCells }).Count -ne 0) {
        throw "The runners returned different cell counts for $($benchmarkScenario.Name)."
    }
}

$summary = @(
    foreach ($benchmarkScenario in $scenarios) {
        foreach ($runtime in @(".NET v1", "Rust")) {
            $group = @($results | Where-Object {
                $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq $runtime
            })
            $medianQueryElapsedMs = Get-Median ([double[]]$group.QueryElapsedMs)
            $medianProcessElapsedMs = Get-Median ([double[]]$group.ProcessElapsedMs)
            $medianPeakWorkingSetMB = Get-Median ([double[]]$group.PeakWorkingSetMB)
            [pscustomobject]@{
                Scenario = $benchmarkScenario.Name
                Runtime = $runtime
                MeasuredPasses = $benchmarkScenario.MeasuredPasses
                WarmupPasses = $benchmarkScenario.WarmupPasses
                Rows = $group[0].Rows
                Cells = $group[0].Cells
                MedianQueryElapsedMs = [Math]::Round($medianQueryElapsedMs, 2)
                RowsPerSecond = [Math]::Round($group[0].Rows / ($medianQueryElapsedMs / 1000), 0)
                MedianProcessElapsedMs = [Math]::Round($medianProcessElapsedMs, 2)
                MedianPeakWorkingSetMB = [Math]::Round($medianPeakWorkingSetMB, 2)
                MaximumPeakWorkingSetMB = [Math]::Round(($group.PeakWorkingSetMB | Measure-Object -Maximum).Maximum, 2)
            }
        }
    }
)

$comparison = @(
    foreach ($benchmarkScenario in $scenarios) {
        $dotnetSummary = $summary | Where-Object {
            $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq ".NET v1"
        }
        $rustSummary = $summary | Where-Object {
            $_.Scenario -eq $benchmarkScenario.Name -and $_.Runtime -eq "Rust"
        }
        [pscustomobject]@{
            Scenario = $benchmarkScenario.Name
            RustQueryThroughputRatio = [Math]::Round(
                $dotnetSummary.MedianQueryElapsedMs / $rustSummary.MedianQueryElapsedMs,
                2
            )
            RustPeakMemoryRatio = if ($dotnetSummary.MedianPeakWorkingSetMB -gt 0) {
                [Math]::Round(
                    $rustSummary.MedianPeakWorkingSetMB / $dotnetSummary.MedianPeakWorkingSetMB,
                    2
                )
            } else {
                $null
            }
        }
    }
)

foreach ($benchmarkScenario in $scenarios) {
    "Scenario: $($benchmarkScenario.Name)"
    $results | Where-Object Scenario -eq $benchmarkScenario.Name |
        Sort-Object Iteration, Runtime |
        Select-Object Runtime, Iteration, Rows, Cells,
            @{ Name = "QueryMs"; Expression = { $_.QueryElapsedMs } },
            @{ Name = "ProcessMs"; Expression = { $_.ProcessElapsedMs } },
            @{ Name = "PeakMB"; Expression = { $_.PeakWorkingSetMB } } |
        Format-Table -AutoSize
    "Summary: $($benchmarkScenario.Name)"
    $summary | Where-Object Scenario -eq $benchmarkScenario.Name |
        Select-Object Runtime,
            @{ Name = "MedianQueryMs"; Expression = { $_.MedianQueryElapsedMs } },
            @{ Name = "RowsPerSec"; Expression = { $_.RowsPerSecond } },
            @{ Name = "MedianPeakMB"; Expression = { $_.MedianPeakWorkingSetMB } },
            @{ Name = "MaxPeakMB"; Expression = { $_.MaximumPeakWorkingSetMB } } |
        Format-Table -AutoSize
    $scenarioComparison = $comparison | Where-Object Scenario -eq $benchmarkScenario.Name
    "Rust query throughput: $($scenarioComparison.RustQueryThroughputRatio)x .NET v1; Rust peak memory: $($scenarioComparison.RustPeakMemoryRatio)x .NET v1"
}

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
    Iterations = $Iterations
    RequestedScenario = $Scenario
    Scenarios = $scenarios
    DotNetVersion = (& dotnet --version).Trim()
    DotNetMiniExcelVersion = $dotnetVersion
    DotNetRevision = $dotnetCommit
    RustVersion = (& rustc +1.85.0 --version).Trim()
    RustRevision = (& git -C $repositoryRoot rev-parse HEAD).Trim()
    TimingScope = "Runner-internal query loop; process startup excluded; JIT included only without warm-up"
    MemoryScope = "Peak process working set sampled approximately every 10 ms, including warm-up"
    Results = $results
    Summary = $summary
    Comparison = $comparison
}
$report | ConvertTo-Json -Depth 5 | Set-Content -Path $OutputJson -Encoding utf8
"JSON report: $OutputJson"