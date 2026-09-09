using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text;

namespace MiniExcelLibs;

internal static class MiniExcelBinaryPayload
{
    private const byte Version = 1;
    private const int MaxDepth = 64;

    private enum PayloadKind : byte
    {
        XlsxWrite = 1,
        CsvWrite = 2,
        TemplateValue = 3,
        MappedTemplate = 4
    }

    private enum ValueKind : byte
    {
        Null,
        Boolean,
        Int64,
        UInt64,
        Double,
        String,
        Array,
        Object,
        Decimal
    }

    internal static byte[] EncodeXlsxWrite(
        IReadOnlyList<string> schema,
        MiniExcelRustWriteOptions options)
    {
        using var stream = CreatePayload(PayloadKind.XlsxWrite, out var writer);
        using (writer)
        {
            WriteStrings(writer, schema);
            WriteString(writer, options.SheetName);
            WriteBoolean(writer, options.OverwriteFile);
            WriteBoolean(writer, options.PrintHeader);
            WriteBoolean(writer, options.AutoFilter);
            WriteBoolean(writer, options.RightToLeft);
            WriteBoolean(writer, options.AutoWidth);
            WriteBoolean(writer, options.WrapCellContents);
            writer.Write((byte)options.HorizontalAlignment);
            writer.Write((byte)options.VerticalAlignment);
            writer.Write((byte)options.TableStyle);
            WriteBoolean(writer, options.HeaderWrapText);
            WriteString(writer, options.HeaderBackgroundColor);
            writer.Write((byte)options.HeaderHorizontalAlignment);
            writer.Write((byte)options.HeaderVerticalAlignment);
            writer.Write(options.MinWidth);
            writer.Write(options.MaxWidth);
            writer.Write(options.FreezeRowCount);
            writer.Write(options.FreezeColumnCount);
            WriteString(writer, options.DateFormat);
            WriteString(writer, options.TimeFormat);
            WriteString(writer, options.DateTimeFormat);
            WriteString(writer, options.DurationFormat);
            WriteStringMap(writer, options.ColumnFormats);
            WriteDoubleMap(writer, options.ColumnWidths);
            WriteBooleanMap(writer, options.HiddenColumns);
            WriteStrings(
                writer,
                options.DynamicColumns
                    .Where(column => column.Value.IsFormula)
                    .Select(column => string.IsNullOrWhiteSpace(column.Value.Name)
                        ? column.Key
                        : column.Value.Name!)
                    .ToArray());
        }
        return stream.ToArray();
    }

    internal static byte[] EncodeCsvWrite(
        IReadOnlyList<string> schema,
        MiniExcelRustCsvWriteOptions options)
    {
        using var stream = CreatePayload(PayloadKind.CsvWrite, out var writer);
        using (writer)
        {
            WriteStrings(writer, schema);
            writer.Write(checked((byte)options.Delimiter));
            writer.Write((byte)options.Encoding);
            WriteBoolean(writer, options.WriteBom);
            WriteBoolean(writer, options.PrintHeader);
            WriteBoolean(writer, options.OverwriteFile);
        }
        return stream.ToArray();
    }

    internal static byte[] EncodeTemplateValue(object? value)
    {
        using var stream = CreatePayload(PayloadKind.TemplateValue, out var writer);
        using (writer)
            WriteValue(writer, value, 0, new HashSet<object>(ReferenceComparer.Instance));
        return stream.ToArray();
    }

    internal static byte[] EncodeMappedTemplate(
        string sheetName,
        IEnumerable<MappedCell> cells)
    {
        var materialized = cells.ToList();
        using var stream = CreatePayload(PayloadKind.MappedTemplate, out var writer);
        using (writer)
        {
            WriteString(writer, sheetName);
            writer.Write(checked((uint)materialized.Count));
            var ancestors = new HashSet<object>(ReferenceComparer.Instance);
            foreach (var cell in materialized)
            {
                WriteString(writer, cell.Address);
                WriteBoolean(writer, cell.Formula);
                WriteValue(writer, cell.Value, 0, ancestors);
            }
        }
        return stream.ToArray();
    }

    private static MemoryStream CreatePayload(PayloadKind kind, out BinaryWriter writer)
    {
        var stream = new MemoryStream();
        writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
        writer.Write(new[] { (byte)'M', (byte)'X', (byte)'B', (byte)'P' });
        writer.Write(Version);
        writer.Write((byte)kind);
        return stream;
    }

