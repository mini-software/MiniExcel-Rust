using System.Collections;
using System.Globalization;
using System.Linq.Expressions;
using System.Reflection;

namespace MiniExcelLibs;

public sealed class MiniExcelRustMapping<T>
{
    private readonly List<IMappedNode> _nodes = new();

    public string WorksheetName { get; private set; } = "Sheet1";

    public MiniExcelRustPropertyMapping<T, TProperty> Property<TProperty>(
        Expression<Func<T, TProperty>> property)
    {
        if (property is null)
            throw new ArgumentNullException(nameof(property));
        var node = new PropertyNode(
            source => property.Compile()((T)source),
            CreateSetter(property),
            typeof(TProperty));
        _nodes.Add(node);
        return new MiniExcelRustPropertyMapping<T, TProperty>(node);
    }

    public MiniExcelRustCollectionMapping<T, TCollection> Collection<TCollection>(
        Expression<Func<T, TCollection>> collection)
        where TCollection : IEnumerable
    {
        if (collection is null)
            throw new ArgumentNullException(nameof(collection));
        var node = new CollectionNode(
            source => propertyValue(collection.Compile()((T)source)),
            CreateSetter(collection),
            typeof(TCollection),
            CollectionItemType(typeof(TCollection)));
        _nodes.Add(node);
        return new MiniExcelRustCollectionMapping<T, TCollection>(node);

        static IEnumerable propertyValue(TCollection value) => value;
    }

    public MiniExcelRustMapping<T> ToWorksheet(string worksheetName)
    {
        if (string.IsNullOrWhiteSpace(worksheetName) || worksheetName.Length > 31)
            throw new ArgumentException("The worksheet name must contain 1 to 31 characters.", nameof(worksheetName));
        WorksheetName = worksheetName;
        return this;
    }

    internal int MaximumRow => _nodes.Count == 0 ? 1 : _nodes.Max(node => node.MaximumRow);
    internal int MinimumRow => _nodes.Count == 0 ? 1 : _nodes.Min(node => node.MinimumRow);

    internal int Write(object source, MappedGrid grid, int rowOffset)
    {
        var maximumRow = rowOffset;
        foreach (var node in _nodes)
            maximumRow = Math.Max(maximumRow, node.Write(source, grid, rowOffset));
        return maximumRow;
    }

    internal void Read(object destination, IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset, int endRow)
    {
        foreach (var node in _nodes)
            node.Read(destination, rows, rowOffset, endRow);
    }

    internal bool HasAnchorData(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset)
    {
        var directAnchors = _nodes
            .OfType<PropertyNode>()
            .Where(node => node.MinimumRow == MinimumRow)
            .Cast<IMappedNode>()
            .ToArray();
        var anchors = directAnchors.Length > 0
            ? directAnchors
            : _nodes.Where(node => node.MinimumRow == MinimumRow);
        return anchors.Any(node => node.HasData(rows, rowOffset));
    }

    private static Action<object, object?>? CreateSetter<TProperty>(Expression<Func<T, TProperty>> expression)
    {
        var member = expression.Body as MemberExpression;
        if (member is null && expression.Body is UnaryExpression unary)
            member = unary.Operand as MemberExpression;
        return member?.Member switch
        {
            PropertyInfo property when property.CanWrite => (target, value) => property.SetValue(target, ConvertValue(value, property.PropertyType)),
            FieldInfo field when !field.IsInitOnly => (target, value) => field.SetValue(target, ConvertValue(value, field.FieldType)),
            _ => null
        };
    }

    private static Type CollectionItemType(Type collectionType)
    {
        if (collectionType.IsArray)
            return collectionType.GetElementType()!;
        return collectionType
            .GetInterfaces()
            .Concat(new[] { collectionType })
            .FirstOrDefault(type => type.IsGenericType && type.GetGenericTypeDefinition() == typeof(IEnumerable<>))?
            .GetGenericArguments()[0] ?? typeof(object);
    }

