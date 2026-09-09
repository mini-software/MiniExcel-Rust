namespace MiniExcelLibs;

public enum MiniExcelRustPictureAnchor : byte
{
    OneCell,
    Absolute,
    TwoCell
}

public sealed class MiniExcelRustPicture
{
    public byte[] ImageBytes { get; set; } = Array.Empty<byte>();
    public string? SheetName { get; set; }
    public string CellAddress { get; set; } = "A1";
    public int WidthPx { get; set; } = 80;
    public int HeightPx { get; set; } = 24;
    public MiniExcelRustPictureAnchor Anchor { get; set; }
    public int LocationX { get; set; }
    public int LocationY { get; set; }
}