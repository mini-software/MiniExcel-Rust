using MiniExcelLibs;

if (args.Length is < 1 or > 2)
{
    Console.Error.WriteLine("Usage: DotNetV1Query <xlsx-path> [passes]");
    return 2;
}

if (args.Length == 2 && (!int.TryParse(args[1], out var parsedPasses) || parsedPasses < 1))
{
    Console.Error.WriteLine("passes must be a positive integer");
    return 2;
}

var path = Path.GetFullPath(args[0]);
var passes = args.Length == 2 ? int.Parse(args[1]) : 1;
long rowCount = 0;

for (var pass = 0; pass < passes; pass++)
{
    foreach (var row in MiniExcel.Query(path, useHeaderRow: false))
    {
        _ = row;
        rowCount++;
    }
}

Console.WriteLine(rowCount);
return 0;