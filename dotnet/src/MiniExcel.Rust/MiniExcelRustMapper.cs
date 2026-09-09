using System.ComponentModel;
using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Resources;

namespace MiniExcelLibs;

internal static class MiniExcelRustMapper
{
    public static IEnumerable<IDictionary<string, object?>> ToRows(IEnumerable values)
    {
        IReadOnlyList<MemberMapping>? mappings = null;
        Type? mappedType = null;
        foreach (var value in values)
        {
            if (value is null)
                throw new ArgumentException("Typed export rows cannot contain null values.", nameof(values));
            if (value is IDictionary<string, object?> row)
            {
                yield return row;
                continue;
            }
            var valueType = value.GetType();
            if (mappedType != valueType)
            {
                mappedType = valueType;
                mappings = CreateMappings(valueType, true, CultureInfo.InvariantCulture, null)
                    .OrderBy(mapping => mapping.Index ?? int.MaxValue)
                    .ToList();
            }
            IDictionary<string, object?> projected = new Dictionary<string, object?>(StringComparer.Ordinal);
            foreach (var mapping in mappings!)
                projected.Add(mapping.Names[0], NormalizeWriteValue(mapping.FormatValue(mapping.GetValue(value))));
            yield return projected;
        }
    }

    public static IEnumerable<IDictionary<string, object?>> ToRows<T>(
        IEnumerable<T> values,
        IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>? dynamicColumns = null)
    {
        var mappings = CreateMappings(
                typeof(T),
                forWrite: true,
                CultureInfo.InvariantCulture,
                dynamicColumns)
            .OrderBy(mapping => mapping.Index ?? int.MaxValue)
            .ToList();
        foreach (var value in values)
        {
            if (value is null)
                throw new ArgumentException("Typed export rows cannot contain null values.", nameof(values));
            IDictionary<string, object?> row = new Dictionary<string, object?>(StringComparer.Ordinal);
            foreach (var mapping in mappings)
                row.Add(
                    mapping.Names[0],
                    NormalizeWriteValue(mapping.FormatValue(mapping.GetValue(value))));
            yield return row;
        }
    }

    public static IEnumerable<T> Map<T>(
        IEnumerable<IDictionary<string, object?>> rows,
        CultureInfo? culture = null,
        IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>? dynamicColumns = null)
        where T : class, new()
    {
        culture ??= CultureInfo.InvariantCulture;
        var mappings = CreateMappings(typeof(T), forWrite: false, culture, dynamicColumns);
        var rowIndex = 1;
        foreach (var row in rows)
        {
            var instance = new T();
            var values = row.Values.ToList();
            foreach (var mapping in mappings)
            {
                object? value = null;
                var found = mapping.Index is int index
                    ? index >= 0 && index < values.Count && Assign(values[index], out value)
                    : TryGetValue(row, mapping.Names, out value);
                if (!found)
                    throw new MiniExcelRustColumnNotFoundException(mapping.Names[0], rowIndex);

                try
                {
                    mapping.SetValue(instance, ConvertValue(value, mapping.ValueType, mapping.Format, culture));
                }
                catch (Exception error) when (error is InvalidCastException or FormatException or OverflowException or ArgumentException)
                {
                    throw new MiniExcelRustMappingException(
                        mapping.Names[0],
                        rowIndex,
                        value,
                        mapping.ValueType,
                        error);
                }
            }
            yield return instance;
            rowIndex++;
        }
    }

    private static IReadOnlyList<MemberMapping> CreateMappings(
        Type type,
        bool forWrite,
        CultureInfo culture,
        IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>? dynamicColumns)
    {
        const BindingFlags flags = BindingFlags.Instance | BindingFlags.Public;
        var members = type.GetProperties(flags)
            .Where(property =>
                property.GetIndexParameters().Length == 0 &&
                (forWrite ? property.GetMethod is not null : property.SetMethod is not null))
            .Cast<MemberInfo>()
            .Concat(type.GetFields(flags).Where(HasMiniExcelAttribute));
        return members
            .Where(member => !IsIgnored(member))
            .Select(member => CreateMapping(member, culture, dynamicColumns))
            .Where(mapping => !mapping.Ignore)
            .ToList();
    }

