namespace MiniExcelLibs;

public enum MiniExcelRustSheetType : byte
{
    Worksheet,
    DialogSheet,
    MacroSheet,
    ChartSheet,
    Vba
}

public enum MiniExcelRustSheetState : byte
{
    Visible,
    Hidden,
    VeryHidden
}

/// <summary>
/// Describes an XLSX sheet in workbook order.
/// </summary>
public sealed class MiniExcelRustSheetInfo
{
    internal MiniExcelRustSheetInfo(
        uint id,
        uint index,
        string name,
        MiniExcelRustSheetType sheetType,
        MiniExcelRustSheetState state,
        bool active)
    {
        Id = id;
        Index = index;
        Name = name;
        SheetType = sheetType;
        State = state;
        Active = active;
    }

    public uint Id { get; }

    public uint Index { get; }

    public string Name { get; }

    public MiniExcelRustSheetType SheetType { get; }

    public MiniExcelRustSheetState State { get; }

    public bool Active { get; }
}