    internal static object? ConvertValue(object? value, Type targetType)
    {
        if (value is null || value is DBNull)
            return targetType.IsValueType && Nullable.GetUnderlyingType(targetType) is null
                ? Activator.CreateInstance(targetType)
                : null;
        var effectiveType = Nullable.GetUnderlyingType(targetType) ?? targetType;
        if (effectiveType.IsInstanceOfType(value))
            return value;
        if (effectiveType.IsEnum)
            return value is string text
                ? Enum.Parse(effectiveType, text, true)
                : Enum.ToObject(effectiveType, Convert.ToInt64(value, CultureInfo.InvariantCulture));
        if (effectiveType == typeof(Guid))
            return Guid.Parse(Convert.ToString(value, CultureInfo.InvariantCulture)!);
        if (effectiveType == typeof(Uri))
            return new Uri(Convert.ToString(value, CultureInfo.InvariantCulture)!, UriKind.RelativeOrAbsolute);
        if (effectiveType == typeof(TimeSpan))
            return value is TimeSpan span ? span : TimeSpan.Parse(Convert.ToString(value, CultureInfo.InvariantCulture)!, CultureInfo.InvariantCulture);
        if (effectiveType == typeof(DateTimeOffset))
            return value is DateTimeOffset offset ? offset : new DateTimeOffset(Convert.ToDateTime(value, CultureInfo.InvariantCulture));
        return Convert.ChangeType(value, effectiveType, CultureInfo.InvariantCulture);
    }

    internal interface IMappedNode
    {
        int MinimumRow { get; }
        int MaximumRow { get; }
        int Write(object source, MappedGrid grid, int rowOffset);
        void Read(object destination, IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset, int endRow);
        bool HasData(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset);
    }

    internal sealed class PropertyNode(
        Func<object, object?> getter,
        Action<object, object?>? setter,
        Type propertyType) : IMappedNode
    {
        public int Row { get; set; }
        public int Column { get; set; }
        public string? Format { get; set; }
        public string? Formula { get; set; }
        public int MinimumRow => Row;
        public int MaximumRow => Row;

        public int Write(object source, MappedGrid grid, int rowOffset)
        {
            if (Row == 0)
                throw new InvalidOperationException("A property mapping requires ToCell().");
            grid.Set(Row + rowOffset, Column, Formula ?? getter(source), Format, Formula is not null);
            return Row + rowOffset;
        }

        public void Read(object destination, IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset, int endRow)
        {
            if (Row == 0)
                throw new InvalidOperationException("A property mapping requires ToCell().");
            if (setter is null)
                throw new InvalidOperationException("A mapped property must be writable when reading.");
            if (MappedGrid.TryGet(rows, Row + rowOffset, Column, out var value))
                setter(destination, ConvertValue(value, propertyType));
        }

        public bool HasData(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset) =>
            Row != 0 && MappedGrid.TryGet(rows, Row + rowOffset, Column, out _);
    }

