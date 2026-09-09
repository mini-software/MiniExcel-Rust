namespace MiniExcelLibs;

public sealed class MiniExcelRustDynamicColumn
{
    public string? Name { get; set; }
    public int? Index { get; set; }
    public string? Format { get; set; }
    public bool Ignore { get; set; }
    public Func<object?, object?>? CustomFormatter { get; set; }
    public bool IsFormula { get; set; }
}