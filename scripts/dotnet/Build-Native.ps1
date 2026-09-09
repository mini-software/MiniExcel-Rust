[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet(
        'win-x64',
        'win-arm64',
        'linux-x64',
        'linux-arm64',
        'linux-musl-x64',
        'linux-musl-arm64',
        'osx-x64',
        'osx-arm64'
    )]
    [string] $Rid,

    [switch] $UseZig,

    [string] $Toolchain
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent

$targets = @{
    'win-x64' = @{ Triple = 'x86_64-pc-windows-msvc'; File = 'miniexcel_ffi.dll' }
    'win-arm64' = @{ Triple = 'aarch64-pc-windows-msvc'; File = 'miniexcel_ffi.dll' }
    'linux-x64' = @{ Triple = 'x86_64-unknown-linux-gnu'; File = 'libminiexcel_ffi.so' }
    'linux-arm64' = @{ Triple = 'aarch64-unknown-linux-gnu'; File = 'libminiexcel_ffi.so' }
    'linux-musl-x64' = @{ Triple = 'x86_64-unknown-linux-musl'; File = 'libminiexcel_ffi.so' }
    'linux-musl-arm64' = @{ Triple = 'aarch64-unknown-linux-musl'; File = 'libminiexcel_ffi.so' }
    'osx-x64' = @{ Triple = 'x86_64-apple-darwin'; File = 'libminiexcel_ffi.dylib' }
    'osx-arm64' = @{ Triple = 'aarch64-apple-darwin'; File = 'libminiexcel_ffi.dylib' }
}

$target = $targets[$Rid]
$originalRustFlags = $env:RUSTFLAGS
Push-Location $repositoryRoot
try {
    if (-not [string]::IsNullOrWhiteSpace($Toolchain)) {
        & rustup target add --toolchain $Toolchain $target.Triple
    }
    else {
        & rustup target add $target.Triple
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to install Rust target $($target.Triple)."
    }

    $buildArguments = @(
        $(if ($UseZig) { 'zigbuild' } else { 'build' }),
        '--release',
        '--locked',
        '-p',
        'miniexcel-ffi',
        '--target',
        $target.Triple
    )

    if ($UseZig) {
        $env:RUSTFLAGS = "$originalRustFlags -C target-feature=-crt-static".Trim()
    }

    if (-not [string]::IsNullOrWhiteSpace($Toolchain)) {
        & rustup run $Toolchain cargo @buildArguments
    }
    else {
        & cargo @buildArguments
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to build native library for $Rid."
    }

    $source = Join-Path $repositoryRoot "target/$($target.Triple)/release/$($target.File)"
    $destinationDirectory = Join-Path $repositoryRoot "target/nuget/native/$Rid"
    New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
    Copy-Item $source (Join-Path $destinationDirectory $target.File) -Force
}
finally {
    $env:RUSTFLAGS = $originalRustFlags
    Pop-Location
}
