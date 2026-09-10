using System.Diagnostics;
using System.Globalization;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using MiniExcelLibs;
using ManagedMiniExcel = MiniExcelLibs.MiniExcel;

if (args.Length == 0)
    return Usage();

return args[0].ToLowerInvariant() switch
{
    "generate" => Generate(args),
    "generate-template" => GenerateTemplate(args),
    "verify" => Verify(args),
    "fingerprint" => Fingerprint(args),
    "managed" or "managed-query" => BenchmarkQuery(args, useRust: false, firstOnly: false),
    "rust-dotnet" or "rust-dotnet-query" => BenchmarkQuery(args, useRust: true, firstOnly: false),
    "managed-query-first" => BenchmarkQuery(args, useRust: false, firstOnly: true),
    "rust-dotnet-query-first" => BenchmarkQuery(args, useRust: true, firstOnly: true),
    "managed-create" => BenchmarkCreate(args, useRust: false),
    "rust-dotnet-create" => BenchmarkCreate(args, useRust: true),
    "managed-template" => BenchmarkTemplate(args, useRust: false),
    "rust-dotnet-template" => BenchmarkTemplate(args, useRust: true),
    _ => Usage()
};

static int Generate(string[] arguments)
{
    if (arguments.Length != 4 ||
        !int.TryParse(arguments[2], out var rowCount) || rowCount < 1 ||
        !int.TryParse(arguments[3], out var columnCount) || columnCount is < 1 or > 26)
        return Usage();

    CreateWorkbook(Path.GetFullPath(arguments[1]), rowCount, columnCount);
    return 0;
}

static int Verify(string[] arguments)
{
    if (arguments.Length != 2)
        return Usage();

    var path = Path.GetFullPath(arguments[1]);
    using var managed = Query(path, useRust: false).GetEnumerator();
    using var rust = Query(path, useRust: true).GetEnumerator();
    long rowIndex = 0;
    while (true)
    {
        var hasManaged = managed.MoveNext();
        var hasRust = rust.MoveNext();
        Require(hasManaged == hasRust, $"Row count differs after row {rowIndex}.");
        if (!hasManaged)
            break;
        CompareRows(managed.Current, rust.Current, rowIndex);
        rowIndex++;
    }

    Console.WriteLine($"Verified {rowIndex} rows against MiniExcel 1.46.0.");
    return 0;
}

static int GenerateTemplate(string[] arguments)
{
    if (arguments.Length != 2)
        return Usage();
    var path = Path.GetFullPath(arguments[1]);
    if (File.Exists(path))
        File.Delete(path);
    MiniExcelRust.SaveAs(
        path,
        new[]
        {
            new Dictionary<string, object?> { ["Name"] = "Name", ["Department"] = "Department" },
            new Dictionary<string, object?> { ["Name"] = "{{employees.name}}", ["Department"] = "{{employees.department}}" }
        },
        printHeader: false);
    return 0;
}

static int Fingerprint(string[] arguments)
{
    if (arguments.Length != 2)
        return Usage();
    var (rows, cells, contentHash) = FingerprintRows(MiniExcelRust.Query(Path.GetFullPath(arguments[1])));
    Console.WriteLine(JsonSerializer.Serialize(new { Rows = rows, Cells = cells, ContentHash = contentHash }));
    return 0;
}

static int BenchmarkQuery(string[] arguments, bool useRust, bool firstOnly)
{
    if (arguments.Length is < 2 or > 4 ||
        arguments.Length >= 3 && (!int.TryParse(arguments[2], out var passes) || passes < 1) ||
        arguments.Length >= 4 && (!int.TryParse(arguments[3], out var warmups) || warmups < 0))
        return Usage();

    var path = Path.GetFullPath(arguments[1]);
    var measuredPasses = arguments.Length >= 3 ? int.Parse(arguments[2], CultureInfo.InvariantCulture) : 1;
    var warmupPasses = arguments.Length >= 4 ? int.Parse(arguments[3], CultureInfo.InvariantCulture) : 0;
    for (var pass = 0; pass < warmupPasses; pass++)
        Consume(path, useRust, firstOnly);

    GC.Collect();
    GC.WaitForPendingFinalizers();
    GC.Collect();
    var allocatedBefore = GC.GetTotalAllocatedBytes(precise: true);
    var stopwatch = Stopwatch.StartNew();
    var firstRowMilliseconds = 0d;
    long rows = 0;
    long cells = 0;
    using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
    for (var pass = 0; pass < measuredPasses; pass++)
    {
        foreach (var row in Query(path, useRust))
        {
            if (rows == 0)
                firstRowMilliseconds = stopwatch.Elapsed.TotalMilliseconds;
            rows++;
            cells += row.Count;
            AppendRow(hash, row);
            if (firstOnly)
                break;
        }
    }
    stopwatch.Stop();

    Console.WriteLine(JsonSerializer.Serialize(new BenchmarkResult(
        firstOnly ? "QueryFirst" : "Query",
        useRust ? "MiniExcel.Rust (.NET)" : "MiniExcel",
        Environment.Version.ToString(),
        measuredPasses,
        rows,
        cells,
        Convert.ToHexString(hash.GetHashAndReset()),
        null,
        stopwatch.Elapsed.TotalMilliseconds,
        firstRowMilliseconds,
        GC.GetTotalAllocatedBytes(precise: true) - allocatedBefore)));
    return 0;
}