    internal sealed class CollectionNode(
        Func<object, IEnumerable> getter,
        Action<object, object?>? setter,
        Type collectionType,
        Type itemType) : IMappedNode
    {
        public int Row { get; set; }
        public int Column { get; set; }
        public int Spacing { get; set; }
        public IItemPlan? ItemPlan { get; set; }
        public int MinimumRow => Row;
        public int MaximumRow => ItemPlan?.MaximumRow ?? Math.Max(1, Row);

        public int Write(object source, MappedGrid grid, int rowOffset)
        {
            if (Row == 0)
                throw new InvalidOperationException("A collection mapping requires StartAt().");
            var maximumRow = Row + rowOffset;
            var itemOffset = rowOffset;
            foreach (var item in getter(source))
            {
                if (ItemPlan is null)
                {
                    grid.Set(Row + itemOffset, Column, item, null, false);
                    maximumRow = Row + itemOffset;
                    itemOffset += Spacing + 1;
                }
                else if (item is not null)
                {
                    maximumRow = Math.Max(maximumRow, ItemPlan.Write(item, grid, itemOffset));
                    itemOffset += Math.Max(1, maximumRow - (Row + itemOffset) + 1) + Spacing;
                }
            }
            return maximumRow;
        }

        public void Read(object destination, IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset, int endRow)
        {
            if (Row == 0)
                throw new InvalidOperationException("A collection mapping requires StartAt().");
            if (setter is null)
                throw new InvalidOperationException("A mapped collection must be writable when reading.");
            var values = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(itemType))!;
            if (ItemPlan is null)
            {
                for (var row = Row + rowOffset; row <= endRow; row += Spacing + 1)
                {
                    if (!MappedGrid.TryGet(rows, row, Column, out var value))
                        break;
                    values.Add(ConvertValue(value, itemType));
                }
            }
            else
            {
                var starts = new List<int>();
                for (var row = Row + rowOffset; row <= endRow; row++)
                {
                    var itemOffset = row - ItemPlan.MinimumRow;
                    if (ItemPlan.HasAnchorData(rows, itemOffset))
                        starts.Add(row);
                }
                for (var index = 0; index < starts.Count; index++)
                {
                    var itemOffset = starts[index] - ItemPlan.MinimumRow;
                    var itemEnd = index + 1 < starts.Count ? starts[index + 1] - 1 : endRow;
                    values.Add(ItemPlan.Read(rows, itemOffset, itemEnd));
                }
            }
            setter(destination, AdaptCollection(values, collectionType, itemType));
        }

        public bool HasData(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset) =>
            Row != 0 && (ItemPlan?.HasAnchorData(rows, rowOffset + Row - ItemPlan.MinimumRow)
                ?? MappedGrid.TryGet(rows, Row + rowOffset, Column, out _));

        private static object AdaptCollection(IList values, Type targetType, Type elementType)
        {
            if (targetType.IsArray)
            {
                var array = Array.CreateInstance(elementType, values.Count);
                values.CopyTo(array, 0);
                return array;
            }
            if (targetType.IsInstanceOfType(values))
                return values;
            if (Activator.CreateInstance(targetType) is IList target)
            {
                foreach (var value in values)
                    target.Add(value);
                return target;
            }
            throw new InvalidOperationException($"Mapped collection type '{targetType}' cannot be populated.");
        }
    }

    internal interface IItemPlan
    {
        int MinimumRow { get; }
        int MaximumRow { get; }
        int Write(object source, MappedGrid grid, int rowOffset);
        object Read(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset, int endRow);
        bool HasAnchorData(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset);
    }

    internal sealed class ItemPlan<TItem>(MiniExcelRustMapping<TItem> mapping) : IItemPlan
    {
        public int MinimumRow => mapping.MinimumRow;
        public int MaximumRow => mapping.MaximumRow;
        public int Write(object source, MappedGrid grid, int rowOffset) => mapping.Write(source, grid, rowOffset);
        public object Read(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset, int endRow)
        {
            var item = Activator.CreateInstance<TItem>()!;
            mapping.Read(item!, rows, rowOffset, endRow);
            return item!;
        }
        public bool HasAnchorData(IReadOnlyList<IDictionary<string, object?>> rows, int rowOffset) =>
            mapping.HasAnchorData(rows, rowOffset);
    }
}

public sealed class MiniExcelRustPropertyMapping<T, TProperty>
{
    private readonly MiniExcelRustMapping<T>.PropertyNode _node;

    internal MiniExcelRustPropertyMapping(MiniExcelRustMapping<T>.PropertyNode node)
    {
        _node = node;
    }

    public MiniExcelRustPropertyMapping<T, TProperty> ToCell(string cellAddress)
    {
        (_node.Row, _node.Column) = MappedGrid.ParseCell(cellAddress);
        return this;
    }

    public MiniExcelRustPropertyMapping<T, TProperty> WithFormat(string format)
    {
        _node.Format = format;
        return this;
    }

    public MiniExcelRustPropertyMapping<T, TProperty> WithFormula(string formula)
    {
        _node.Formula = formula;
        return this;
    }
}

