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

    var csvPath = Path.Combine(temporaryDirectory, "input.csv");
    File.WriteAllText(csvPath, "Name;Score\r\nLinus;21\r\n");
    var csvConfiguration = new CsvConfiguration { Seperator = ';' };
    var csvRow = MiniExcelRust.QueryCsv(csvPath, useHeaderRow: true, configuration: csvConfiguration).Single();
    Require((string)csvRow["Name"]! == "Linus", "MiniExcel v1 CSV configuration was not applied.");

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

internal sealed class PackageRow
{
    [ExcelColumn(Name = "Display Name")]
    public string Name { get; set; } = string.Empty;

    public int Score { get; set; }
}
