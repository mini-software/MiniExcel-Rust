namespace MiniExcelLibs;

public enum MiniExcelRustCsvEncoding : byte
{
    Utf8,
    Utf16Le,
    Utf16Be,
    Gbk,
    Windows1252
}

/// <summary>
/// Configures Rust-backed CSV queries.
/// </summary>
public sealed class MiniExcelRustCsvReadOptions
{
    public static MiniExcelRustCsvReadOptions FromMiniExcel(
        Csv.CsvConfiguration configuration) =>
        MiniExcelV1Adapters.ToReadOptions(configuration);

    public static implicit operator MiniExcelRustCsvReadOptions(
        Csv.CsvConfiguration configuration) =>
        FromMiniExcel(configuration);

    public System.Globalization.CultureInfo Culture { get; set; } = System.Globalization.CultureInfo.InvariantCulture;

    public IDictionary<string, MiniExcelRustDynamicColumn> DynamicColumns { get; } =
        new Dictionary<string, MiniExcelRustDynamicColumn>(StringComparer.Ordinal);

    public char Delimiter { get; set; } = ',';

    public MiniExcelRustCsvEncoding Encoding { get; set; } = MiniExcelRustCsvEncoding.Utf8;

    public bool ReadEmptyStringAsNull { get; set; }

    public bool TrimColumnNames { get; set; }
}