public sealed class MiniExcelRustCollectionMapping<T, TCollection>
    where TCollection : IEnumerable
{
    private readonly MiniExcelRustMapping<T>.CollectionNode _node;

    internal MiniExcelRustCollectionMapping(MiniExcelRustMapping<T>.CollectionNode node)
    {
        _node = node;
    }

    public MiniExcelRustCollectionMapping<T, TCollection> StartAt(string cellAddress)
    {
        (_node.Row, _node.Column) = MappedGrid.ParseCell(cellAddress);
        return this;
    }

    public MiniExcelRustCollectionMapping<T, TCollection> WithSpacing(int spacing)
    {
        if (spacing < 0)
            throw new ArgumentOutOfRangeException(nameof(spacing));
        _node.Spacing = spacing;
        return this;
    }

    public MiniExcelRustCollectionMapping<T, TCollection> WithItemMapping<TItem>(
        Action<MiniExcelRustMapping<TItem>> configure)
    {
        if (configure is null)
            throw new ArgumentNullException(nameof(configure));
        var mapping = new MiniExcelRustMapping<TItem>();
        configure(mapping);
        _node.ItemPlan = new MiniExcelRustMapping<T>.ItemPlan<TItem>(mapping);
        return this;
    }
}

internal sealed class MappedGrid
{
    private readonly SortedDictionary<(int Row, int Column), object?> _values = new();
    private readonly Dictionary<int, string> _formats = new();
    private readonly HashSet<(int Row, int Column)> _formulaCells = new();

    public void Set(int row, int column, object? value, string? format, bool formula)
    {
        _values[(row, column)] = value;
        if (format is not null)
            _formats[column] = format;
        if (formula)
            _formulaCells.Add((row, column));
    }

    public int Save(string path, string sheetName, bool overwriteFile)
    {
        if (_values.Count == 0)
            throw new InvalidOperationException("The mapping did not produce any cells.");
        var maxRow = _values.Keys.Max(cell => cell.Row);
        var maxColumn = _values.Keys.Max(cell => cell.Column);
        var schema = Enumerable.Range(1, maxColumn).Select(ColumnName).ToArray();
        var rows = new List<IDictionary<string, object?>>(maxRow);
        for (var rowIndex = 1; rowIndex <= maxRow; rowIndex++)
        {
            IDictionary<string, object?> row = new Dictionary<string, object?>(StringComparer.Ordinal);
            foreach (var cell in _values.Where(value => value.Key.Row == rowIndex))
                row[ColumnName(cell.Key.Column)] = cell.Value;
            rows.Add(row);
        }
        var options = new MiniExcelRustWriteOptions
        {
            SheetName = sheetName,
            PrintHeader = false,
            OverwriteFile = overwriteFile
        };
        foreach (var format in _formats)
            options.ColumnFormats[ColumnName(format.Key)] = format.Value;
        var count = MiniExcelRust.SaveAsWithSchema(path, schema, rows, options);
        if (_formulaCells.Count > 0)
            MiniExcelRust.FillMappedTemplateCore(path, path, CreateTemplatePayload(sheetName), true);
        return count;
    }

    public byte[] CreateTemplatePayload(string sheetName)
    {
        return MiniExcelBinaryPayload.EncodeMappedTemplate(
            sheetName,
            _values.Select(cell => new MiniExcelBinaryPayload.MappedCell(
                $"{ColumnName(cell.Key.Column)}{cell.Key.Row}",
                cell.Value,
                _formulaCells.Contains(cell.Key))));
    }

    internal static (int Row, int Column) ParseCell(string cellAddress)
    {
        if (string.IsNullOrWhiteSpace(cellAddress))
            throw new ArgumentException("A cell address is required.", nameof(cellAddress));
        var letters = cellAddress.TakeWhile(char.IsLetter).ToArray();
        var digits = cellAddress.SkipWhile(char.IsLetter).ToArray();
        if (letters.Length == 0 || digits.Length == 0 || !int.TryParse(new string(digits), out var row) || row < 1)
            throw new ArgumentException($"Invalid cell address '{cellAddress}'.", nameof(cellAddress));
        var column = 0;
        foreach (var letter in letters)
        {
            if (letter is not (>= 'A' and <= 'Z') and not (>= 'a' and <= 'z'))
                throw new ArgumentException($"Invalid cell address '{cellAddress}'.", nameof(cellAddress));
            column = checked(column * 26 + char.ToUpperInvariant(letter) - 'A' + 1);
        }
        if (column > 16_384 || row > 1_048_576)
            throw new ArgumentOutOfRangeException(nameof(cellAddress));
        return (row, column);
    }