    private static MemberMapping CreateMapping(
        MemberInfo member,
        CultureInfo culture,
        IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>? dynamicColumns)
    {
        var names = new List<string> { member.Name };
        int? index = null;
        string? format = null;
        Type? resourceType = null;
        foreach (var attribute in member.CustomAttributes)
        {
            var name = attribute.AttributeType.Name;
            if (name is "ExcelColumnNameAttribute" or "MiniExcelColumnNameAttribute")
            {
                AddConstructorName(attribute, names);
                AddNamedString(attribute, "Name", names);
                AddAliases(attribute, names);
                resourceType = ReadNamedType(attribute, "ResourceType") ?? resourceType;
            }
            else if (name is "ExcelColumnIndexAttribute" or "MiniExcelColumnIndexAttribute")
            {
                index = ReadIndex(attribute);
            }
            else if (name is "ExcelColumnAttribute" or "MiniExcelColumnAttribute")
            {
                AddNamedString(attribute, "Name", names);
                AddAliases(attribute, names);
                index = ReadNamedInt(attribute, "Index") ?? index;
                format = ReadNamedString(attribute, "Format") ?? format;
                resourceType = ReadNamedType(attribute, "ResourceType") ?? resourceType;
            }
            else if (name is "ExcelFormatAttribute" or "MiniExcelFormatAttribute")
            {
                format = attribute.ConstructorArguments.FirstOrDefault().Value as string;
            }
        }

        if (member.GetCustomAttribute<DisplayNameAttribute>() is { DisplayName.Length: > 0 } display)
            names.Insert(0, display.DisplayName);
        if (resourceType is not null)
            names[0] = GetLocalizedName(resourceType, names[0], culture);

        var dynamicColumn = dynamicColumns is not null && dynamicColumns.TryGetValue(member.Name, out var configured)
            ? configured
            : null;
        if (!string.IsNullOrWhiteSpace(dynamicColumn?.Name))
            names.Insert(0, dynamicColumn!.Name!);
        index = dynamicColumn?.Index ?? index;
        format = dynamicColumn?.Format ?? format;

        var valueType = member is PropertyInfo property ? property.PropertyType : ((FieldInfo)member).FieldType;
        return new MemberMapping(
            member,
            names.Distinct(StringComparer.OrdinalIgnoreCase).ToArray(),
            index,
            valueType,
            format,
            dynamicColumn?.Ignore is true,
            dynamicColumn?.CustomFormatter);
    }

    private static bool TryGetValue(
        IDictionary<string, object?> row,
        IReadOnlyList<string> names,
        out object? value)
    {
        foreach (var name in names)
        {
            if (row.TryGetValue(name, out value))
                return true;
            var match = row.FirstOrDefault(cell => string.Equals(cell.Key, name, StringComparison.OrdinalIgnoreCase));
            if (match.Key is not null)
            {
                value = match.Value;
                return true;
            }
        }
        value = null;
        return false;
    }

    private static bool Assign(object? source, out object? value)
    {
        value = source;
        return true;
    }

    private static object? ConvertValue(
        object? value,
        Type targetType,
        string? format,
        CultureInfo culture)
    {
        if (value is null || value is DBNull)
        {
            if (!targetType.IsValueType || Nullable.GetUnderlyingType(targetType) is not null)
                return null;
            return Activator.CreateInstance(targetType);
        }

        var effectiveType = Nullable.GetUnderlyingType(targetType) ?? targetType;
        if (effectiveType.IsInstanceOfType(value))
            return value;
        if (effectiveType == typeof(string))
            return Convert.ToString(value, culture);
        if (effectiveType == typeof(Guid))
            return Guid.Parse(Convert.ToString(value, culture)!);
        if (effectiveType == typeof(Uri))
            return new Uri(Convert.ToString(value, culture)!, UriKind.RelativeOrAbsolute);
        if (effectiveType == typeof(DateTime))
        {
            if (value is double serial)
                return DateTime.FromOADate(serial);
            var text = Convert.ToString(value, culture)!;
            return format is null
                ? DateTime.Parse(text, culture)
                : DateTime.ParseExact(text, format, CultureInfo.InvariantCulture);
        }
        if (effectiveType == typeof(DateTimeOffset))
            return DateTimeOffset.Parse(Convert.ToString(value, culture)!, culture);
        if (effectiveType == typeof(TimeSpan))
        {
            if (value is double milliseconds)
                return TimeSpan.FromMilliseconds(milliseconds);
            var text = Convert.ToString(value, culture)!;
            return format is null
                ? TimeSpan.Parse(text, culture)
                : TimeSpan.ParseExact(text, format, CultureInfo.InvariantCulture);
        }
        if (effectiveType == typeof(bool))
        {
            var text = Convert.ToString(value, culture);
            return text switch { "1" => true, "0" => false, _ => bool.Parse(text!) };
        }
        if (effectiveType.IsEnum)
        {
            var text = Convert.ToString(value, culture)!;
            var described = effectiveType.GetFields()
                .FirstOrDefault(field => field.GetCustomAttribute<DescriptionAttribute>()?.Description == text);
            return Enum.Parse(effectiveType, described?.Name ?? text, ignoreCase: true);
        }
        return Convert.ChangeType(value, effectiveType, culture);
    }

    private static object? NormalizeWriteValue(object? value)
    {
        if (value is null)
            return null;
        var type = value.GetType();
        if (type.IsEnum)
        {
            var field = type.GetField(value.ToString()!);
            return field?.GetCustomAttribute<DescriptionAttribute>()?.Description ?? value.ToString();
        }
        if (value is Guid or Uri)
            return value.ToString();
        return value;
    }

