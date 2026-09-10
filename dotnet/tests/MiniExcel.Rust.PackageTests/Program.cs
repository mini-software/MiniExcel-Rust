using MiniExcelLibs;
using MiniExcelLibs.Attributes;
using MiniExcelLibs.Csv;
using MiniExcelLibs.OpenXml;

var temporaryDirectory = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-package-{Guid.NewGuid():N}");
Directory.CreateDirectory(temporaryDirectory);
try
{
    var rustAssembly = typeof(MiniExcelRust).Assembly;
    Require(rustAssembly.GetType("MiniExcelLibs.MiniExcel") is null, "MiniExcel.Rust must not duplicate the MiniExcel facade.");
    Require(rustAssembly.GetType("MiniExcelLibs.IConfiguration") is null, "MiniExcel.Rust must reuse MiniExcel v1 configuration types.");
    Require(typeof(IConfiguration).Assembly != rustAssembly, "IConfiguration must come from the MiniExcel dependency.");

    var inputPath = Path.Combine(temporaryDirectory, "input.xlsx");
    global::MiniExcelLibs.MiniExcel.SaveAs(
        inputPath,
        new[]
        {
            new Dictionary<string, object?>
            {
                ["Display Name"] = "Ada",
                ["Score"] = 42
            }
        });

    var readConfiguration = new OpenXmlConfiguration
    {
        IgnoreEmptyRows = true,
        FillMergedCells = true,
        TrimColumnNames = true
    };
    var row = MiniExcelRust.Query(
        inputPath,
        useHeaderRow: true,
        configuration: readConfiguration).Single();
    Require((string)row["Display Name"]! == "Ada", "Dynamic XLSX query did not return the expected name.");

    var typed = MiniExcelRust.Query<PackageRow>(
        inputPath,
        configuration: readConfiguration).Single();
    Require(typed.Name == "Ada" && typed.Score == 42, "MiniExcel v1 attributes were not honored by typed mapping.");

    var batchInputPath = Path.Combine(temporaryDirectory, "batch-input.xlsx");
    global::MiniExcelLibs.MiniExcel.SaveAs(
        batchInputPath,
        Enumerable.Range(1, 130)
            .Select(index => new Dictionary<string, object?>
            {
                ["Name"] = $"Row {index}",
                ["Score"] = index
            }));
    var batchRows = MiniExcelRust.Query(batchInputPath, useHeaderRow: true).ToList();
    Require(batchRows.Count == 130, "Multi-batch query returned an unexpected row count.");
    Require(batchRows[64]["Name"]?.ToString() == "Row 65", "Multi-batch query lost row order.");
    var firstColumnName = batchRows[0].Keys.First();
    Require(
        batchRows.All(batchRow => ReferenceEquals(firstColumnName, batchRow.Keys.First())),
        "Multi-batch query did not reuse managed column names.");

    var outputPath = Path.Combine(temporaryDirectory, "output.xlsx");
    var writeConfiguration = new OpenXmlConfiguration
    {
        AutoFilter = false,
        FreezeRowCount = 0,
        RightToLeft = true
    };
    var count = MiniExcelRust.SaveAs(
        outputPath,
        new[] { new PackageRow { Name = "Grace", Score = 84 } },
        writeConfiguration);
    Require(count == 1, "Rust-backed XLSX write returned an unexpected row count.");
    var roundTrip = global::MiniExcelLibs.MiniExcel.Query<PackageRow>(outputPath).Single();
    Require(roundTrip.Name == "Grace" && roundTrip.Score == 84, "MiniExcel v1 could not read the Rust-written workbook.");

    var asyncOutputPath = Path.Combine(temporaryDirectory, "async-output.xlsx");
    var asyncCount = await MiniExcelRust.SaveAsAsync(
        asyncOutputPath,
        AsyncRows(new PackageRow { Name = "Katherine", Score = 126 }));
    Require(asyncCount == 1, "Rust-backed async XLSX write returned an unexpected row count.");
    Require(
        global::MiniExcelLibs.MiniExcel.Query<PackageRow>(asyncOutputPath).Single().Name == "Katherine",
        "MiniExcel v1 could not read the async Rust-written workbook.");

    var csvPath = Path.Combine(temporaryDirectory, "input.csv");
    File.WriteAllText(csvPath, "Name;Score\r\nLinus;21\r\n");
    var csvConfiguration = new CsvConfiguration { Seperator = ';' };
    var csvRow = MiniExcelRust.QueryCsv(csvPath, useHeaderRow: true, configuration: csvConfiguration).Single();
    Require((string)csvRow["Name"]! == "Linus", "MiniExcel v1 CSV configuration was not applied.");

    var asyncCsvPath = Path.Combine(temporaryDirectory, "async-output.csv");
    var asyncCsvCount = await MiniExcelRust.SaveAsCsvAsync(
        asyncCsvPath,
        AsyncRows(new PackageRow { Name = "Margaret", Score = 168 }));
    Require(asyncCsvCount == 1, "Rust-backed async CSV write returned an unexpected row count.");
    Require(
        MiniExcelRust.QueryCsv(asyncCsvPath, useHeaderRow: true).Single()["Display Name"]?.ToString() == "Margaret",
        "Rust could not read the async Rust-written CSV file.");

    var templatePath = Path.Combine(temporaryDirectory, "template.xlsx");
    MiniExcelRust.SaveAs(
        templatePath,
        new[]
        {
            new Dictionary<string, object?>
            {
                ["A"] = "{{count}}",
                ["B"] = "{{enabled}}",
                ["C"] = "Value: {{text}}"
            },
            new Dictionary<string, object?>
            {
                ["A"] = "{{items.name}}",
                ["B"] = "{{items.score}}",
                ["C"] = null
            }
        },
        printHeader: false);
    var templateOutputPath = Path.Combine(temporaryDirectory, "template-output.xlsx");
    MiniExcelRust.FillTemplate(
        templateOutputPath,
        templatePath,
        new
        {
            count = 42,
            enabled = true,
            text = "binary",
            items = new[]
            {
                new { name = "Ada", score = 10 },
                new { name = "Linus", score = 20 }
            }
        });
    var templateRows = MiniExcelRust.Query(templateOutputPath).ToList();
    Require(Convert.ToDouble(templateRows[0]["A"]) == 42d, "Binary template number was not applied.");
    Require((bool)templateRows[0]["B"]!, "Binary template boolean was not applied.");
    Require(templateRows[0]["C"]?.ToString() == "Value: binary", "Binary template text was not applied.");
    Require(templateRows[1]["A"]?.ToString() == "Ada", "Binary template list item was not expanded.");
    Require(Convert.ToDouble(templateRows[2]["B"]) == 20d, "Binary template list value was not expanded.");

    var mappedOutputPath = Path.Combine(temporaryDirectory, "mapped-output.xlsx");
    var mapping = new MiniExcelRustMapping<PackageRow>().ToWorksheet("Sheet1");
    mapping.Property(row => row.Name).ToCell("A1");
    mapping.Property(row => row.Score).ToCell("B1");
    MiniExcelRustMappingExtensions.FillMappedTemplate(
        mappedOutputPath,
        templatePath,
        new[] { new PackageRow { Name = "Dorothy", Score = 210 } },
        mapping);
    var mappedRow = MiniExcelRust.Query(mappedOutputPath).First();
    Require(mappedRow["A"]?.ToString() == "Dorothy", "Binary mapped-template text was not applied.");
    Require(Convert.ToDouble(mappedRow["B"]) == 210d, "Binary mapped-template number was not applied.");

    Console.WriteLine("MiniExcel.Rust package smoke test passed.");
}
finally
{
    Directory.Delete(temporaryDirectory, recursive: true);
}

static void Require(bool condition, string message)
{
    if (!condition)
        throw new InvalidOperationException(message);
}

static async IAsyncEnumerable<T> AsyncRows<T>(params T[] rows)
{
    foreach (var row in rows)
    {
        yield return row;
        await Task.Yield();
    }
}

internal sealed class PackageRow
{
    [ExcelColumn(Name = "Display Name")]
    public string Name { get; set; } = string.Empty;

    public int Score { get; set; }
}
