[CmdletBinding()]
param(
    [ValidateSet('win-x64', 'win-arm64', 'linux-x64', 'linux-arm64', 'linux-musl-x64', 'linux-musl-arm64', 'osx-x64', 'osx-arm64')]
    [string] $Rid = 'win-x64',

    [string] $Version = '0.1.0-dev',

    [switch] $SkipNativeBuild
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$packageDirectory = Join-Path $repositoryRoot 'target/nuget/packages'
$packageCache = Join-Path $repositoryRoot 'target/nuget/packages-cache'
$restoreDirectory = Join-Path $repositoryRoot 'target/nuget/restore'
$restoreConfig = Join-Path $restoreDirectory 'NuGet.Config'
$consumerProject = Join-Path $repositoryRoot 'dotnet/tests/MiniExcel.Rust.PackageTests/MiniExcel.Rust.PackageTests.csproj'

if (-not $SkipNativeBuild) {
    & (Join-Path $PSScriptRoot 'Build-Native.ps1') -Rid $Rid
}

New-Item -ItemType Directory -Path $packageDirectory -Force | Out-Null
& dotnet pack (Join-Path $repositoryRoot 'dotnet/src/MiniExcel.Rust/MiniExcel.Rust.csproj') `
    -c Release `
    -o $packageDirectory `
    -p:PackageVersion=$Version `
    -p:MiniExcelRustRequireAllNativeAssets=false
if ($LASTEXITCODE -ne 0) {
    throw 'NuGet pack failed.'
}

$package = Join-Path $packageDirectory "MiniExcel.Rust.$Version.nupkg"
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($package)
try {
    $nativeEntry = $archive.Entries | Where-Object { $_.FullName -like "runtimes/$Rid/native/*" }
    if ($null -eq $nativeEntry) {
        throw "The package does not contain a native asset for $Rid."
    }
}
finally {
    $archive.Dispose()
}

$cachedPackage = Join-Path $packageCache "miniexcel.rust/$($Version.ToLowerInvariant())"
if (Test-Path $cachedPackage) {
    Remove-Item $cachedPackage -Recurse -Force
}
& dotnet new nugetconfig --output $restoreDirectory --force | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw 'NuGet configuration creation failed.'
}
& dotnet nuget add source $packageDirectory --name MiniExcelRustLocal --configfile $restoreConfig | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw 'Local NuGet source configuration failed.'
}
& dotnet restore $consumerProject `
    --force `
    --no-cache `
    --packages $packageCache `
    --configfile $restoreConfig `
    -p:MiniExcelRustPackageVersion=$Version
if ($LASTEXITCODE -ne 0) {
    throw 'Package consumer restore failed.'
}

& dotnet run --project $consumerProject -c Release --no-restore `
    -p:MiniExcelRustPackageVersion=$Version
if ($LASTEXITCODE -ne 0) {
    throw 'Package consumer smoke test failed.'
}