    private static void WriteValue(
        BinaryWriter writer,
        object? value,
        int depth,
        ISet<object> ancestors)
    {
        if (depth > MaxDepth)
            throw new InvalidOperationException($"Template values cannot exceed {MaxDepth} nested levels.");

        switch (value)
        {
            case null:
            case DBNull:
                writer.Write((byte)ValueKind.Null);
                return;
            case bool boolean:
                writer.Write((byte)ValueKind.Boolean);
                WriteBoolean(writer, boolean);
                return;
            case byte or sbyte or short or ushort or int or uint or long:
                writer.Write((byte)ValueKind.Int64);
                writer.Write(Convert.ToInt64(value, CultureInfo.InvariantCulture));
                return;
            case ulong unsigned:
                writer.Write((byte)ValueKind.UInt64);
                writer.Write(unsigned);
                return;
            case float or double:
                var number = Convert.ToDouble(value, CultureInfo.InvariantCulture);
                if (double.IsNaN(number) || double.IsInfinity(number))
                    throw new ArgumentException("Template numbers must be finite.", nameof(value));
                writer.Write((byte)ValueKind.Double);
                writer.Write(number);
                return;
            case decimal decimalValue:
                writer.Write((byte)ValueKind.Decimal);
                WriteString(writer, decimalValue.ToString(CultureInfo.InvariantCulture));
                return;
            case string text:
                writer.Write((byte)ValueKind.String);
                WriteString(writer, text);
                return;
            case char character:
                writer.Write((byte)ValueKind.String);
                WriteString(writer, character.ToString());
                return;
            case DateTime dateTime:
                writer.Write((byte)ValueKind.String);
                WriteString(writer, dateTime.ToString("O", CultureInfo.InvariantCulture));
                return;
            case DateTimeOffset dateTimeOffset:
                writer.Write((byte)ValueKind.String);
                WriteString(writer, dateTimeOffset.ToString("O", CultureInfo.InvariantCulture));
                return;
            case TimeSpan timeSpan:
                writer.Write((byte)ValueKind.String);
                WriteString(writer, timeSpan.ToString("c", CultureInfo.InvariantCulture));
                return;
            case Guid or Uri:
                writer.Write((byte)ValueKind.String);
                WriteString(writer, Convert.ToString(value, CultureInfo.InvariantCulture)!);
                return;
            case byte[] bytes:
                writer.Write((byte)ValueKind.String);
                WriteString(writer, Convert.ToBase64String(bytes));
                return;
        }

        var type = value.GetType();
        if (type.IsEnum)
        {
            writer.Write((byte)ValueKind.Int64);
            writer.Write(Convert.ToInt64(value, CultureInfo.InvariantCulture));
            return;
        }

        if (!type.IsValueType && !ancestors.Add(value))
            throw new InvalidOperationException("Template values cannot contain reference cycles.");
        try
        {
            if (value is IDictionary dictionary)
            {
                writer.Write((byte)ValueKind.Object);
                writer.Write(checked((uint)dictionary.Count));
                foreach (DictionaryEntry entry in dictionary)
                {
                    if (entry.Key is not string key)
                        throw new ArgumentException("Template dictionary keys must be strings.", nameof(value));
                    WriteString(writer, key);
                    WriteValue(writer, entry.Value, depth + 1, ancestors);
                }
                return;
            }

            if (value is IEnumerable sequence)
            {
                var items = sequence.Cast<object?>().ToList();
                writer.Write((byte)ValueKind.Array);
                writer.Write(checked((uint)items.Count));
                foreach (var item in items)
                    WriteValue(writer, item, depth + 1, ancestors);
                return;
            }

            var properties = type.GetProperties(BindingFlags.Instance | BindingFlags.Public)
                .Where(property => property.GetMethod is not null && property.GetIndexParameters().Length == 0)
                .Where(property => !HasJsonIgnore(property))
                .ToList();
            writer.Write((byte)ValueKind.Object);
            writer.Write(checked((uint)properties.Count));
            foreach (var property in properties)
            {
                WriteString(writer, JsonPropertyName(property));
                WriteValue(writer, property.GetValue(value), depth + 1, ancestors);
            }
        }
        finally
        {
            if (!type.IsValueType)
                ancestors.Remove(value);
        }
    }

    private static bool HasJsonIgnore(MemberInfo member) =>
        member.CustomAttributes.Any(attribute =>
            attribute.AttributeType.FullName == "System.Text.Json.Serialization.JsonIgnoreAttribute");

    private static string JsonPropertyName(MemberInfo member)
    {
        var attribute = member.CustomAttributes.FirstOrDefault(candidate =>
            candidate.AttributeType.FullName == "System.Text.Json.Serialization.JsonPropertyNameAttribute");
        return attribute?.ConstructorArguments.FirstOrDefault().Value as string ?? member.Name;
    }

    private static void WriteStrings(BinaryWriter writer, IReadOnlyCollection<string> values)
    {
        writer.Write(checked((uint)values.Count));
        foreach (var value in values)
            WriteString(writer, value);
    }

    private static void WriteStringMap(BinaryWriter writer, IDictionary<string, string> values)
    {
        writer.Write(checked((uint)values.Count));
        foreach (var value in values)
        {
            WriteString(writer, value.Key);
            WriteString(writer, value.Value);
        }
    }

    private static void WriteDoubleMap(BinaryWriter writer, IDictionary<string, double> values)
    {
        writer.Write(checked((uint)values.Count));
        foreach (var value in values)
        {
            WriteString(writer, value.Key);
            writer.Write(value.Value);
        }
    }

    private static void WriteBooleanMap(BinaryWriter writer, IDictionary<string, bool> values)
    {
        writer.Write(checked((uint)values.Count));
        foreach (var value in values)
        {
            WriteString(writer, value.Key);
            WriteBoolean(writer, value.Value);
        }
    }

    private static void WriteString(BinaryWriter writer, string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        writer.Write(checked((uint)bytes.Length));
        writer.Write(bytes);
    }

    private static void WriteBoolean(BinaryWriter writer, bool value) =>
        writer.Write((byte)(value ? 1 : 0));

    internal readonly struct MappedCell
    {
        internal MappedCell(string address, object? value, bool formula)
        {
            Address = address;
            Value = value;
            Formula = formula;
        }

        internal string Address { get; }
        internal object? Value { get; }
        internal bool Formula { get; }
    }

    private sealed class ReferenceComparer : IEqualityComparer<object>
    {
        internal static readonly ReferenceComparer Instance = new();

        public new bool Equals(object? left, object? right) => ReferenceEquals(left, right);

        public int GetHashCode(object value) => RuntimeHelpers.GetHashCode(value);
    }
}