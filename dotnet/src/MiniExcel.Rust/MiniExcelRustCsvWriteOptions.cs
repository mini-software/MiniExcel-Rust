namespace MiniExcelLibs;

/// <summary>
/// Configures Rust-backed CSV writes.
/// </summary>
public sealed class MiniExcelRustCsvWriteOptions
{
    public char Delimiter { get; set; } = ',';

    public MiniExcelRustCsvEncoding Encoding { get; set; } = MiniExcelRustCsvEncoding.Utf8;

    public bool WriteBom { get; set; } = true;

    public bool PrintHeader { get; set; } = true;

    public bool OverwriteFile { get; set; }
}