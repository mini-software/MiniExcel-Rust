namespace MiniExcelLibs;

/// <summary>
/// Represents the used A1 range of an XLSX worksheet.
/// </summary>
public sealed class MiniExcelRustRange
{
    internal MiniExcelRustRange(string? startCell, string? endCell)
    {
        StartCell = startCell;
        EndCell = endCell;
    }

    public string? StartCell { get; }

    public string? EndCell { get; }
}