    internal static bool TryGet(
        IReadOnlyList<IDictionary<string, object?>> rows,
        int row,
        int column,
        out object? value)
    {
        value = null;
        return row > 0 && row <= rows.Count &&
            rows[row - 1].TryGetValue(ColumnName(column), out value) &&
            value is not null && value is not DBNull &&
            (value is not string text || text.Length > 0);
    }

    internal static string ColumnName(int column)
    {
        var name = string.Empty;
        while (column > 0)
        {
            column--;
            name = (char)('A' + column % 26) + name;
            column /= 26;
        }
        return name;
    }
}

public static partial class MiniExcelRustMappingExtensions
{
    public static T ReadMapped<T>(
        string path,
        MiniExcelRustMapping<T> mapping)
        where T : new()
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (mapping is null)
            throw new ArgumentNullException(nameof(mapping));
        var rows = MiniExcelRust.Query(path, false, mapping.WorksheetName).ToList();
        var value = new T();
        mapping.Read(value!, rows, 0, rows.Count);
        return value;
    }

    public static T ReadMapped<T>(
        Stream stream,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false)
        where T : new()
    {
        if (stream is null)
            throw new ArgumentNullException(nameof(stream));
        if (mapping is null)
            throw new ArgumentNullException(nameof(mapping));
        var rows = MiniExcelRust.Query(stream, false, mapping.WorksheetName, leaveOpen: leaveOpen).ToList();
        var value = new T();
        mapping.Read(value!, rows, 0, rows.Count);
        return value;
    }

    public static Task<T> ReadMappedAsync<T>(
        string path,
        MiniExcelRustMapping<T> mapping,
        CancellationToken cancellationToken = default)
        where T : new()
    {
        return Task.Run(() => ReadMapped(path, mapping), cancellationToken);
    }

    public static Task<T> ReadMappedAsync<T>(
        Stream stream,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false,
        CancellationToken cancellationToken = default)
        where T : new()
    {
        return Task.Run(() => ReadMapped(stream, mapping, leaveOpen), cancellationToken);
    }

    public static int ExportMapped<T>(
        string path,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool overwriteFile = false)
    {
        if (values is null)
            throw new ArgumentNullException(nameof(values));
        if (mapping is null)
            throw new ArgumentNullException(nameof(mapping));
        var grid = BuildGrid(values, mapping);
        return grid.Save(path, mapping.WorksheetName, overwriteFile);
    }

    public static int ExportMapped<T>(
        Stream stream,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false)
    {
        if (stream is null)
            throw new ArgumentNullException(nameof(stream));
        if (!stream.CanWrite)
            throw new ArgumentException("The stream must be writable.", nameof(stream));
        var outputPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-mapped-export-{Guid.NewGuid():N}.xlsx");
        try
        {
            var count = ExportMapped(outputPath, values, mapping);
            CopyToStream(outputPath, stream);
            return count;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            if (File.Exists(outputPath))
                File.Delete(outputPath);
        }
    }

    public static Task<int> ExportMappedAsync<T>(
        string path,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool overwriteFile = false,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() =>
        {
            var grid = BuildGrid(values, mapping, cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            return grid.Save(path, mapping.WorksheetName, overwriteFile);
        }, cancellationToken);
    }

    public static Task<int> ExportMappedAsync<T>(
        Stream stream,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() =>
        {
            cancellationToken.ThrowIfCancellationRequested();
            return ExportMapped(stream, values, mapping, leaveOpen);
        }, cancellationToken);
    }

    public static void FillMappedTemplate<T>(
        string destinationPath,
        string templatePath,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool overwriteFile = false)
    {
        if (values is null)
            throw new ArgumentNullException(nameof(values));
        if (mapping is null)
            throw new ArgumentNullException(nameof(mapping));
        var grid = BuildGrid(values, mapping);
        MiniExcelRust.FillMappedTemplateCore(
            destinationPath,
            templatePath,
            grid.CreateTemplatePayload(mapping.WorksheetName),
            overwriteFile);
    }

    public static void FillMappedTemplate<T>(
        Stream outputStream,
        Stream templateStream,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false,
        bool leaveTemplateOpen = false)
    {
        if (outputStream is null)
            throw new ArgumentNullException(nameof(outputStream));
        if (!outputStream.CanWrite)
            throw new ArgumentException("The stream must be writable.", nameof(outputStream));
        if (templateStream is null)
            throw new ArgumentNullException(nameof(templateStream));
        if (!templateStream.CanRead)
            throw new ArgumentException("The stream must be readable.", nameof(templateStream));
        var templatePath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-mapped-template-{Guid.NewGuid():N}.xlsx");
        var outputPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-mapped-output-{Guid.NewGuid():N}.xlsx");
        try
        {
            using (var file = File.Create(templatePath))
                templateStream.CopyTo(file);
            FillMappedTemplate(outputPath, templatePath, values, mapping);
            if (outputStream.CanSeek)
            {
                outputStream.Position = 0;
                outputStream.SetLength(0);
            }
            using var result = File.OpenRead(outputPath);
            result.CopyTo(outputStream);
        }
        finally
        {
            if (!leaveOpen)
                outputStream.Dispose();
            if (!leaveTemplateOpen)
                templateStream.Dispose();
            if (File.Exists(templatePath))
                File.Delete(templatePath);
            if (File.Exists(outputPath))
                File.Delete(outputPath);
        }
    }

    public static void FillMappedTemplate<T>(
        Stream outputStream,
        byte[] templateBytes,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false)
    {
        if (templateBytes is null)
            throw new ArgumentNullException(nameof(templateBytes));
        using var templateStream = new MemoryStream(templateBytes, writable: false);
        FillMappedTemplate(outputStream, templateStream, values, mapping, leaveOpen, false);
    }

    public static Task FillMappedTemplateAsync<T>(
        string destinationPath,
        string templatePath,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool overwriteFile = false,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(
            () => FillMappedTemplate(destinationPath, templatePath, values, mapping, overwriteFile),
            cancellationToken);
    }

    public static Task FillMappedTemplateAsync<T>(
        Stream outputStream,
        Stream templateStream,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false,
        bool leaveTemplateOpen = false,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() =>
        {
            cancellationToken.ThrowIfCancellationRequested();
            FillMappedTemplate(outputStream, templateStream, values, mapping, leaveOpen, leaveTemplateOpen);
        }, cancellationToken);
    }

    public static Task FillMappedTemplateAsync<T>(
        Stream outputStream,
        byte[] templateBytes,
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        bool leaveOpen = false,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() =>
        {
            cancellationToken.ThrowIfCancellationRequested();
            FillMappedTemplate(outputStream, templateBytes, values, mapping, leaveOpen);
        }, cancellationToken);
    }

    private static MappedGrid BuildGrid<T>(
        IEnumerable<T> values,
        MiniExcelRustMapping<T> mapping,
        CancellationToken cancellationToken = default)
    {
        var grid = new MappedGrid();
        var offset = 0;
        foreach (var value in values)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (value is null)
                throw new ArgumentException("Mapped values cannot contain null.", nameof(values));
            var maximumRow = mapping.Write(value, grid, offset);
            offset = Math.Max(offset + mapping.MaximumRow, maximumRow);
        }
        return grid;
    }

    private static void CopyToStream(string path, Stream stream)
    {
        if (stream.CanSeek)
        {
            stream.Position = 0;
            stream.SetLength(0);
        }
        using var input = File.OpenRead(path);
        input.CopyTo(stream);
    }
}