static int BenchmarkCreate(string[] arguments, bool useRust)
{
    if (arguments.Length != 6 ||
        !int.TryParse(arguments[2], out var rows) || rows < 1 ||
        !int.TryParse(arguments[3], out var columns) || columns is < 1 or > 26 ||
        !int.TryParse(arguments[4], out var passes) || passes < 1 ||
        !int.TryParse(arguments[5], out var warmups) || warmups < 0)
        return Usage();
    var outputPath = Path.GetFullPath(arguments[1]);
    var values = CreateRows(rows, columns);
    for (var pass = 0; pass < warmups; pass++)
        WriteWorkbook(outputPath, values, useRust);

    ForceCollection();
    var allocatedBefore = GC.GetTotalAllocatedBytes(precise: true);
    var stopwatch = Stopwatch.StartNew();
    for (var pass = 0; pass < passes; pass++)
        WriteWorkbook(outputPath, values, useRust);
    stopwatch.Stop();
    Console.WriteLine(JsonSerializer.Serialize(new BenchmarkResult(
        "Create",
        useRust ? "MiniExcel.Rust (.NET)" : "MiniExcel",
        Environment.Version.ToString(),
        passes,
        (long)rows * passes,
        (long)rows * columns * passes,
        null,
        outputPath,
        stopwatch.Elapsed.TotalMilliseconds,
        null,
        GC.GetTotalAllocatedBytes(precise: true) - allocatedBefore)));
    return 0;
}

static int BenchmarkTemplate(string[] arguments, bool useRust)
{
    if (arguments.Length != 6 ||
        !int.TryParse(arguments[3], out var rows) || rows < 1 ||
        !int.TryParse(arguments[4], out var passes) || passes < 1 ||
        !int.TryParse(arguments[5], out var warmups) || warmups < 0)
        return Usage();
    var templatePath = Path.GetFullPath(arguments[1]);
    var outputPath = Path.GetFullPath(arguments[2]);
    var value = new
    {
        employees = Enumerable.Range(1, rows)
            .Select(_ => new { name = "Jack", department = "HR" })
            .ToArray()
    };
    for (var pass = 0; pass < warmups; pass++)
        FillTemplate(outputPath, templatePath, value, useRust);

    ForceCollection();
    var allocatedBefore = GC.GetTotalAllocatedBytes(precise: true);
    var stopwatch = Stopwatch.StartNew();
    for (var pass = 0; pass < passes; pass++)
        FillTemplate(outputPath, templatePath, value, useRust);
    stopwatch.Stop();
    Console.WriteLine(JsonSerializer.Serialize(new BenchmarkResult(
        "Template",
        useRust ? "MiniExcel.Rust (.NET)" : "MiniExcel",
        Environment.Version.ToString(),
        passes,
        (long)rows * passes,
        (long)rows * 2 * passes,
        null,
        outputPath,
        stopwatch.Elapsed.TotalMilliseconds,
        null,
        GC.GetTotalAllocatedBytes(precise: true) - allocatedBefore)));
    return 0;
}

static IEnumerable<IDictionary<string, object?>> Query(string path, bool useRust)
{
    if (useRust)
        return MiniExcelRust.Query(path, useHeaderRow: false);
    return ManagedMiniExcel.Query(path, useHeaderRow: false)
        .Cast<IDictionary<string, object?>>();
}

static void Consume(string path, bool useRust, bool firstOnly)
{
    foreach (var row in Query(path, useRust))
    {
        _ = row.Count;
        if (firstOnly)
            break;
    }
}

static List<IDictionary<string, object?>> CreateRows(int rows, int columns) =>
    Enumerable.Range(1, rows)
        .Select(_ => (IDictionary<string, object?>)Enumerable.Range(1, columns)
            .ToDictionary(column => $"Column{column}", _ => (object?)"Hello World"))
        .ToList();

static void WriteWorkbook(
    string path,
    IEnumerable<IDictionary<string, object?>> values,
    bool useRust)
{
    if (File.Exists(path))
        File.Delete(path);
    if (useRust)
        MiniExcelRust.SaveAs(path, values);
    else
        ManagedMiniExcel.SaveAs(path, values);
}

static void FillTemplate(string outputPath, string templatePath, object value, bool useRust)
{
    if (File.Exists(outputPath))
        File.Delete(outputPath);
    if (useRust)
        MiniExcelRust.FillTemplate(outputPath, templatePath, value);
    else
        ManagedMiniExcel.SaveAsByTemplate(outputPath, templatePath, value);
}

