using MiniExcelLibs.Attributes;
using MiniExcelLibs.Csv;
using MiniExcelLibs.OpenXml;

namespace MiniExcelLibs;

internal static class MiniExcelV1Adapters
{
    internal static MiniExcelRustReadOptions ToReadOptions(OpenXmlConfiguration configuration)
    {
        if (configuration is null)
            throw new ArgumentNullException(nameof(configuration));
        if (configuration.SharedStringCacheSize < 0)
            throw new ArgumentOutOfRangeException(nameof(configuration), "SharedStringCacheSize cannot be negative.");

        var options = new MiniExcelRustReadOptions
        {
            Culture = configuration.Culture,
            FillMergedCells = configuration.FillMergedCells,
            TrimColumnNames = configuration.TrimColumnNames,
            IgnoreEmptyRows = configuration.IgnoreEmptyRows,
            EnableSharedStringCache = configuration.EnableSharedStringCache,
            SharedStringCacheSize = checked((ulong)configuration.SharedStringCacheSize),
            SharedStringCachePath = configuration.SharedStringCachePath
        };
        CopyDynamicColumns(configuration.DynamicColumns, options.DynamicColumns);
        return options;
    }

    internal static MiniExcelRustCsvReadOptions ToReadOptions(CsvConfiguration configuration)
    {
        if (configuration is null)
            throw new ArgumentNullException(nameof(configuration));
        var options = new MiniExcelRustCsvReadOptions
        {
            Culture = configuration.Culture,
            Delimiter = configuration.Seperator,
            ReadEmptyStringAsNull = configuration.ReadEmptyStringAsNull
        };
        CopyDynamicColumns(configuration.DynamicColumns, options.DynamicColumns);
        return options;
    }

    internal static MiniExcelRustWriteOptions ToWriteOptions(OpenXmlConfiguration configuration)
    {
        if (configuration is null)
            throw new ArgumentNullException(nameof(configuration));
        var style = configuration.StyleOptions;
        var header = style?.HeaderStyle;
        var options = new MiniExcelRustWriteOptions
        {
            AutoFilter = configuration.AutoFilter,
            RightToLeft = configuration.RightToLeft,
            AutoWidth = configuration.EnableAutoWidth,
            MinWidth = configuration.MinWidth,
            MaxWidth = configuration.MaxWidth,
            FreezeRowCount = checked((uint)configuration.FreezeRowCount),
            FreezeColumnCount = checked((ushort)configuration.FreezeColumnCount),
            TableStyle = configuration.TableStyles == TableStyles.None
                ? MiniExcelRustTableStyle.None
                : MiniExcelRustTableStyle.Default,
            WrapCellContents = style?.WrapCellContents ?? false,
            HorizontalAlignment = ToHorizontalAlignment(style?.HorizontalAlignment),
            VerticalAlignment = ToVerticalAlignment(style?.VerticalAlignment),
            HeaderWrapText = header?.WrapText ?? false,
            HeaderBackgroundColor = header is null
                ? "4472C4"
                : $"{header.BackgroundColor.R:X2}{header.BackgroundColor.G:X2}{header.BackgroundColor.B:X2}",
            HeaderHorizontalAlignment = ToHorizontalAlignment(header?.HorizontalAlignment),
            HeaderVerticalAlignment = ToVerticalAlignment(header?.VerticalAlignment)
        };

        CopyDynamicColumns(configuration.DynamicColumns, options.DynamicColumns);
        foreach (var column in configuration.DynamicColumns ?? Array.Empty<DynamicExcelColumn>())
        {
            if (string.IsNullOrWhiteSpace(column.Key))
                continue;
            if (!string.IsNullOrWhiteSpace(column.Format))
                options.ColumnFormats[column.Key] = column.Format;
            options.ColumnWidths[column.Key] = column.Width;
            options.HiddenColumns[column.Key] = column.Hidden;
        }
        return options;
    }

    private static void CopyDynamicColumns(
        IEnumerable<DynamicExcelColumn>? source,
        IDictionary<string, MiniExcelRustDynamicColumn> destination)
    {
        if (source is null)
            return;

        foreach (var column in source)
        {
            if (string.IsNullOrWhiteSpace(column.Key))
                throw new ArgumentException("MiniExcel DynamicColumns entries must have a key.", nameof(source));
            destination[column.Key] = new MiniExcelRustDynamicColumn
            {
                Name = column.Name,
                Index = column.Index >= 0 ? column.Index : null,
                Format = column.Format,
                Ignore = column.Ignore,
                CustomFormatter = column.CustomFormatter is null
                    ? null
                    : value => column.CustomFormatter(value!),
                IsFormula = column.Type == ColumnType.Formula
            };
        }
    }

    private static MiniExcelRustHorizontalAlignment ToHorizontalAlignment(
        HorizontalCellAlignment? alignment) => alignment switch
    {
        HorizontalCellAlignment.Center => MiniExcelRustHorizontalAlignment.Center,
        HorizontalCellAlignment.Right => MiniExcelRustHorizontalAlignment.Right,
        _ => MiniExcelRustHorizontalAlignment.Left
    };

    private static MiniExcelRustVerticalAlignment ToVerticalAlignment(
        VerticalCellAlignment? alignment) => alignment switch
    {
        VerticalCellAlignment.Center => MiniExcelRustVerticalAlignment.Center,
        VerticalCellAlignment.Top => MiniExcelRustVerticalAlignment.Top,
        _ => MiniExcelRustVerticalAlignment.Bottom
    };
}