[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $PackagePath,

    [string[]] $ExpectedRids = @(
        'win-x64',
        'win-arm64',
        'linux-x64',
        'linux-arm64',
        'linux-musl-x64',
        'linux-musl-arm64',
        'osx-x64',
        'osx-arm64'
    )
)

$ErrorActionPreference = 'Stop'
$nativeFiles = @{
    'win-x64' = 'miniexcel_ffi.dll'
    'win-arm64' = 'miniexcel_ffi.dll'
    'linux-x64' = 'libminiexcel_ffi.so'
    'linux-arm64' = 'libminiexcel_ffi.so'
    'linux-musl-x64' = 'libminiexcel_ffi.so'
    'linux-musl-arm64' = 'libminiexcel_ffi.so'
    'osx-x64' = 'libminiexcel_ffi.dylib'
    'osx-arm64' = 'libminiexcel_ffi.dylib'
}
$expectedNativeEntries = @($ExpectedRids | ForEach-Object {
    if (-not $nativeFiles.ContainsKey($_)) {
        throw "Unsupported RID '$_'."
    }
    "runtimes/$_/native/$($nativeFiles[$_])"
})

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $PackagePath))
try {
    $actualEntries = @($archive.Entries | ForEach-Object FullName)
    foreach ($entry in @(
        'lib/net8.0/MiniExcel.Rust.dll',
        'lib/netstandard2.0/MiniExcel.Rust.dll'
    ) + $expectedNativeEntries) {
        if ($entry -notin $actualEntries) {
            throw "The package is missing $entry."
        }
    }

    $unexpectedNativeEntries = @($actualEntries | Where-Object {
        $_ -like 'runtimes/*/native/*' -and $_ -notin $expectedNativeEntries
    })
    if ($unexpectedNativeEntries.Count -ne 0) {
        throw "The package contains unexpected native assets: $($unexpectedNativeEntries -join ', ')."
    }

    $nuspecEntry = $archive.Entries | Where-Object FullName -like '*.nuspec' | Select-Object -First 1
    $reader = [IO.StreamReader]::new($nuspecEntry.Open())
    try {
        [xml] $nuspec = $reader.ReadToEnd()
    }
    finally {
        $reader.Dispose()
    }
    $namespace = [Xml.XmlNamespaceManager]::new($nuspec.NameTable)
    $namespace.AddNamespace('n', $nuspec.DocumentElement.NamespaceURI)
    $packageId = $nuspec.SelectSingleNode('/n:package/n:metadata/n:id', $namespace).InnerText
    if ($packageId -ne 'MiniExcel.Rust') {
        throw "Unexpected package ID '$packageId'."
    }
    $dependencies = @($nuspec.SelectNodes('//n:dependency[@id="MiniExcel"]', $namespace))
    $invalidDependencies = @($dependencies | Where-Object version -ne '[1.46.0]')
    if ($dependencies.Count -ne 2 -or $invalidDependencies.Count -ne 0) {
        throw 'Every target framework must depend on MiniExcel [1.46.0].'
    }
    $jsonDependencies = @($nuspec.SelectNodes('//n:dependency[@id="System.Text.Json"]', $namespace))
    if ($jsonDependencies.Count -ne 0) {
        throw 'MiniExcel.Rust must not expose a System.Text.Json package dependency.'
    }
}
finally {
    $archive.Dispose()
}

Write-Host "Verified MiniExcel.Rust with $($ExpectedRids.Count) native asset(s) and the MiniExcel v1 dependency."
