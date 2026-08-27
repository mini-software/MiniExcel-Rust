[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^v\d+\.\d+\.0$')]
    [string]$Tag,

    [ValidatePattern('^v\d+\.\d+\.0$')]
    [string]$PreviousTag,

    [string]$OutputPath = 'RELEASE_NOTES.md',

    [string]$Repository = $env:GITHUB_REPOSITORY
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$env:LC_ALL = 'C'

function Invoke-Git {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)

    $result = @(& git @Arguments)
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
    return $result
}

function Resolve-Repository {
    param([string]$ConfiguredRepository)

    if (-not [string]::IsNullOrWhiteSpace($ConfiguredRepository)) {
        return $ConfiguredRepository
    }

    $remote = (@(Invoke-Git -Arguments @('remote', 'get-url', 'origin')))[0]
    if ($remote -notmatch 'github\.com[:/](?<repository>[^/]+/[^/.]+)(?:\.git)?$') {
        throw 'Set -Repository or GITHUB_REPOSITORY to an owner/repository value.'
    }
    return $Matches.repository
}

function Resolve-GitHubLogin {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryName,
        [Parameter(Mandatory = $true)][string]$Commit
    )

    $response = & gh api "repos/$RepositoryName/commits/$Commit" 2>$null
    if ($LASTEXITCODE -ne 0) {
        return $null
    }
    $commitDetails = $response | ConvertFrom-Json
    if ($null -eq $commitDetails.author) {
        return $null
    }
    return $commitDetails.author.login
}

$Repository = Resolve-Repository -ConfiguredRepository $Repository
$version = $Tag.Substring(1)
$releaseCommit = (@(Invoke-Git -Arguments @('rev-parse', "$Tag^{commit}")))[0].Trim()

if ([string]::IsNullOrWhiteSpace($PreviousTag)) {
    $PreviousTag = Invoke-Git -Arguments @('tag', '--merged', $releaseCommit, '--sort=-version:refname') |
        Where-Object { $_ -ne $Tag -and $_ -match '^v\d+\.\d+\.0$' } |
        Select-Object -First 1
}
if ([string]::IsNullOrWhiteSpace($PreviousTag)) {
    throw "No previous vN.N.0 tag was found before $Tag."
}

$range = "$PreviousTag..$Tag"
$shortStat = (@(Invoke-Git -Arguments @('diff', '--shortstat', $range)) -join '').Trim()
$fileCount = if ($shortStat -match '(\d+) files? changed') { [int]$Matches[1] } else { 0 }
$insertions = if ($shortStat -match '(\d+) insertions?\(\+\)') { [int]$Matches[1] } else { 0 }
$deletions = if ($shortStat -match '(\d+) deletions?\(-\)') { [int]$Matches[1] } else { 0 }

$commitLines = Invoke-Git -Arguments @('log', '--reverse', '--format=%H%x09%s', $range)
if ($commitLines.Count -eq 0) {
    throw "$range contains no commits."
}

$contributors = @{}
$changes = foreach ($line in $commitLines) {
    $parts = $line -split "`t", 2
    $commit = $parts[0]
    $subject = $parts[1]
    $shortCommit = $commit.Substring(0, 7)
    $login = Resolve-GitHubLogin -RepositoryName $Repository -Commit $commit
    if ([string]::IsNullOrWhiteSpace($login)) {
        $author = (@(Invoke-Git -Arguments @('show', '-s', '--format=%an', $commit)))[0]
        $key = "name:$author"
        $attribution = $author
        $contributor = $author
    } else {
        $key = "login:$login"
        $attribution = "@$login"
        $contributor = "[@$login](https://github.com/$login)"
    }
    if (-not $contributors.ContainsKey($key)) {
        $contributors[$key] = [pscustomobject]@{ Display = $contributor; Count = 0 }
    }
    $contributors[$key].Count++
    "- $subject by $attribution in [``$shortCommit``](https://github.com/$Repository/commit/$commit)."
}

$contributorLines = $contributors.Values |
    Sort-Object -Property Display |
    ForEach-Object {
        $noun = if ($_.Count -eq 1) { 'commit' } else { 'commits' }
        "- $($_.Display) contributed $($_.Count) $noun in this release."
    }

$browserAsset = "miniexcel-browser-lab-$Tag.zip"
$cliAsset = "miniexcel-cli-$Tag-windows-x64.zip"
$notes = @(
    '## Summary',
    '',
    "MiniExcel Rust $Tag contains $($commitLines.Count) commits across $fileCount files, with $insertions insertions and $deletions deletions. These notes are generated from the complete ``$range`` Git diff.",
    '',
    "Release commit: [``$($releaseCommit.Substring(0, 7))``](https://github.com/$Repository/commit/$releaseCommit)",
    '',
    "## What's Changed",
    ''
) + $changes + @(
    '',
    '## Contributors',
    ''
) + $contributorLines + @(
    '',
    '## GUI',
    '',
    'Open the hosted Browser Lab:',
    '',
    "- https://$($Repository.Split('/')[0]).github.io/$($Repository.Split('/')[1])/",
    '',
    'Download the static GUI package:',
    '',
    "- [$browserAsset](https://github.com/$Repository/releases/download/$Tag/$browserAsset)",
    '',
    '```powershell',
    "gh release download $Tag -R $Repository -p `"$browserAsset`"",
    '```',
    '',
    'Serve the extracted GUI over HTTP; browser modules and WebAssembly do not work correctly through `file://`.',
    '',
    '## CLI',
    '',
    'Download the Windows x64 CLI:',
    '',
    "- [$cliAsset](https://github.com/$Repository/releases/download/$Tag/$cliAsset)",
    '',
    '```powershell',
    "gh release download $Tag -R $Repository -p `"$cliAsset`"",
    "Expand-Archive $cliAsset",
    ".\$($cliAsset.Substring(0, $cliAsset.Length - 4))\miniexcel.exe --help",
    '```',
    '',
    '## Verification',
    '',
    'Download `SHA256SUMS.txt` with the assets and verify on Windows:',
    '',
    '```powershell',
    'Get-FileHash .\miniexcel-*.zip -Algorithm SHA256',
    '```',
    '',
    "Full Changelog: https://github.com/$Repository/compare/$PreviousTag...$Tag"
)

$parent = Split-Path -Parent $OutputPath
if (-not [string]::IsNullOrWhiteSpace($parent)) {
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
}
Set-Content -LiteralPath $OutputPath -Value ($notes -join "`n") -Encoding utf8NoBOM
Write-Host "Generated $OutputPath from $range."