    private static bool HasMiniExcelAttribute(MemberInfo member) =>
        member.CustomAttributes.Any(attribute => attribute.AttributeType.Name.IndexOf("Excel", StringComparison.Ordinal) >= 0);

    private static bool IsIgnored(MemberInfo member) => member.CustomAttributes.Any(attribute =>
        (attribute.AttributeType.Name is "ExcelIgnoreAttribute" or "MiniExcelIgnoreAttribute" &&
         (attribute.ConstructorArguments.Count == 0 || attribute.ConstructorArguments[0].Value is not false)) ||
        (attribute.AttributeType.Name is "ExcelColumnAttribute" or "MiniExcelColumnAttribute" &&
         attribute.NamedArguments.Any(argument => argument.MemberName == "Ignore" && argument.TypedValue.Value is true)));

    private static void AddConstructorName(CustomAttributeData attribute, IList<string> names)
    {
        if (attribute.ConstructorArguments.Count > 0 && attribute.ConstructorArguments[0].Value is string value && value.Length > 0)
            names.Insert(0, value);
    }

    private static void AddNamedString(CustomAttributeData attribute, string propertyName, IList<string> names)
    {
        var argument = attribute.NamedArguments.FirstOrDefault(item => item.MemberName == propertyName);
        if (argument.TypedValue.Value is string value && value.Length > 0)
            names.Insert(0, value);
    }

    private static void AddAliases(CustomAttributeData attribute, ICollection<string> names)
    {
        if (attribute.ConstructorArguments.Count > 1 &&
            attribute.ConstructorArguments[1].Value is IEnumerable<CustomAttributeTypedArgument> constructorAliases)
        {
            foreach (var alias in constructorAliases)
            {
                if (alias.Value is string value && value.Length > 0)
                    names.Add(value);
            }
        }
        var argument = attribute.NamedArguments.FirstOrDefault(item => item.MemberName == "Aliases");
        if (argument.TypedValue.Value is IEnumerable<CustomAttributeTypedArgument> aliases)
        {
            foreach (var alias in aliases)
            {
                if (alias.Value is string value && value.Length > 0)
                    names.Add(value);
            }
        }
    }

    private static int? ReadIndex(CustomAttributeData attribute)
    {
        if (attribute.ConstructorArguments.Count == 0)
            return null;
        var value = attribute.ConstructorArguments[0].Value;
        if (value is int index)
            return index;
        if (value is string columnName)
            return ColumnNameToIndex(columnName);
        return null;
    }

    private static int? ReadNamedInt(CustomAttributeData attribute, string propertyName)
    {
        var argument = attribute.NamedArguments.FirstOrDefault(item => item.MemberName == propertyName);
        return argument.TypedValue.Value is int value && value >= 0 ? value : null;
    }

    private static string? ReadNamedString(CustomAttributeData attribute, string propertyName)
    {
        var argument = attribute.NamedArguments.FirstOrDefault(item => item.MemberName == propertyName);
        return argument.TypedValue.Value as string;
    }

    private static Type? ReadNamedType(CustomAttributeData attribute, string propertyName)
    {
        var argument = attribute.NamedArguments.FirstOrDefault(item => item.MemberName == propertyName);
        return argument.TypedValue.Value as Type;
    }

    private static string GetLocalizedName(Type resourceType, string key, CultureInfo culture)
    {
        const BindingFlags flags = BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic;
        var manager = resourceType.GetProperty(nameof(ResourceManager), flags)?.GetValue(null) as ResourceManager
            ?? new ResourceManager(resourceType);
        return manager.GetString(key, culture) ?? key;
    }

    private static int ColumnNameToIndex(string columnName)
    {
        var index = 0;
        foreach (var character in columnName.ToUpperInvariant())
        {
            if (character is < 'A' or > 'Z')
                throw new ArgumentException($"Invalid Excel column name '{columnName}'.");
            index = checked(index * 26 + character - 'A' + 1);
        }
        return index - 1;
    }

    private sealed class MemberMapping(
        MemberInfo member,
        string[] names,
        int? index,
        Type valueType,
        string? format,
        bool ignore,
        Func<object?, object?>? customFormatter)
    {
        public string[] Names { get; } = names;
        public int? Index { get; } = index;
        public Type ValueType { get; } = valueType;
        public string? Format { get; } = format;
        public bool Ignore { get; } = ignore;

        public void SetValue(object target, object? value)
        {
            if (member is PropertyInfo property)
                property.SetValue(target, value);
            else
                ((FieldInfo)member).SetValue(target, value);
        }

        public object? GetValue(object target)
        {
            return member is PropertyInfo property
                ? property.GetValue(target)
                : ((FieldInfo)member).GetValue(target);
        }

        public object? FormatValue(object? value)
        {
            if (customFormatter is null)
                return value;
            try
            {
                return customFormatter(value);
            }
            catch
            {
                return value;
            }
        }
    }
}