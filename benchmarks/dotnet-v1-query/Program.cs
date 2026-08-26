using MiniExcelLibs;
using System.Diagnostics;
using System.Text.Json;

if (args.Length is < 1 or > 3)
{
    Console.Error.WriteLine("Usage: DotNetV1Query <xlsx-path> [measured-passes] [warmup-passes]");
    return 2;
}

if (args.Length >= 2 && (!int.TryParse(args[1], out var measuredPasses) || measuredPasses < 1))
{
    Console.Error.WriteLine("measured-passes must be a positive integer");
    return 2;
}

if (args.Length == 3 && (!int.TryParse(args[2], out var warmupPasses) || warmupPasses < 0))
{
    Console.Error.WriteLine("warmup-passes must be a non-negative integer");
    return 2;
}

var path = Path.GetFullPath(args[0]);
var measured = args.Length >= 2 ? int.Parse(args[1]) : 1;
var warmup = args.Length == 3 ? int.Parse(args[2]) : 0;

RunQuery(path, warmup);
GC.Collect();
GC.WaitForPendingFinalizers();
GC.Collect();

var stopwatch = Stopwatch.StartNew();
var (rows, cells) = RunQuery(path, measured);
stopwatch.Stop();

Console.WriteLine(JsonSerializer.Serialize(new
{
    Rows = rows,
    Cells = cells,
    QueryElapsedMs = stopwatch.Elapsed.TotalMilliseconds
}));
return 0;

static (long Rows, long Cells) RunQuery(string path, int passes)
{
    long rows = 0;
    long cells = 0;
    for (var pass = 0; pass < passes; pass++)
    {
        foreach (object row in MiniExcel.Query(path, useHeaderRow: false))
        {
            rows++;
            cells += ((IDictionary<string, object>)row).Count;
        }
    }
    return (rows, cells);
}