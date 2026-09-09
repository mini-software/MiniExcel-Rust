namespace MiniExcelLibs;

/// <summary>
/// Configures Rust-backed XLSX queries.
/// </summary>
public sealed class MiniExcelRustReadOptions
{
    public static MiniExcelRustReadOptions FromMiniExcel(
        OpenXml.OpenXmlConfiguration configuration) =>
        MiniExcelV1Adapters.ToReadOptions(configuration);

    public static implicit operator MiniExcelRustReadOptions(
        OpenXml.OpenXmlConfiguration configuration) =>
        FromMiniExcel(configuration);

    public System.Globalization.CultureInfo Culture { get; set; } = System.Globalization.CultureInfo.InvariantCulture;

    public IDictionary<string, MiniExcelRustDynamicColumn> DynamicColumns { get; } =
        new Dictionary<string, MiniExcelRustDynamicColumn>(StringComparer.Ordinal);

    public bool IgnoreEmptyRows { get; set; }

    public bool FillMergedCells { get; set; }

    public bool TrimColumnNames { get; set; } = true;

    public bool EnableSharedStringCache { get; set; } = true;

    public ulong SharedStringCacheSize { get; set; } = 5 * 1024 * 1024;

    public string? SharedStringCachePath { get; set; } = Path.GetTempPath();
}