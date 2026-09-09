namespace MiniExcelLibs;

/// <summary>
/// Configures insertion of a Rust-generated worksheet into an XLSX workbook.
/// </summary>
public sealed class MiniExcelRustInsertOptions
{
    public bool PrintHeader { get; set; } = true;

    public bool ReplaceExistingSheet { get; set; }

    public bool RemoveSupportedRelationships { get; set; }

    public bool OverwriteDestination { get; set; }
}