static void ForceCollection()
{
    GC.Collect();
    GC.WaitForPendingFinalizers();
    GC.Collect();
}

static void CompareRows(
    IDictionary<string, object?> managed,
    IDictionary<string, object?> rust,
    long rowIndex)
{
    Require(managed.Keys.SequenceEqual(rust.Keys, StringComparer.Ordinal),
        $"Column order differs at row {rowIndex}.");
    foreach (var key in managed.Keys)
    {
        Require(Normalize(managed[key]) == Normalize(rust[key]),
            $"Value differs at row {rowIndex}, column {key}: managed={managed[key]}, rust={rust[key]}.");
    }
}

static void AppendRow(IncrementalHash hash, IDictionary<string, object?> row)
{
    foreach (var cell in row)
    {
        AppendText(hash, cell.Key);
        AppendText(hash, Normalize(cell.Value));
    }
}

static (long Rows, long Cells, string ContentHash) FingerprintRows(
    IEnumerable<IDictionary<string, object?>> values)
{
    long rows = 0;
    long cells = 0;
    using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
    foreach (var row in values)
    {
        rows++;
        cells += row.Count;
        AppendRow(hash, row);
    }
    return (rows, cells, Convert.ToHexString(hash.GetHashAndReset()));
}

static void AppendText(IncrementalHash hash, string value)
{
    var bytes = Encoding.UTF8.GetBytes(value);
    hash.AppendData(BitConverter.GetBytes(bytes.Length));
    hash.AppendData(bytes);
}

static string Normalize(object? value) => value switch
{
    null or DBNull => "null",
    IFormattable formattable => formattable.ToString(null, CultureInfo.InvariantCulture) ?? string.Empty,
    _ => value.ToString() ?? string.Empty
};

static void CreateWorkbook(string path, int rows, int columns)
{
    Directory.CreateDirectory(Path.GetDirectoryName(path)!);
    if (File.Exists(path))
        File.Delete(path);
    using var archive = ZipFile.Open(path, ZipArchiveMode.Create);
    AddEntry(archive, "[Content_Types].xml", """
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
          <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
          <Default Extension="xml" ContentType="application/xml"/>
          <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
          <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
        </Types>
        """);
    AddEntry(archive, "_rels/.rels", """
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
          <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
        </Relationships>
        """);
    AddEntry(archive, "xl/workbook.xml", """
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
          <sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
        </workbook>
        """);
    AddEntry(archive, "xl/_rels/workbook.xml.rels", """
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
          <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
        </Relationships>
        """);

    var entry = archive.CreateEntry("xl/worksheets/sheet1.xml", CompressionLevel.Fastest);
    using var writer = new StreamWriter(entry.Open(), new UTF8Encoding(false));
    writer.Write("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>");
    for (var row = 1; row <= rows; row++)
    {
        writer.Write($"<row r=\"{row}\">");
        for (var column = 1; column <= columns; column++)
        {
            var reference = $"{(char)('A' + column - 1)}{row}";
            var value = (long)(row - 1) * columns + column;
            writer.Write($"<c r=\"{reference}\"><v>{value}</v></c>");
        }
        writer.Write("</row>");
    }
    writer.Write("</sheetData></worksheet>");
}

static void AddEntry(ZipArchive archive, string name, string contents)
{
    var entry = archive.CreateEntry(name, CompressionLevel.Fastest);
    using var writer = new StreamWriter(entry.Open(), new UTF8Encoding(false));
    writer.Write(contents);
}

static void Require(bool condition, string message)
{
    if (!condition)
        throw new InvalidOperationException(message);
}

static int Usage()
{
    Console.Error.WriteLine("Usage:");
    Console.Error.WriteLine("  NuGetV1Query generate <xlsx-path> <rows> <columns>");
    Console.Error.WriteLine("  NuGetV1Query generate-template <xlsx-path>");
    Console.Error.WriteLine("  NuGetV1Query verify <xlsx-path>");
    Console.Error.WriteLine("  NuGetV1Query fingerprint <xlsx-path>");
    Console.Error.WriteLine("  NuGetV1Query <managed|rust-dotnet>-<query|query-first> <xlsx-path> [passes] [warmups]");
    Console.Error.WriteLine("  NuGetV1Query <managed|rust-dotnet>-create <output-path> <rows> <columns> <passes> <warmups>");
    Console.Error.WriteLine("  NuGetV1Query <managed|rust-dotnet>-template <template-path> <output-path> <rows> <passes> <warmups>");
    return 2;
}

internal sealed record BenchmarkResult(
    string Method,
    string Runtime,
    string DotNetRuntime,
    int Passes,
    long Rows,
    long Cells,
    string? ContentHash,
    string? OutputPath,
    double ElapsedMilliseconds,
    double? FirstRowMilliseconds,
    long? AllocatedBytes);