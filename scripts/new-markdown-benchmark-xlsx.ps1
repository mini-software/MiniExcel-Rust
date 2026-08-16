[CmdletBinding()]
param(
    [string]$Output = "target/markdown-comparison/synthetic.xlsx",
    [ValidateRange(1, 1000000)]
    [int]$Rows = 100000
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$outputPath = if ([IO.Path]::IsPathRooted($Output)) {
    $Output
} else {
    Join-Path $repoRoot $Output
}
$outputParent = Split-Path -Parent $outputPath
New-Item -ItemType Directory -Force -Path $outputParent | Out-Null
$staging = Join-Path $outputParent ".xlsx-$([Guid]::NewGuid().ToString('N'))"
$utf8 = [Text.UTF8Encoding]::new($false)

try {
    New-Item -ItemType Directory -Force -Path (Join-Path $staging "_rels") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $staging "xl/_rels") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $staging "xl/worksheets") | Out-Null

    [IO.File]::WriteAllText(
        (Join-Path $staging "[Content_Types].xml"),
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>' +
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
        '<Default Extension="xml" ContentType="application/xml"/>' +
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>' +
        '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>' +
        '</Types>',
        $utf8
    )
    [IO.File]::WriteAllText(
        (Join-Path $staging "_rels/.rels"),
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>' +
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>' +
        '</Relationships>',
        $utf8
    )
    [IO.File]::WriteAllText(
        (Join-Path $staging "xl/workbook.xml"),
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>' +
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">' +
        '<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets>' +
        '</workbook>',
        $utf8
    )
    [IO.File]::WriteAllText(
        (Join-Path $staging "xl/_rels/workbook.xml.rels"),
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>' +
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>' +
        '</Relationships>',
        $utf8
    )

    $sheetPath = Join-Path $staging "xl/worksheets/sheet1.xml"
    $writer = [IO.StreamWriter]::new($sheetPath, $false, $utf8, 1MB)
    try {
        $lastRow = $Rows + 1
        $writer.Write('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>')
        $writer.Write('<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">')
        $writer.Write("<dimension ref=`"A1:E$lastRow`"/><sheetData>")
        $writer.Write('<row r="1">')
        foreach ($header in @(@("A", "ID"), @("B", "Category"), @("C", "Region"), @("D", "Amount"), @("E", "Note"))) {
            $writer.Write("<c r=`"$($header[0])1`" t=`"inlineStr`"><is><t>$($header[1])</t></is></c>")
        }
        $writer.Write('</row>')

        $regions = @("North", "South", "East", "West")
        for ($index = 1; $index -le $Rows; $index++) {
            $row = $index + 1
            $category = $index % 10
            $region = $regions[$index % $regions.Count]
            $amount = ($index * 37) % 10000
            $writer.Write("<row r=`"$row`">")
            $writer.Write("<c r=`"A$row`"><v>$index</v></c>")
            $writer.Write("<c r=`"B$row`" t=`"inlineStr`"><is><t>Category $category</t></is></c>")
            $writer.Write("<c r=`"C$row`" t=`"inlineStr`"><is><t>$region</t></is></c>")
            $writer.Write("<c r=`"D$row`"><v>$amount</v></c>")
            $writer.Write("<c r=`"E$row`" t=`"inlineStr`"><is><t>Evidence row $index</t></is></c>")
            $writer.Write('</row>')
        }
        $writer.Write('</sheetData></worksheet>')
    } finally {
        $writer.Dispose()
    }

    Remove-Item $outputPath -Force -ErrorAction SilentlyContinue
    [IO.Compression.ZipFile]::CreateFromDirectory(
        $staging,
        $outputPath,
        [IO.Compression.CompressionLevel]::Optimal,
        $false
    )
    [pscustomobject]@{
        Path = $outputPath
        DataRows = $Rows
        XlsxBytes = (Get-Item $outputPath).Length
        WorksheetXmlBytes = (Get-Item $sheetPath).Length
    }
} finally {
    Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
}