namespace MiniExcelLibs;

public enum MiniExcelRustHorizontalAlignment { Left, Center, Right }
public enum MiniExcelRustVerticalAlignment { Bottom, Center, Top }
public enum MiniExcelRustTableStyle { None, Default }

/// <summary>
/// Configures Rust-backed XLSX writes.
/// </summary>
public sealed class MiniExcelRustWriteOptions
{
    public static MiniExcelRustWriteOptions FromMiniExcel(
        OpenXml.OpenXmlConfiguration configuration) =>
        MiniExcelV1Adapters.ToWriteOptions(configuration);

    public static implicit operator MiniExcelRustWriteOptions(
        OpenXml.OpenXmlConfiguration configuration) =>
        FromMiniExcel(configuration);

    public string SheetName { get; set; } = "Sheet1";
    public bool OverwriteFile { get; set; }
    public bool PrintHeader { get; set; } = true;
    public bool AutoFilter { get; set; } = true;
    public bool RightToLeft { get; set; }
    public bool AutoWidth { get; set; }
    public bool WrapCellContents { get; set; }
    public MiniExcelRustHorizontalAlignment HorizontalAlignment { get; set; }
    public MiniExcelRustVerticalAlignment VerticalAlignment { get; set; }
    public MiniExcelRustTableStyle TableStyle { get; set; } = MiniExcelRustTableStyle.Default;
    public bool HeaderWrapText { get; set; }
    public string HeaderBackgroundColor { get; set; } = "4472C4";
    public MiniExcelRustHorizontalAlignment HeaderHorizontalAlignment { get; set; }
    public MiniExcelRustVerticalAlignment HeaderVerticalAlignment { get; set; }
    public double MinWidth { get; set; } = 8.42857143;
    public double MaxWidth { get; set; } = 200;
    public uint FreezeRowCount { get; set; } = 1;
    public ushort FreezeColumnCount { get; set; }
    public string DateFormat { get; set; } = "yyyy-mm-dd";
    public string TimeFormat { get; set; } = "hh:mm:ss";
    public string DateTimeFormat { get; set; } = "yyyy-mm-dd hh:mm:ss";
    public string DurationFormat { get; set; } = "[h]:mm:ss";
    public IDictionary<string, string> ColumnFormats { get; } = new Dictionary<string, string>();
    public IDictionary<string, double> ColumnWidths { get; } = new Dictionary<string, double>();
    public IDictionary<string, bool> HiddenColumns { get; } = new Dictionary<string, bool>();
    public IDictionary<string, MiniExcelRustDynamicColumn> DynamicColumns { get; } =
        new Dictionary<string, MiniExcelRustDynamicColumn>(StringComparer.Ordinal);
}