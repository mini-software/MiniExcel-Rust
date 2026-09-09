using System.Collections;
using System.Data;
using System.Globalization;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace MiniExcelLibs;

/// <summary>
/// Provides XLSX queries backed by the native MiniExcel Rust library.
/// </summary>
public static class MiniExcelRust
{
    private const int BatchSize = 64;

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryAsync(
        string path,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null,
        CancellationToken cancellationToken = default)
    {
        return ToAsyncEnumerable(
            Query(path, useHeaderRow, sheetName, startCell, configuration),
            cancellationToken);
    }

    public static IAsyncEnumerable<T> QueryAsync<T>(
        string path,
        string? sheetName = null,
        string startCell = "A1",
        bool treatHeaderAsData = false,
        MiniExcelRustReadOptions? configuration = null,
        CancellationToken cancellationToken = default)
        where T : class, new()
    {
        return ToAsyncEnumerable(
            Query<T>(path, sheetName, startCell, treatHeaderAsData, configuration),
            cancellationToken);
    }

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryAsync(
        Stream stream,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false,
        CancellationToken cancellationToken = default) =>
        ToAsyncEnumerable(
            Query(stream, useHeaderRow, sheetName, startCell, configuration, leaveOpen),
            cancellationToken);

    public static IAsyncEnumerable<T> QueryAsync<T>(
        Stream stream,
        string? sheetName = null,
        string startCell = "A1",
        bool treatHeaderAsData = false,
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false,
        CancellationToken cancellationToken = default)
        where T : class, new() =>
        ToAsyncEnumerable(
            Query<T>(stream, sheetName, startCell, treatHeaderAsData, configuration, leaveOpen),
            cancellationToken);

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryRangeAsync(
        string path,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        string? endCell = null,
        MiniExcelRustReadOptions? configuration = null,
        CancellationToken cancellationToken = default)
    {
        return ToAsyncEnumerable(
            QueryRange(path, useHeaderRow, sheetName, startCell, endCell, configuration),
            cancellationToken);
    }

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryRangeAsync(
        Stream stream,
        bool useHeaderRow,
        string? sheetName,
        int startRowIndex,
        int startColumnIndex,
        int? endRowIndex = null,
        int? endColumnIndex = null,
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false,
        CancellationToken cancellationToken = default) =>
        ToAsyncEnumerable(
            QueryRange(
                stream,
                useHeaderRow,
                sheetName,
                startRowIndex,
                startColumnIndex,
                endRowIndex,
                endColumnIndex,
                configuration,
                leaveOpen),
            cancellationToken);

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryRangeAsync(
        Stream stream,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        string? endCell = null,
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false,
        CancellationToken cancellationToken = default) =>
        ToAsyncEnumerable(
            QueryRange(stream, useHeaderRow, sheetName, startCell, endCell, configuration, leaveOpen),
            cancellationToken);

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryTableAsync(
        string path,
        string? sheetName = null,
        string tableName = "Table1",
        CancellationToken cancellationToken = default)
    {
        return ToAsyncEnumerable(QueryTable(path, sheetName, tableName), cancellationToken);
    }

    public static IAsyncEnumerable<T> QueryTableAsync<T>(
        string path,
        string? sheetName = null,
        string tableName = "Table1",
        CancellationToken cancellationToken = default)
        where T : class, new() =>
        ToAsyncEnumerable(QueryTable<T>(path, sheetName, tableName), cancellationToken);

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryTableAsync(
        Stream stream,
        string? sheetName = null,
        string tableName = "Table1",
        bool leaveOpen = false,
        CancellationToken cancellationToken = default) =>
        ToAsyncEnumerable(QueryTable(stream, sheetName, tableName, leaveOpen), cancellationToken);

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryCsvAsync(
        string path,
        bool useHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null,
        CancellationToken cancellationToken = default)
    {
        return ToAsyncEnumerable(QueryCsv(path, useHeaderRow, configuration), cancellationToken);
    }

    public static IAsyncEnumerable<T> QueryCsvAsync<T>(
        string path,
        bool treatHeaderAsData = false,
        MiniExcelRustCsvReadOptions? configuration = null,
        CancellationToken cancellationToken = default)
        where T : class, new() =>
        ToAsyncEnumerable(QueryCsv<T>(path, treatHeaderAsData, configuration), cancellationToken);

    public static IEnumerable<T> Query<T>(
        string path,
        string? sheetName = null,
        string startCell = "A1",
        bool treatHeaderAsData = false,
        MiniExcelRustReadOptions? configuration = null)
        where T : class, new()
    {
        return MiniExcelRustMapper.Map<T>(
            Query(path, !treatHeaderAsData, sheetName, startCell, configuration),
            configuration?.Culture,
            configuration?.DynamicColumns as IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>);
    }

    public static IEnumerable<T> Query<T>(
        Stream stream,
        string? sheetName = null,
        string startCell = "A1",
        bool treatHeaderAsData = false,
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false)
        where T : class, new()
    {
        return MiniExcelRustMapper.Map<T>(
            Query(stream, !treatHeaderAsData, sheetName, startCell, configuration, leaveOpen),
            configuration?.Culture,
            configuration?.DynamicColumns as IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>);
    }

    public static IEnumerable<T> QueryRange<T>(
        string path,
        string? sheetName = null,
        string startCell = "A1",
        string? endCell = null,
        bool treatHeaderAsData = false,
        MiniExcelRustReadOptions? configuration = null)
        where T : class, new()
    {
        return MiniExcelRustMapper.Map<T>(
            QueryRange(path, !treatHeaderAsData, sheetName, startCell, endCell, configuration),
            configuration?.Culture,
            configuration?.DynamicColumns as IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>);
    }

    public static IEnumerable<T> QueryRange<T>(
        Stream stream,
        string? sheetName = null,
        string startCell = "A1",
        string? endCell = null,
        bool treatHeaderAsData = false,
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false)
        where T : class, new() =>
        MiniExcelRustMapper.Map<T>(
            QueryRange(
                stream,
                !treatHeaderAsData,
                sheetName,
                startCell,
                endCell,
                configuration,
                leaveOpen),
            configuration?.Culture,
            configuration?.DynamicColumns as IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>);

    public static IEnumerable<T> QueryTable<T>(
        string path,
        string? sheetName = null,
        string tableName = "Table1")
        where T : class, new()
    {
        return MiniExcelRustMapper.Map<T>(QueryTable(path, sheetName, tableName));
    }

    public static IEnumerable<T> QueryTable<T>(
        Stream stream,
        string? sheetName = null,
        string tableName = "Table1",
        bool leaveOpen = false)
        where T : class, new() =>
        MiniExcelRustMapper.Map<T>(QueryTable(stream, sheetName, tableName, leaveOpen));

    public static IEnumerable<T> QueryCsv<T>(
        string path,
        bool treatHeaderAsData = false,
        MiniExcelRustCsvReadOptions? configuration = null)
        where T : class, new()
    {
        return MiniExcelRustMapper.Map<T>(
            QueryCsv(path, !treatHeaderAsData, configuration),
            configuration?.Culture,
            configuration?.DynamicColumns as IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>);
    }

    public static IEnumerable<T> QueryCsv<T>(
        Stream stream,
        bool treatHeaderAsData = false,
        MiniExcelRustCsvReadOptions? configuration = null,
        bool leaveOpen = false)
        where T : class, new()
    {
        return MiniExcelRustMapper.Map<T>(
            QueryCsv(stream, !treatHeaderAsData, configuration, leaveOpen),
            configuration?.Culture,
            configuration?.DynamicColumns as IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>);
    }

    /// <summary>
    /// Returns worksheet names in workbook order.
    /// </summary>
    public static List<string> GetSheetNames(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));

        EnsureAbiVersion();

        using var nativePath = new Utf8String(Path.GetFullPath(path));
        var result = NativeMethods.GetSheetNames(
            nativePath.Pointer,
            out var rawHandle,
            out var data,
            out var length);
        if (result < 0)
            throw CreateNativeException(result);

        using var handle = new NativeBufferHandle(rawHandle);
        var byteLength = checked((int)length.ToUInt64());
        var frame = new byte[byteLength];
        Marshal.Copy(data, frame, 0, byteLength);
        return DecodeStrings(frame);
    }

    /// <summary>
    /// Returns worksheet names from a stream in workbook order.
    /// </summary>
    public static List<string> GetSheetNames(Stream stream, bool leaveOpen = false)
    {
        return UseStagedStream(stream, leaveOpen, GetSheetNames);
    }

    /// <summary>
    /// Asynchronously returns worksheet names in workbook order.
    /// </summary>
    public static Task<List<string>> GetSheetNamesAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() => GetSheetNames(path), cancellationToken);
    }

    /// <summary>
    /// Returns selected column names from an XLSX worksheet.
    /// </summary>
    public static List<string> GetColumnNames(
        string path,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1")
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (string.IsNullOrWhiteSpace(startCell))
            throw new ArgumentException("The start cell is required.", nameof(startCell));

        EnsureAbiVersion();

        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        using var nativeStartCell = new Utf8String(startCell);
        var result = NativeMethods.GetColumns(
            nativePath.Pointer,
            useHeaderRow ? (byte)1 : (byte)0,
            nativeSheetName.Pointer,
            nativeStartCell.Pointer,
            out var rawHandle,
            out var data,
            out var length);
        if (result < 0)
            throw CreateNativeException(result);

        using var handle = new NativeBufferHandle(rawHandle);
        var byteLength = checked((int)length.ToUInt64());
        var frame = new byte[byteLength];
        Marshal.Copy(data, frame, 0, byteLength);
        return DecodeStrings(frame);
    }

    /// <summary>
    /// Returns selected column names from an XLSX stream.
    /// </summary>
    public static List<string> GetColumnNames(
        Stream stream,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        bool leaveOpen = false)
    {
        return UseStagedStream(
            stream,
            leaveOpen,
            path => GetColumnNames(path, useHeaderRow, sheetName, startCell));
    }

    /// <summary>
    /// Asynchronously returns selected column names from an XLSX worksheet.
    /// </summary>
    public static Task<List<string>> GetColumnNamesAsync(
        string path,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        CancellationToken cancellationToken = default)
    {
        return Task.Run(
            () => GetColumnNames(path, useHeaderRow, sheetName, startCell),
            cancellationToken);
    }

    /// <summary>
    /// Returns the used range of every worksheet in workbook order.
    /// </summary>
    public static List<MiniExcelRustRange> GetSheetDimensions(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));

        EnsureAbiVersion();

        using var nativePath = new Utf8String(Path.GetFullPath(path));
        var result = NativeMethods.GetSheetDimensions(
            nativePath.Pointer,
            out var rawHandle,
            out var data,
            out var length);
        if (result < 0)
            throw CreateNativeException(result);

        using var handle = new NativeBufferHandle(rawHandle);
        var byteLength = checked((int)length.ToUInt64());
        var frame = new byte[byteLength];
        Marshal.Copy(data, frame, 0, byteLength);
        return DecodeRanges(frame);
    }

    /// <summary>
    /// Returns the used range of every worksheet in an XLSX stream.
    /// </summary>
    public static List<MiniExcelRustRange> GetSheetDimensions(Stream stream, bool leaveOpen = false)
    {
        return UseStagedStream(stream, leaveOpen, GetSheetDimensions);
    }

    /// <summary>
    /// Asynchronously returns the used range of every worksheet in workbook order.
    /// </summary>
    public static Task<List<MiniExcelRustRange>> GetSheetDimensionsAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() => GetSheetDimensions(path), cancellationToken);
    }

    /// <summary>
    /// Returns detailed information for every sheet in workbook order.
    /// </summary>
    public static List<MiniExcelRustSheetInfo> GetSheetInformations(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));

        EnsureAbiVersion();

        using var nativePath = new Utf8String(Path.GetFullPath(path));
        var result = NativeMethods.GetSheetInfo(
            nativePath.Pointer,
            out var rawHandle,
            out var data,
            out var length);
        if (result < 0)
            throw CreateNativeException(result);

        using var handle = new NativeBufferHandle(rawHandle);
        var byteLength = checked((int)length.ToUInt64());
        var frame = new byte[byteLength];
        Marshal.Copy(data, frame, 0, byteLength);
        return DecodeSheetInfo(frame);
    }

    /// <summary>
    /// Returns detailed information for every sheet in an XLSX stream.
    /// </summary>
    public static List<MiniExcelRustSheetInfo> GetSheetInformations(
        Stream stream,
        bool leaveOpen = false)
    {
        return UseStagedStream(stream, leaveOpen, GetSheetInformations);
    }

    /// <summary>
    /// Asynchronously returns detailed information for every sheet in workbook order.
    /// </summary>
    public static Task<List<MiniExcelRustSheetInfo>> GetSheetInformationsAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() => GetSheetInformations(path), cancellationToken);
    }

    public static IDictionary<string, object?> ReadMapped(
        string path,
        IReadOnlyDictionary<string, string> mapping,
        string? sheetName = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (mapping is null || mapping.Count == 0)
            throw new ArgumentException("At least one cell mapping is required.", nameof(mapping));

        EnsureAbiVersion();
        var mappingFrame = EncodeMapping(mapping);
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        var mappingHandle = GCHandle.Alloc(mappingFrame, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.ReadMapped(
                nativePath.Pointer,
                nativeSheetName.Pointer,
                mappingHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)mappingFrame.Length,
                out var rawHandle,
                out var data,
                out var length);
            if (result < 0)
                throw CreateNativeException(result);
            using var handle = new NativeBufferHandle(rawHandle);
            var frame = new byte[checked((int)length.ToUInt64())];
            Marshal.Copy(data, frame, 0, frame.Length);
            return DecodeBatch(frame).Single();
        }
        finally
        {
            mappingHandle.Free();
        }
    }

    public static T ReadMapped<T>(
        string path,
        IReadOnlyDictionary<string, string> mapping,
        string? sheetName = null)
        where T : class, new() =>
        MiniExcelRustMapper.Map<T>(new[] { ReadMapped(path, mapping, sheetName) }).Single();

    public static T ReadMapped<T>(
        Stream stream,
        IReadOnlyDictionary<string, string> mapping,
        string? sheetName = null,
        bool leaveOpen = false)
        where T : class, new() =>
        UseStagedStream(stream, leaveOpen, path => ReadMapped<T>(path, mapping, sheetName));

    /// <summary>
    /// Returns threaded comments, replies, and legacy notes from an XLSX worksheet.
    /// </summary>
    public static MiniExcelRustCommentResult RetrieveComments(string path, string? sheetName = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));

        EnsureAbiVersion();
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        var result = NativeMethods.GetComments(
            nativePath.Pointer,
            nativeSheetName.Pointer,
            out var rawHandle,
            out var data,
            out var length);
        if (result < 0)
            throw CreateNativeException(result);

        using var handle = new NativeBufferHandle(rawHandle);
        var byteLength = checked((int)length.ToUInt64());
        var frame = new byte[byteLength];
        Marshal.Copy(data, frame, 0, byteLength);
        return DecodeComments(frame);
    }

    /// <summary>
    /// Returns threaded comments, replies, and legacy notes from an XLSX stream.
    /// </summary>
    public static MiniExcelRustCommentResult RetrieveComments(
        Stream stream,
        string? sheetName = null,
        bool leaveOpen = false)
    {
        return UseStagedStream(stream, leaveOpen, path => RetrieveComments(path, sheetName));
    }

    /// <summary>
    /// Asynchronously returns comments and notes from an XLSX worksheet.
    /// </summary>
    public static Task<MiniExcelRustCommentResult> RetrieveCommentsAsync(
        string path,
        string? sheetName = null,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() => RetrieveComments(path, sheetName), cancellationToken);
    }

    /// <summary>
    /// Materializes an XLSX query as a DataTable.
    /// </summary>
    public static DataTable QueryAsDataTable(
        string path,
        bool hasHeaderRow = true,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));

        var fullPath = Path.GetFullPath(path);
        var columns = GetColumnNames(fullPath, hasHeaderRow, sheetName, startCell);
        var rows = Query(fullPath, hasHeaderRow, sheetName, startCell, configuration);
        return CreateDataTable(columns, rows);
    }

    /// <summary>
    /// Materializes an XLSX stream query as a DataTable.
    /// </summary>
    public static DataTable QueryAsDataTable(
        Stream stream,
        bool hasHeaderRow = true,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        return UseStagedStream(
            stream,
            leaveOpen,
            path => QueryAsDataTable(path, hasHeaderRow, sheetName, startCell, configuration));
    }

    /// <summary>
    /// Returns a DataReader over a materialized Rust-backed XLSX query.
    /// </summary>
    public static IDataReader GetReader(
        string path,
        bool hasHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null)
    {
        if (sheetName is not null)
            return QueryAsDataTable(path, hasHeaderRow, sheetName, startCell, configuration).CreateDataReader();

        var dataSet = new DataSet();
        foreach (var name in GetSheetNames(path))
        {
            var table = QueryAsDataTable(path, hasHeaderRow, name, startCell, configuration);
            table.TableName = name;
            dataSet.Tables.Add(table);
        }
        return dataSet.CreateDataReader();
    }

    /// <summary>
    /// Returns a DataReader over a materialized Rust-backed XLSX stream query.
    /// </summary>
    public static IDataReader GetReader(
        Stream stream,
        bool hasHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        return UseStagedStream(
            stream,
            leaveOpen,
            path => GetReader(path, hasHeaderRow, sheetName, startCell, configuration));
    }

    /// <summary>
    /// Streams rows from an XLSX file through the native Rust query engine.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> Query(
        string path,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (string.IsNullOrWhiteSpace(startCell))
            throw new ArgumentException("The start cell is required.", nameof(startCell));

        return QueryIterator(Path.GetFullPath(path), useHeaderRow, sheetName, startCell, null, configuration);
    }

    /// <summary>
    /// Streams rows from an XLSX stream through the native Rust query engine.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> Query(
        Stream stream,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        ValidateReadableStream(stream);
        if (string.IsNullOrWhiteSpace(startCell))
            throw new ArgumentException("The start cell is required.", nameof(startCell));

        return QueryStreamIterator(stream, useHeaderRow, sheetName, startCell, null, configuration, leaveOpen);
    }

    /// <summary>
    /// Streams rows from an inclusive XLSX cell range through the native Rust query engine.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> QueryRange(
        string path,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        string? endCell = null,
        MiniExcelRustReadOptions? configuration = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (string.IsNullOrWhiteSpace(startCell))
            throw new ArgumentException("The start cell is required.", nameof(startCell));
        if (endCell is not null && string.IsNullOrWhiteSpace(endCell))
            throw new ArgumentException("The end cell cannot be empty.", nameof(endCell));

        return QueryIterator(Path.GetFullPath(path), useHeaderRow, sheetName, startCell, endCell, configuration);
    }

    /// <summary>
    /// Streams rows from an inclusive XLSX range in a stream through the native Rust query engine.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> QueryRange(
        Stream stream,
        bool useHeaderRow = false,
        string? sheetName = null,
        string startCell = "A1",
        string? endCell = null,
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        ValidateReadableStream(stream);
        if (string.IsNullOrWhiteSpace(startCell))
            throw new ArgumentException("The start cell is required.", nameof(startCell));
        if (endCell is not null && string.IsNullOrWhiteSpace(endCell))
            throw new ArgumentException("The end cell cannot be empty.", nameof(endCell));

        return QueryStreamIterator(stream, useHeaderRow, sheetName, startCell, endCell, configuration, leaveOpen);
    }

    public static IEnumerable<IDictionary<string, object?>> QueryRange(
        string path,
        bool useHeaderRow,
        string? sheetName,
        int startRowIndex,
        int startColumnIndex,
        int? endRowIndex = null,
        int? endColumnIndex = null,
        MiniExcelRustReadOptions? configuration = null)
    {
        var startCell = ToCellReference(startRowIndex, startColumnIndex);
        var endCell = endRowIndex.HasValue || endColumnIndex.HasValue
            ? ToCellReference(endRowIndex ?? 1_048_576, endColumnIndex ?? 16_384)
            : null;
        return QueryRange(path, useHeaderRow, sheetName, startCell, endCell, configuration);
    }

    public static IEnumerable<IDictionary<string, object?>> QueryRange(
        Stream stream,
        bool useHeaderRow,
        string? sheetName,
        int startRowIndex,
        int startColumnIndex,
        int? endRowIndex = null,
        int? endColumnIndex = null,
        MiniExcelRustReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        var startCell = ToCellReference(startRowIndex, startColumnIndex);
        var endCell = endRowIndex.HasValue || endColumnIndex.HasValue
            ? ToCellReference(endRowIndex ?? 1_048_576, endColumnIndex ?? 16_384)
            : null;
        return QueryRange(
            stream,
            useHeaderRow,
            sheetName,
            startCell,
            endCell,
            configuration,
            leaveOpen);
    }

    public static IAsyncEnumerable<IDictionary<string, object?>> QueryRangeAsync(
        string path,
        bool useHeaderRow,
        string? sheetName,
        int startRowIndex,
        int startColumnIndex,
        int? endRowIndex = null,
        int? endColumnIndex = null,
        MiniExcelRustReadOptions? configuration = null,
        CancellationToken cancellationToken = default) =>
        ToAsyncEnumerable(
            QueryRange(
                path,
                useHeaderRow,
                sheetName,
                startRowIndex,
                startColumnIndex,
                endRowIndex,
                endColumnIndex,
                configuration),
            cancellationToken);

    /// <summary>
    /// Streams rows from a named OpenXML table.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> QueryTable(
        string path,
        string? sheetName = null,
        string tableName = "Table1")
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (string.IsNullOrWhiteSpace(tableName))
            throw new ArgumentException("The table name is required.", nameof(tableName));

        return QueryTableIterator(Path.GetFullPath(path), sheetName, tableName);
    }

    /// <summary>
    /// Streams rows from a named OpenXML table in a stream.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> QueryTable(
        Stream stream,
        string? sheetName = null,
        string tableName = "Table1",
        bool leaveOpen = false)
    {
        ValidateReadableStream(stream);
        if (string.IsNullOrWhiteSpace(tableName))
            throw new ArgumentException("The table name is required.", nameof(tableName));

        return QueryTableStreamIterator(stream, sheetName, tableName, leaveOpen);
    }

    /// <summary>
    /// Streams rows from a CSV file through the native Rust query engine.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> QueryCsv(
        string path,
        bool useHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        ValidateCsvConfiguration(configuration);

        return QueryCsvIterator(Path.GetFullPath(path), useHeaderRow, configuration);
    }

    /// <summary>
    /// Streams rows from a CSV stream through the native Rust query engine.
    /// </summary>
    public static IEnumerable<IDictionary<string, object?>> QueryCsv(
        Stream stream,
        bool useHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        ValidateReadableStream(stream);
        ValidateCsvConfiguration(configuration);

        return QueryCsvStreamIterator(stream, useHeaderRow, configuration, leaveOpen);
    }

    /// <summary>
    /// Returns selected column names from a CSV file.
    /// </summary>
    public static List<string> GetCsvColumnNames(
        string path,
        bool useHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        ValidateCsvConfiguration(configuration);
        EnsureAbiVersion();
        configuration ??= new MiniExcelRustCsvReadOptions();

        using var nativePath = new Utf8String(Path.GetFullPath(path));
        var result = NativeMethods.GetCsvColumns(
            nativePath.Pointer,
            useHeaderRow ? (byte)1 : (byte)0,
            (byte)configuration.Delimiter,
            (byte)configuration.Encoding,
            configuration.ReadEmptyStringAsNull ? (byte)1 : (byte)0,
            configuration.TrimColumnNames ? (byte)1 : (byte)0,
            out var rawHandle,
            out var data,
            out var length);
        if (result < 0)
            throw CreateNativeException(result);

        using var handle = new NativeBufferHandle(rawHandle);
        var byteLength = checked((int)length.ToUInt64());
        var frame = new byte[byteLength];
        Marshal.Copy(data, frame, 0, byteLength);
        return DecodeStrings(frame);
    }

    /// <summary>
    /// Returns selected column names from a CSV stream.
    /// </summary>
    public static List<string> GetCsvColumnNames(
        Stream stream,
        bool useHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        return UseStagedStream(
            stream,
            leaveOpen,
            path => GetCsvColumnNames(path, useHeaderRow, configuration));
    }

    /// <summary>
    /// Asynchronously returns selected column names from a CSV file.
    /// </summary>
    public static Task<List<string>> GetCsvColumnNamesAsync(
        string path,
        bool useHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(
            () => GetCsvColumnNames(path, useHeaderRow, configuration),
            cancellationToken);
    }

    /// <summary>
    /// Materializes a CSV query as a DataTable.
    /// </summary>
    public static DataTable QueryCsvAsDataTable(
        string path,
        bool hasHeaderRow = true,
        MiniExcelRustCsvReadOptions? configuration = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));

        var fullPath = Path.GetFullPath(path);
        var columns = GetCsvColumnNames(fullPath, hasHeaderRow, configuration);
        var rows = QueryCsv(fullPath, hasHeaderRow, configuration);
        return CreateDataTable(columns, rows);
    }

    /// <summary>
    /// Materializes a CSV stream query as a DataTable.
    /// </summary>
    public static DataTable QueryCsvAsDataTable(
        Stream stream,
        bool hasHeaderRow = true,
        MiniExcelRustCsvReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        return UseStagedStream(
            stream,
            leaveOpen,
            path => QueryCsvAsDataTable(path, hasHeaderRow, configuration));
    }

    /// <summary>
    /// Returns a DataReader over a materialized Rust-backed CSV query.
    /// </summary>
    public static IDataReader GetCsvReader(
        string path,
        bool hasHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null)
    {
        return QueryCsvAsDataTable(path, hasHeaderRow, configuration).CreateDataReader();
    }

    /// <summary>
    /// Returns a DataReader over a materialized Rust-backed CSV stream query.
    /// </summary>
    public static IDataReader GetCsvReader(
        Stream stream,
        bool hasHeaderRow = false,
        MiniExcelRustCsvReadOptions? configuration = null,
        bool leaveOpen = false)
    {
        return QueryCsvAsDataTable(stream, hasHeaderRow, configuration, leaveOpen).CreateDataReader();
    }

    /// <summary>
    /// Creates a single-sheet XLSX workbook from dynamic rows.
    /// </summary>
    public static int SaveAs(
        string path,
        IEnumerable<IDictionary<string, object?>> rows,
        bool printHeader = true,
        string sheetName = "Sheet1",
        bool overwriteFile = false,
        IProgress<int>? progress = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        if (string.IsNullOrWhiteSpace(sheetName))
            throw new ArgumentException("The sheet name is required.", nameof(sheetName));

        EnsureAbiVersion();
        var materializedRows = rows.ToList();
        var frame = EncodeRows(materializedRows);
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        var frameHandle = GCHandle.Alloc(frame, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.SaveAs(
                nativePath.Pointer,
                frameHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)frame.Length,
                printHeader ? (byte)1 : (byte)0,
                nativeSheetName.Pointer,
                overwriteFile ? (byte)1 : (byte)0,
                out var rowCount);
            if (result < 0)
                throw CreateNativeException(result);
            ReportProgress(progress, materializedRows);
            return checked((int)rowCount);
        }
        finally
        {
            frameHandle.Free();
        }
    }

    public static int SaveAs<T>(
        string path,
        IEnumerable<T> rows,
        bool printHeader = true,
        string sheetName = "Sheet1",
        bool overwriteFile = false,
        IProgress<int>? progress = null)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        if (rows is IEnumerable<IDictionary<string, object?>> dynamicRows)
            return SaveAs(path, dynamicRows, printHeader, sheetName, overwriteFile, progress);
        return SaveAs(
            path,
            MiniExcelRustMapper.ToRows(rows),
            printHeader,
            sheetName,
            overwriteFile,
            progress);
    }

    public static int SaveAs<T>(
        Stream stream,
        IEnumerable<T> rows,
        bool printHeader = true,
        string sheetName = "Sheet1",
        bool leaveOpen = false,
        IProgress<int>? progress = null)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        if (rows is IEnumerable<IDictionary<string, object?>> dynamicRows)
            return SaveAs(stream, dynamicRows, printHeader, sheetName, leaveOpen, progress);
        return SaveAs(stream, MiniExcelRustMapper.ToRows(rows), printHeader, sheetName, leaveOpen, progress);
    }

    public static async Task<int> SaveAsAsync<T>(
        string path,
        IAsyncEnumerable<T> rows,
        bool printHeader = true,
        string sheetName = "Sheet1",
        bool overwriteFile = false,
        IProgress<int>? progress = null,
        CancellationToken cancellationToken = default)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        cancellationToken.ThrowIfCancellationRequested();
        var spoolPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-spool-{Guid.NewGuid():N}.bin");
        try
        {
            List<string>? schema = null;
            long cellCount = 0;
            using (var spool = new FileStream(
                spoolPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                81920,
                useAsync: true))
            {
                await foreach (var value in rows.WithCancellation(cancellationToken).ConfigureAwait(false))
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    var row = value is IDictionary<string, object?> dynamicRow
                        ? dynamicRow
                        : MiniExcelRustMapper.ToRows(new[] { value }).Single();
                    schema ??= row.Keys.ToList();
                    cellCount += row.Count;
                    var frame = EncodeRows(new[] { row });
                    var length = BitConverter.GetBytes(checked((uint)frame.Length));
                    await spool.WriteAsync(length, 0, length.Length, cancellationToken).ConfigureAwait(false);
                    await spool.WriteAsync(frame, 0, frame.Length, cancellationToken).ConfigureAwait(false);
                }
            }
            if (schema is null)
                throw new InvalidOperationException("Async export requires at least one row to infer its schema.");

            var payload = MiniExcelBinaryPayload.EncodeXlsxWrite(
                schema,
                new MiniExcelRustWriteOptions
                {
                    SheetName = sheetName,
                    OverwriteFile = overwriteFile,
                    PrintHeader = printHeader
                });
            using var nativePath = new Utf8String(Path.GetFullPath(path));
            using var nativeSpoolPath = new Utf8String(spoolPath);
            using var nativeCancellation = NativeCancellationHandle.Create();
            using var registration = cancellationToken.Register(
                static state => NativeMethods.Cancel((NativeCancellationHandle)state!),
                nativeCancellation);
            var payloadHandle = GCHandle.Alloc(payload, GCHandleType.Pinned);
            try
            {
                var nativeResult = await Task.Run(() =>
                {
                    var result = NativeMethods.SaveAsSpooledAsync(
                        nativePath.Pointer,
                        nativeSpoolPath.Pointer,
                        payloadHandle.AddrOfPinnedObject(),
                        (UIntPtr)(uint)payload.Length,
                        nativeCancellation,
                        out var rowCount);
                    return (Result: result, RowCount: rowCount);
                }).ConfigureAwait(false);
                if (nativeResult.Result < 0)
                {
                    if (cancellationToken.IsCancellationRequested)
                        throw new OperationCanceledException(cancellationToken);
                    throw CreateNativeException(nativeResult.Result);
                }
                if (progress is not null)
                {
                    for (long index = 0; index < cellCount; index++)
                        progress.Report(1);
                }
                return checked((int)nativeResult.RowCount);
            }
            finally
            {
                payloadHandle.Free();
            }
        }
        finally
        {
            DeleteTemporaryFile(spoolPath);
        }
    }

    public static async Task<int> SaveAsAsync<T>(
        Stream stream,
        IAsyncEnumerable<T> rows,
        bool printHeader = true,
        string sheetName = "Sheet1",
        bool leaveOpen = false,
        IProgress<int>? progress = null,
        CancellationToken cancellationToken = default)
    {
        ValidateWritableStream(stream);
        var temporaryPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.xlsx");
        try
        {
            var count = await SaveAsAsync(
                temporaryPath,
                rows,
                printHeader,
                sheetName,
                false,
                progress,
                cancellationToken).ConfigureAwait(false);
            CopyFileToStream(temporaryPath, stream);
            return count;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    public static int[] SaveAsSheets(
        string path,
        IEnumerable<KeyValuePair<string, IEnumerable<IDictionary<string, object?>>>> sheets,
        bool printHeader = true,
        bool overwriteFile = false)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (sheets is null)
            throw new ArgumentNullException(nameof(sheets));

        EnsureAbiVersion();
        var frame = EncodeSheets(sheets);
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        var frameHandle = GCHandle.Alloc(frame, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.SaveAsSheets(
                nativePath.Pointer,
                frameHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)frame.Length,
                printHeader ? (byte)1 : (byte)0,
                overwriteFile ? (byte)1 : (byte)0,
                out var rawHandle,
                out var data,
                out var length);
            if (result < 0)
                throw CreateNativeException(result);
            using var handle = new NativeBufferHandle(rawHandle);
            var byteLength = checked((int)length.ToUInt64());
            var resultFrame = new byte[byteLength];
            Marshal.Copy(data, resultFrame, 0, byteLength);
            var reader = new FrameReader(resultFrame);
            var count = reader.ReadLength();
            var rowCounts = new int[count];
            for (var index = 0; index < count; index++)
                rowCounts[index] = reader.ReadLength();
            reader.EnsureComplete();
            return rowCounts;
        }
        finally
        {
            frameHandle.Free();
        }
    }

    public static int[] SaveAsSheets(
        Stream stream,
        IEnumerable<KeyValuePair<string, IEnumerable<IDictionary<string, object?>>>> sheets,
        bool printHeader = true,
        bool leaveOpen = false)
    {
        ValidateWritableStream(stream);
        var temporaryPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.xlsx");
        try
        {
            var rowCounts = SaveAsSheets(temporaryPath, sheets, printHeader);
            CopyFileToStream(temporaryPath, stream);
            return rowCounts;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    public static int SaveAsWithSchema(
        string path,
        IReadOnlyList<string> schema,
        IEnumerable<IDictionary<string, object?>> rows,
        MiniExcelRustWriteOptions? options = null)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (schema is null)
            throw new ArgumentNullException(nameof(schema));
        if (schema.Count == 0 || schema.Any(string.IsNullOrWhiteSpace))
            throw new ArgumentException("The schema must contain at least one named column.", nameof(schema));
        if (schema.Distinct(StringComparer.Ordinal).Count() != schema.Count)
            throw new ArgumentException("Schema column names must be unique.", nameof(schema));
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        options ??= new MiniExcelRustWriteOptions();
        EnsureAbiVersion();
        var frame = EncodeRows(rows);
        var payload = MiniExcelBinaryPayload.EncodeXlsxWrite(schema, options);
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        var frameHandle = GCHandle.Alloc(frame, GCHandleType.Pinned);
        var payloadHandle = GCHandle.Alloc(payload, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.SaveAsConfigured(
                nativePath.Pointer,
                frameHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)frame.Length,
                payloadHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)payload.Length,
                out var rowCount);
            if (result < 0)
                throw CreateNativeException(result);
            return checked((int)rowCount);
        }
        finally
        {
            payloadHandle.Free();
            frameHandle.Free();
        }
    }

    public static int SaveAs<T>(
        string path,
        IEnumerable<T> rows,
        MiniExcelRustWriteOptions options)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        if (options is null)
            throw new ArgumentNullException(nameof(options));
        var dynamicColumns = options.DynamicColumns as IReadOnlyDictionary<string, MiniExcelRustDynamicColumn>;
        var dynamicRows = MiniExcelRustMapper.ToRows(rows, dynamicColumns).ToList();
        if (dynamicRows.Count == 0)
            throw new ArgumentException("Typed configured export requires at least one row.", nameof(rows));
        var schema = dynamicRows[0].Keys.ToList();
        return SaveAsWithSchema(path, schema, dynamicRows, options);
    }

    /// <summary>
    /// Creates a single-sheet XLSX workbook and copies it to a writable stream.
    /// </summary>
    public static int SaveAs(
        Stream stream,
        IEnumerable<IDictionary<string, object?>> rows,
        bool printHeader = true,
        string sheetName = "Sheet1",
        bool leaveOpen = false,
        IProgress<int>? progress = null)
    {
        if (stream is null)
            throw new ArgumentNullException(nameof(stream));
        if (!stream.CanWrite)
            throw new ArgumentException("The stream must be writable.", nameof(stream));

        var temporaryPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.xlsx");
        try
        {
            var rowCount = SaveAs(temporaryPath, rows, printHeader, sheetName, progress: progress);
            using var input = File.OpenRead(temporaryPath);
            input.CopyTo(stream);
            return rowCount;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    /// <summary>
    /// Creates a CSV file from dynamic rows.
    /// </summary>
    public static int SaveAsCsv(
        string path,
        IEnumerable<IDictionary<string, object?>> rows,
        MiniExcelRustCsvWriteOptions? configuration = null)
    {
        return WriteCsv(path, rows, configuration, append: false);
    }

    public static int SaveAsCsv<T>(
        string path,
        IEnumerable<T> rows,
        MiniExcelRustCsvWriteOptions? configuration = null)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        if (rows is IEnumerable<IDictionary<string, object?>> dynamicRows)
            return SaveAsCsv(path, dynamicRows, configuration);
        return SaveAsCsv(path, MiniExcelRustMapper.ToRows(rows), configuration);
    }

    public static int SaveAsCsv<T>(
        Stream stream,
        IEnumerable<T> rows,
        MiniExcelRustCsvWriteOptions? configuration = null,
        bool leaveOpen = false)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        if (rows is IEnumerable<IDictionary<string, object?>> dynamicRows)
            return SaveAsCsv(stream, dynamicRows, configuration, leaveOpen);
        return SaveAsCsv(stream, MiniExcelRustMapper.ToRows(rows), configuration, leaveOpen);
    }

    public static async Task<int> SaveAsCsvAsync<T>(
        string path,
        IAsyncEnumerable<T> rows,
        MiniExcelRustCsvWriteOptions? configuration = null,
        CancellationToken cancellationToken = default)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        cancellationToken.ThrowIfCancellationRequested();
        configuration ??= new MiniExcelRustCsvWriteOptions();
        var spoolPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-csv-spool-{Guid.NewGuid():N}.bin");
        try
        {
            List<string>? schema = null;
            using (var spool = new FileStream(
                spoolPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                81920,
                useAsync: true))
            {
                await foreach (var value in rows.WithCancellation(cancellationToken).ConfigureAwait(false))
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    var row = value is IDictionary<string, object?> dynamicRow
                        ? dynamicRow
                        : MiniExcelRustMapper.ToRows(new[] { value }).Single();
                    schema ??= row.Keys.ToList();
                    var frame = EncodeRows(new[] { row });
                    var length = BitConverter.GetBytes(checked((uint)frame.Length));
                    await spool.WriteAsync(length, 0, length.Length, cancellationToken).ConfigureAwait(false);
                    await spool.WriteAsync(frame, 0, frame.Length, cancellationToken).ConfigureAwait(false);
                }
            }
            if (schema is null)
                throw new InvalidOperationException("Async CSV export requires at least one row to infer its schema.");
            var payload = MiniExcelBinaryPayload.EncodeCsvWrite(schema, configuration);
            using var nativePath = new Utf8String(Path.GetFullPath(path));
            using var nativeSpoolPath = new Utf8String(spoolPath);
            using var nativeCancellation = NativeCancellationHandle.Create();
            using var registration = cancellationToken.Register(
                static state => NativeMethods.Cancel((NativeCancellationHandle)state!),
                nativeCancellation);
            var payloadHandle = GCHandle.Alloc(payload, GCHandleType.Pinned);
            try
            {
                var nativeResult = await Task.Run(() =>
                {
                    var result = NativeMethods.SaveCsvSpooledAsync(
                        nativePath.Pointer,
                        nativeSpoolPath.Pointer,
                        payloadHandle.AddrOfPinnedObject(),
                        (UIntPtr)(uint)payload.Length,
                        nativeCancellation,
                        out var rowCount);
                    return (Result: result, RowCount: rowCount);
                }).ConfigureAwait(false);
                if (nativeResult.Result < 0)
                {
                    if (cancellationToken.IsCancellationRequested)
                        throw new OperationCanceledException(cancellationToken);
                    throw CreateNativeException(nativeResult.Result);
                }
                return checked((int)nativeResult.RowCount);
            }
            finally
            {
                payloadHandle.Free();
            }
        }
        finally
        {
            DeleteTemporaryFile(spoolPath);
        }
    }

    /// <summary>
    /// Appends dynamic rows to a CSV file without repeating its header.
    /// </summary>
    public static int AppendCsv(
        string path,
        IEnumerable<IDictionary<string, object?>> rows,
        MiniExcelRustCsvWriteOptions? configuration = null)
    {
        return WriteCsv(path, rows, configuration, append: true);
    }

    public static int AppendCsv<T>(
        string path,
        IEnumerable<T> rows,
        MiniExcelRustCsvWriteOptions? configuration = null)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        return AppendCsv(path, MiniExcelRustMapper.ToRows(rows), configuration);
    }

    public static int AppendCsv<T>(
        Stream stream,
        IEnumerable<T> rows,
        MiniExcelRustCsvWriteOptions? configuration = null,
        bool leaveOpen = false)
    {
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        if (rows is IEnumerable<IDictionary<string, object?>> dynamicRows)
            return AppendCsv(stream, dynamicRows, configuration, leaveOpen);
        return AppendCsv(stream, MiniExcelRustMapper.ToRows(rows), configuration, leaveOpen);
    }

    public static int SaveAsCsv(
        Stream stream,
        IEnumerable<IDictionary<string, object?>> rows,
        MiniExcelRustCsvWriteOptions? configuration = null,
        bool leaveOpen = false)
    {
        ValidateWritableStream(stream);
        var temporaryPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.csv");
        try
        {
            configuration ??= new MiniExcelRustCsvWriteOptions();
            var rowCount = SaveAsCsv(
                temporaryPath,
                rows,
                new MiniExcelRustCsvWriteOptions
                {
                    Delimiter = configuration.Delimiter,
                    Encoding = configuration.Encoding,
                    WriteBom = configuration.WriteBom,
                    PrintHeader = configuration.PrintHeader,
                    OverwriteFile = false
                });
            CopyFileToStream(temporaryPath, stream);
            return rowCount;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    public static int AppendCsv(
        Stream stream,
        IEnumerable<IDictionary<string, object?>> rows,
        MiniExcelRustCsvWriteOptions? configuration = null,
        bool leaveOpen = false)
    {
        ValidateReadableStream(stream);
        ValidateWritableStream(stream);
        if (!stream.CanSeek)
            throw new ArgumentException("The stream must be seekable for CSV append.", nameof(stream));

        var temporaryPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.csv");
        try
        {
            stream.Position = 0;
            using (var output = File.Create(temporaryPath))
                stream.CopyTo(output);
            var rowCount = AppendCsv(temporaryPath, rows, configuration);
            CopyFileToStream(temporaryPath, stream);
            return rowCount;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    public static void ConvertCsvToXlsx(
        string csvPath,
        string xlsxPath,
        bool csvHasHeader = false)
    {
        var rows = QueryCsv(csvPath, csvHasHeader);
        SaveAs(xlsxPath, rows, csvHasHeader);
    }

    public static void ConvertCsvToXlsx(
        Stream csvStream,
        Stream xlsxStream,
        bool csvHasHeader = false)
    {
        var rows = QueryCsv(csvStream, csvHasHeader, leaveOpen: true);
        SaveAs(xlsxStream, rows, csvHasHeader, leaveOpen: true);
    }

    public static Task ConvertCsvToXlsxAsync(
        string csvPath,
        string xlsxPath,
        bool csvHasHeader = false,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() => ConvertCsvToXlsx(csvPath, xlsxPath, csvHasHeader), cancellationToken);
    }

    public static void ConvertXlsxToCsv(
        string xlsxPath,
        string csvPath,
        bool xlsxHasHeader = true)
    {
        var rows = Query(xlsxPath, xlsxHasHeader);
        SaveAsCsv(
            csvPath,
            rows,
            new MiniExcelRustCsvWriteOptions { PrintHeader = xlsxHasHeader });
    }

    public static void ConvertXlsxToCsv(
        Stream xlsxStream,
        Stream csvStream,
        bool xlsxHasHeader = true)
    {
        var rows = Query(xlsxStream, xlsxHasHeader, leaveOpen: true);
        SaveAsCsv(
            csvStream,
            rows,
            new MiniExcelRustCsvWriteOptions { PrintHeader = xlsxHasHeader },
            leaveOpen: true);
    }

    public static Task ConvertXlsxToCsvAsync(
        string xlsxPath,
        string csvPath,
        bool xlsxHasHeader = true,
        CancellationToken cancellationToken = default)
    {
        return Task.Run(() => ConvertXlsxToCsv(xlsxPath, csvPath, xlsxHasHeader), cancellationToken);
    }

    public static void RenameSheet(string path, string sheetName, string newSheetName)
    {
        ValidatePathAndSheet(path, sheetName);
        if (string.IsNullOrWhiteSpace(newSheetName))
            throw new ArgumentException("The new sheet name is required.", nameof(newSheetName));
        EnsureAbiVersion();
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        using var nativeNewSheetName = new Utf8String(newSheetName);
        var result = NativeMethods.RenameSheet(nativePath.Pointer, nativeSheetName.Pointer, nativeNewSheetName.Pointer);
        if (result < 0)
            throw CreateNativeException(result);
    }

    public static void ReorderSheet(string path, string sheetName, int newSheetIndex)
    {
        ValidatePathAndSheet(path, sheetName);
        EnsureAbiVersion();
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        var result = NativeMethods.ReorderSheet(nativePath.Pointer, nativeSheetName.Pointer, newSheetIndex);
        if (result < 0)
            throw CreateNativeException(result);
    }

    public static void SetSheetVisibility(
        string path,
        string sheetName,
        MiniExcelRustSheetState visibility)
    {
        ValidatePathAndSheet(path, sheetName);
        if (visibility is < MiniExcelRustSheetState.Visible or > MiniExcelRustSheetState.VeryHidden)
            throw new ArgumentOutOfRangeException(nameof(visibility));
        EnsureAbiVersion();
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        var result = NativeMethods.SetSheetVisibility(nativePath.Pointer, nativeSheetName.Pointer, (byte)visibility);
        if (result < 0)
            throw CreateNativeException(result);
    }

    public static int InsertSheet(
        string path,
        IEnumerable<IDictionary<string, object?>> rows,
        string sheetName,
        MiniExcelRustInsertOptions? options = null)
    {
        ValidatePathAndSheet(path, sheetName);
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        options ??= new MiniExcelRustInsertOptions();
        EnsureAbiVersion();

        var frame = EncodeRows(rows);
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        using var nativeSheetName = new Utf8String(sheetName);
        var frameHandle = GCHandle.Alloc(frame, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.InsertSheet(
                nativePath.Pointer,
                frameHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)frame.Length,
                nativeSheetName.Pointer,
                options.PrintHeader ? (byte)1 : (byte)0,
                options.ReplaceExistingSheet ? (byte)1 : (byte)0,
                options.RemoveSupportedRelationships ? (byte)1 : (byte)0,
                out var rowCount);
            if (result < 0)
                throw CreateNativeException(result);
            return checked((int)rowCount);
        }
        finally
        {
            frameHandle.Free();
        }
    }

    public static int InsertSheet(
        Stream stream,
        IEnumerable<IDictionary<string, object?>> rows,
        string sheetName,
        MiniExcelRustInsertOptions? options = null,
        bool leaveOpen = false)
    {
        ValidateReadableStream(stream);
        ValidateWritableStream(stream);
        if (!stream.CanSeek)
            throw new ArgumentException("The stream must be seekable for worksheet insertion.", nameof(stream));
        stream.Position = 0;
        var temporaryPath = StageStream(stream);
        try
        {
            var count = InsertSheet(temporaryPath, rows, sheetName, options);
            CopyFileToStream(temporaryPath, stream);
            return count;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    public static int CopyAndAddSheet(
        string sourcePath,
        string destinationPath,
        IEnumerable<IDictionary<string, object?>> rows,
        string sheetName,
        MiniExcelRustInsertOptions? options = null)
    {
        ValidatePathAndSheet(sourcePath, sheetName);
        if (string.IsNullOrWhiteSpace(destinationPath))
            throw new ArgumentException("The destination path is required.", nameof(destinationPath));
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        options ??= new MiniExcelRustInsertOptions();
        EnsureAbiVersion();

        var frame = EncodeRows(rows);
        using var nativeSourcePath = new Utf8String(Path.GetFullPath(sourcePath));
        using var nativeDestinationPath = new Utf8String(Path.GetFullPath(destinationPath));
        using var nativeSheetName = new Utf8String(sheetName);
        var frameHandle = GCHandle.Alloc(frame, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.CopyAndAddSheet(
                nativeSourcePath.Pointer,
                nativeDestinationPath.Pointer,
                frameHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)frame.Length,
                nativeSheetName.Pointer,
                options.PrintHeader ? (byte)1 : (byte)0,
                options.ReplaceExistingSheet ? (byte)1 : (byte)0,
                options.RemoveSupportedRelationships ? (byte)1 : (byte)0,
                options.OverwriteDestination ? (byte)1 : (byte)0,
                out var rowCount);
            if (result < 0)
                throw CreateNativeException(result);
            return checked((int)rowCount);
        }
        finally
        {
            frameHandle.Free();
        }
    }

    public static void FillTemplate(
        string destinationPath,
        string templatePath,
        object value,
        bool overwriteFile = false,
        bool ignoreMissingVariables = true)
    {
        if (string.IsNullOrWhiteSpace(destinationPath))
            throw new ArgumentException("The destination path is required.", nameof(destinationPath));
        if (string.IsNullOrWhiteSpace(templatePath))
            throw new ArgumentException("The template path is required.", nameof(templatePath));
        if (value is null)
            throw new ArgumentNullException(nameof(value));

        EnsureAbiVersion();
        var payload = MiniExcelBinaryPayload.EncodeTemplateValue(value);
        using var nativeDestinationPath = new Utf8String(Path.GetFullPath(destinationPath));
        using var nativeTemplatePath = new Utf8String(Path.GetFullPath(templatePath));
        var payloadHandle = GCHandle.Alloc(payload, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.FillTemplate(
                nativeDestinationPath.Pointer,
                nativeTemplatePath.Pointer,
            payloadHandle.AddrOfPinnedObject(),
            (UIntPtr)(uint)payload.Length,
                overwriteFile ? (byte)1 : (byte)0,
                ignoreMissingVariables ? (byte)1 : (byte)0);
            if (result < 0)
                throw CreateNativeException(result);
        }
        finally
        {
            payloadHandle.Free();
        }
    }

    internal static void FillMappedTemplateCore(
        string destinationPath,
        string templatePath,
        byte[] payload,
        bool overwriteFile)
    {
        if (string.IsNullOrWhiteSpace(destinationPath))
            throw new ArgumentException("The destination path is required.", nameof(destinationPath));
        if (string.IsNullOrWhiteSpace(templatePath))
            throw new ArgumentException("The template path is required.", nameof(templatePath));
        if (payload is null)
            throw new ArgumentNullException(nameof(payload));

        EnsureAbiVersion();
        using var nativeDestinationPath = new Utf8String(Path.GetFullPath(destinationPath));
        using var nativeTemplatePath = new Utf8String(Path.GetFullPath(templatePath));
        var payloadHandle = GCHandle.Alloc(payload, GCHandleType.Pinned);
        try
        {
            var result = NativeMethods.FillMappedTemplate(
                nativeDestinationPath.Pointer,
                nativeTemplatePath.Pointer,
                payloadHandle.AddrOfPinnedObject(),
                (UIntPtr)(uint)payload.Length,
                overwriteFile ? (byte)1 : (byte)0);
            if (result < 0)
                throw CreateNativeException(result);
        }
        finally
        {
            payloadHandle.Free();
        }
    }

    public static void FillTemplate(
        string destinationPath,
        Stream templateStream,
        object value,
        bool overwriteFile = false,
        bool ignoreMissingVariables = true,
        bool leaveTemplateOpen = false)
    {
        _ = UseStagedStream(templateStream, leaveTemplateOpen, templatePath =>
        {
            FillTemplate(destinationPath, templatePath, value, overwriteFile, ignoreMissingVariables);
            return 0;
        });
    }

    public static void FillTemplate(
        string destinationPath,
        byte[] templateBytes,
        object value,
        bool overwriteFile = false,
        bool ignoreMissingVariables = true)
    {
        if (templateBytes is null)
            throw new ArgumentNullException(nameof(templateBytes));
        using var templateStream = new MemoryStream(templateBytes, writable: false);
        FillTemplate(
            destinationPath,
            templateStream,
            value,
            overwriteFile,
            ignoreMissingVariables,
            leaveTemplateOpen: false);
    }

    public static void FillTemplate(
        Stream destinationStream,
        string templatePath,
        object value,
        bool ignoreMissingVariables = true,
        bool leaveOpen = false)
    {
        ValidateWritableStream(destinationStream);
        var temporaryPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.xlsx");
        try
        {
            FillTemplate(temporaryPath, templatePath, value, false, ignoreMissingVariables);
            CopyFileToStream(temporaryPath, destinationStream);
        }
        finally
        {
            if (!leaveOpen)
                destinationStream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    public static void FillTemplate(
        Stream destinationStream,
        Stream templateStream,
        object value,
        bool ignoreMissingVariables = true,
        bool leaveOpen = false,
        bool leaveTemplateOpen = false)
    {
        _ = UseStagedStream(templateStream, leaveTemplateOpen, templatePath =>
        {
            FillTemplate(destinationStream, templatePath, value, ignoreMissingVariables, leaveOpen);
            return 0;
        });
    }

    public static void FillTemplate(
        Stream destinationStream,
        byte[] templateBytes,
        object value,
        bool ignoreMissingVariables = true,
        bool leaveOpen = false)
    {
        if (templateBytes is null)
            throw new ArgumentNullException(nameof(templateBytes));
        using var templateStream = new MemoryStream(templateBytes, writable: false);
        FillTemplate(
            destinationStream,
            templateStream,
            value,
            ignoreMissingVariables,
            leaveOpen,
            leaveTemplateOpen: false);
    }

    public static void MergeSameCells(
        string destinationPath,
        string sourcePath,
        bool overwriteFile = false)
    {
        if (string.IsNullOrWhiteSpace(destinationPath))
            throw new ArgumentException("The destination path is required.", nameof(destinationPath));
        if (string.IsNullOrWhiteSpace(sourcePath))
            throw new ArgumentException("The source path is required.", nameof(sourcePath));

        EnsureAbiVersion();
        using var nativeDestinationPath = new Utf8String(Path.GetFullPath(destinationPath));
        using var nativeSourcePath = new Utf8String(Path.GetFullPath(sourcePath));
        var result = NativeMethods.MergeSameCells(
            nativeDestinationPath.Pointer,
            nativeSourcePath.Pointer,
            overwriteFile ? (byte)1 : (byte)0);
        if (result < 0)
            throw CreateNativeException(result);
    }

    public static void MergeSameCells(
        string destinationPath,
        Stream sourceStream,
        bool overwriteFile = false,
        bool leaveSourceOpen = false)
    {
        _ = UseStagedStream(sourceStream, leaveSourceOpen, sourcePath =>
        {
            MergeSameCells(destinationPath, sourcePath, overwriteFile);
            return 0;
        });
    }

    public static void MergeSameCells(
        Stream destinationStream,
        string sourcePath,
        bool leaveOpen = false)
    {
        ValidateWritableStream(destinationStream);
        var temporaryPath = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.xlsx");
        try
        {
            MergeSameCells(temporaryPath, sourcePath);
            CopyFileToStream(temporaryPath, destinationStream);
        }
        finally
        {
            if (!leaveOpen)
                destinationStream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    public static void MergeSameCells(
        Stream destinationStream,
        byte[] sourceBytes,
        bool leaveOpen = false)
    {
        if (sourceBytes is null)
            throw new ArgumentNullException(nameof(sourceBytes));
        using var sourceStream = new MemoryStream(sourceBytes, writable: false);
        _ = UseStagedStream(sourceStream, false, sourcePath =>
        {
            MergeSameCells(destinationStream, sourcePath, leaveOpen);
            return 0;
        });
    }

    public static void MergeSameCells(
        Stream destinationStream,
        Stream sourceStream,
        bool leaveOpen = false,
        bool leaveSourceOpen = false)
    {
        _ = UseStagedStream(sourceStream, leaveSourceOpen, sourcePath =>
        {
            MergeSameCells(destinationStream, sourcePath, leaveOpen);
            return 0;
        });
    }

    public static void AddPicture(string path, params MiniExcelRustPicture[] pictures)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (pictures is null || pictures.Length == 0)
            throw new ArgumentException("At least one picture is required.", nameof(pictures));
        EnsureAbiVersion();
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        foreach (var picture in pictures)
        {
            if (picture.ImageBytes is null || picture.ImageBytes.Length == 0)
                throw new ArgumentException("Picture data is required.", nameof(pictures));
            if (picture.WidthPx <= 0 || picture.HeightPx <= 0)
                throw new ArgumentOutOfRangeException(nameof(pictures), "Picture dimensions must be positive.");
            using var nativeSheetName = new Utf8String(picture.SheetName);
            using var nativeCellAddress = new Utf8String(picture.CellAddress);
            var imageHandle = GCHandle.Alloc(picture.ImageBytes, GCHandleType.Pinned);
            try
            {
                var result = NativeMethods.AddPicture(
                    nativePath.Pointer,
                    nativeSheetName.Pointer,
                    nativeCellAddress.Pointer,
                    imageHandle.AddrOfPinnedObject(),
                    (UIntPtr)(uint)picture.ImageBytes.Length,
                    checked((uint)picture.WidthPx),
                    checked((uint)picture.HeightPx),
                    (byte)picture.Anchor,
                    picture.LocationX,
                    picture.LocationY);
                if (result < 0)
                    throw CreateNativeException(result);
            }
            finally
            {
                imageHandle.Free();
            }
        }
    }

    public static void AddPicture(
        Stream stream,
        bool leaveOpen = false,
        params MiniExcelRustPicture[] pictures)
    {
        ValidateReadableStream(stream);
        ValidateWritableStream(stream);
        if (!stream.CanSeek)
            throw new ArgumentException("The stream must be seekable for picture insertion.", nameof(stream));
        stream.Position = 0;
        var temporaryPath = StageStream(stream);
        try
        {
            AddPicture(temporaryPath, pictures);
            CopyFileToStream(temporaryPath, stream);
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    private static IEnumerable<IDictionary<string, object?>> QueryStreamIterator(
        Stream stream,
        bool useHeaderRow,
        string? sheetName,
        string startCell,
        string? endCell,
        MiniExcelRustReadOptions? configuration,
        bool leaveOpen)
    {
        string? temporaryPath = null;
        try
        {
            temporaryPath = StageStream(stream);
            foreach (var row in QueryIterator(temporaryPath, useHeaderRow, sheetName, startCell, endCell, configuration))
                yield return row;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    private static IEnumerable<IDictionary<string, object?>> QueryTableStreamIterator(
        Stream stream,
        string? sheetName,
        string tableName,
        bool leaveOpen)
    {
        string? temporaryPath = null;
        try
        {
            temporaryPath = StageStream(stream);
            foreach (var row in QueryTableIterator(temporaryPath, sheetName, tableName))
                yield return row;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    private static IEnumerable<IDictionary<string, object?>> QueryTableIterator(
        string path,
        string? sheetName,
        string tableName)
    {
        EnsureAbiVersion();

        using var nativePath = new Utf8String(path);
        using var nativeSheetName = new Utf8String(sheetName);
        using var nativeTableName = new Utf8String(tableName);
        var result = NativeMethods.QueryTableOpen(
            nativePath.Pointer,
            nativeSheetName.Pointer,
            nativeTableName.Pointer,
            out var rawHandle);
        if (result < 0)
            throw CreateNativeException(result);

        foreach (var row in ReadRows(rawHandle))
            yield return row;
    }

    private static IEnumerable<IDictionary<string, object?>> QueryCsvStreamIterator(
        Stream stream,
        bool useHeaderRow,
        MiniExcelRustCsvReadOptions? configuration,
        bool leaveOpen)
    {
        string? temporaryPath = null;
        try
        {
            temporaryPath = StageStream(stream);
            foreach (var row in QueryCsvIterator(temporaryPath, useHeaderRow, configuration))
                yield return row;
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    private static IEnumerable<IDictionary<string, object?>> QueryCsvIterator(
        string path,
        bool useHeaderRow,
        MiniExcelRustCsvReadOptions? configuration)
    {
        EnsureAbiVersion();
        configuration ??= new MiniExcelRustCsvReadOptions();

        using var nativePath = new Utf8String(path);
        var result = NativeMethods.QueryCsvOpen(
            nativePath.Pointer,
            useHeaderRow ? (byte)1 : (byte)0,
            (byte)configuration.Delimiter,
            (byte)configuration.Encoding,
            configuration.ReadEmptyStringAsNull ? (byte)1 : (byte)0,
            configuration.TrimColumnNames ? (byte)1 : (byte)0,
            out var rawHandle);
        if (result < 0)
            throw CreateNativeException(result);

        foreach (var row in ReadRows(rawHandle))
            yield return row;
    }

    private static IEnumerable<IDictionary<string, object?>> QueryIterator(
        string path,
        bool useHeaderRow,
        string? sheetName,
        string startCell,
        string? endCell,
        MiniExcelRustReadOptions? configuration)
    {
        EnsureAbiVersion();

        using var nativePath = new Utf8String(path);
        using var nativeSheetName = new Utf8String(sheetName);
        using var nativeStartCell = new Utf8String(startCell);
        using var nativeEndCell = new Utf8String(endCell);
        using var nativeCachePath = new Utf8String(configuration?.SharedStringCachePath);
        int result;
        IntPtr rawHandle;
        if (configuration is not null)
        {
            result = NativeMethods.QueryOptionsOpen(
                nativePath.Pointer,
                useHeaderRow ? (byte)1 : (byte)0,
                nativeSheetName.Pointer,
                nativeStartCell.Pointer,
                nativeEndCell.Pointer,
                configuration.IgnoreEmptyRows ? (byte)1 : (byte)0,
                configuration.FillMergedCells ? (byte)1 : (byte)0,
                configuration.TrimColumnNames ? (byte)1 : (byte)0,
                configuration.EnableSharedStringCache ? (byte)1 : (byte)0,
                configuration.SharedStringCacheSize,
                nativeCachePath.Pointer,
                out rawHandle);
        }
        else if (endCell is not null)
        {
            result = NativeMethods.QueryRangeOpen(
                nativePath.Pointer,
                useHeaderRow ? (byte)1 : (byte)0,
                nativeSheetName.Pointer,
                nativeStartCell.Pointer,
                nativeEndCell.Pointer,
                out rawHandle);
        }
        else
        {
            result = NativeMethods.QueryOpen(
                nativePath.Pointer,
                useHeaderRow ? (byte)1 : (byte)0,
                nativeSheetName.Pointer,
                nativeStartCell.Pointer,
                out rawHandle);
        }
        if (result < 0)
            throw CreateNativeException(result);

        foreach (var row in ReadRows(rawHandle))
            yield return row;
    }

    private static IEnumerable<IDictionary<string, object?>> ReadRows(IntPtr rawHandle)
    {
        using var handle = new NativeQueryHandle(rawHandle);
        while (true)
        {
            var result = NativeMethods.QueryNextBatch(handle, BatchSize, out var data, out var length);
            if (result == 0)
                yield break;
            if (result < 0)
                throw CreateNativeException(result);

            var byteLength = checked((int)length.ToUInt64());
            var frame = new byte[byteLength];
            Marshal.Copy(data, frame, 0, byteLength);
            foreach (var row in DecodeBatch(frame))
                yield return row;
        }
    }

    private static IEnumerable<IDictionary<string, object?>> DecodeBatch(byte[] frame)
    {
        var reader = new FrameReader(frame);
        var rowCount = reader.ReadLength();
        for (var rowIndex = 0; rowIndex < rowCount; rowIndex++)
        {
            var cellCount = reader.ReadLength();
            IDictionary<string, object?> row = new Dictionary<string, object?>(cellCount, StringComparer.Ordinal);
            for (var cellIndex = 0; cellIndex < cellCount; cellIndex++)
                row.Add(reader.ReadString(), reader.ReadValue());
            yield return row;
        }

        reader.EnsureComplete();
    }

    private static List<string> DecodeStrings(byte[] frame)
    {
        var reader = new FrameReader(frame);
        var count = reader.ReadLength();
        var values = new List<string>(count);
        for (var index = 0; index < count; index++)
            values.Add(reader.ReadString());
        reader.EnsureComplete();
        return values;
    }

    private static List<MiniExcelRustRange> DecodeRanges(byte[] frame)
    {
        var reader = new FrameReader(frame);
        var count = reader.ReadLength();
        var ranges = new List<MiniExcelRustRange>(count);
        for (var index = 0; index < count; index++)
        {
            var startCell = reader.ReadString();
            var endCell = reader.ReadString();
            ranges.Add(new MiniExcelRustRange(
                startCell.Length == 0 ? null : startCell,
                endCell.Length == 0 ? null : endCell));
        }
        reader.EnsureComplete();
        return ranges;
    }

    private static List<MiniExcelRustSheetInfo> DecodeSheetInfo(byte[] frame)
    {
        var reader = new FrameReader(frame);
        var count = reader.ReadLength();
        var sheets = new List<MiniExcelRustSheetInfo>(count);
        for (var index = 0; index < count; index++)
        {
            sheets.Add(new MiniExcelRustSheetInfo(
                reader.ReadUInt32(),
                reader.ReadUInt32(),
                reader.ReadString(),
                (MiniExcelRustSheetType)reader.ReadByte(),
                (MiniExcelRustSheetState)reader.ReadByte(),
                reader.ReadByte() != 0));
        }
        reader.EnsureComplete();
        return sheets;
    }

    private static MiniExcelRustCommentResult DecodeComments(byte[] frame)
    {
        var reader = new FrameReader(frame);
        var sheetName = reader.ReadString();
        var commentCount = reader.ReadLength();
        var comments = new List<MiniExcelRustThreadedComment>(commentCount);
        for (var index = 0; index < commentCount; index++)
        {
            var id = Guid.Parse(reader.ReadString());
            var referenceCell = reader.ReadString();
            var author = ReadCommentAuthor(reader);
            var createdAt = ReadCommentTimestamp(reader);
            var resolved = reader.ReadByte() != 0;
            var text = reader.ReadString();
            var replyCount = reader.ReadLength();
            var replies = new List<MiniExcelRustThreadedCommentReply>(replyCount);
            for (var replyIndex = 0; replyIndex < replyCount; replyIndex++)
            {
                replies.Add(new MiniExcelRustThreadedCommentReply(
                    Guid.Parse(reader.ReadString()),
                    Guid.Parse(reader.ReadString()),
                    ReadCommentAuthor(reader),
                    ReadCommentTimestamp(reader),
                    reader.ReadString()));
            }
            comments.Add(new MiniExcelRustThreadedComment(
                id,
                referenceCell,
                author,
                createdAt,
                resolved,
                text,
                replies));
        }

        var noteCount = reader.ReadLength();
        var notes = new List<MiniExcelRustNoteComment>(noteCount);
        for (var index = 0; index < noteCount; index++)
        {
            var id = reader.ReadOptionalString();
            notes.Add(new MiniExcelRustNoteComment(
                id is null ? null : Guid.Parse(id),
                reader.ReadString(),
                reader.ReadOptionalString() ?? string.Empty,
                reader.ReadString()));
        }
        reader.EnsureComplete();
        return new MiniExcelRustCommentResult(sheetName, comments, notes);
    }

    private static MiniExcelRustCommentAuthor? ReadCommentAuthor(FrameReader reader)
    {
        if (reader.ReadByte() == 0)
            return null;
        return new MiniExcelRustCommentAuthor(
            Guid.Parse(reader.ReadString()),
            reader.ReadString(),
            reader.ReadOptionalString());
    }

    private static DateTime? ReadCommentTimestamp(FrameReader reader)
    {
        var value = reader.ReadOptionalString();
        return value is null
            ? null
            : DateTime.Parse(value, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind);
    }

    private static DataTable CreateDataTable(
        IReadOnlyList<string> columns,
        IEnumerable<IDictionary<string, object?>> rows)
    {
        var table = new DataTable();
        foreach (var column in columns)
            table.Columns.Add(column, typeof(object));

        foreach (var row in rows)
        {
            var values = new object?[columns.Count];
            for (var index = 0; index < columns.Count; index++)
                values[index] = row.TryGetValue(columns[index], out var value) ? value ?? DBNull.Value : DBNull.Value;
            table.Rows.Add(values);
        }

        return table;
    }

    private static async IAsyncEnumerable<T> ToAsyncEnumerable<T>(
        IEnumerable<T> values,
        [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        foreach (var value in values)
        {
            cancellationToken.ThrowIfCancellationRequested();
            yield return value;
            await Task.Yield();
        }
    }

    private static byte[] EncodeRows(IEnumerable<IDictionary<string, object?>> rows)
    {
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
        WriteRows(writer, rows);
        writer.Flush();
        return stream.ToArray();
    }

    private static void ReportProgress(
        IProgress<int>? progress,
        IEnumerable<IDictionary<string, object?>> rows)
    {
        if (progress is null)
            return;
        foreach (var row in rows)
        {
            foreach (var _ in row)
                progress.Report(1);
        }
    }

    private static byte[] EncodeMapping(IReadOnlyDictionary<string, string> mapping)
    {
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
        writer.Write(checked((uint)mapping.Count));
        foreach (var cell in mapping)
        {
            if (string.IsNullOrWhiteSpace(cell.Key) || string.IsNullOrWhiteSpace(cell.Value))
                throw new ArgumentException("Mapping field names and cell addresses are required.", nameof(mapping));
            WriteFrameString(writer, cell.Key);
            WriteFrameString(writer, cell.Value);
        }
        writer.Flush();
        return stream.ToArray();
    }

    private static byte[] EncodeSheets(
        IEnumerable<KeyValuePair<string, IEnumerable<IDictionary<string, object?>>>> sheets)
    {
        var materializedSheets = sheets.ToList();
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
        writer.Write(checked((uint)materializedSheets.Count));
        foreach (var sheet in materializedSheets)
        {
            if (string.IsNullOrWhiteSpace(sheet.Key))
                throw new ArgumentException("Every sheet must have a name.", nameof(sheets));
            WriteFrameString(writer, sheet.Key);
            WriteRows(writer, sheet.Value);
        }
        writer.Flush();
        return stream.ToArray();
    }

    private static void WriteRows(
        BinaryWriter writer,
        IEnumerable<IDictionary<string, object?>> rows)
    {
        var materializedRows = rows.ToList();
        writer.Write(checked((uint)materializedRows.Count));
        foreach (var row in materializedRows)
        {
            writer.Write(checked((uint)row.Count));
            foreach (var cell in row)
            {
                WriteFrameString(writer, cell.Key);
                WriteFrameValue(writer, cell.Value);
            }
        }
    }

    private static int WriteCsv(
        string path,
        IEnumerable<IDictionary<string, object?>> rows,
        MiniExcelRustCsvWriteOptions? configuration,
        bool append)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (rows is null)
            throw new ArgumentNullException(nameof(rows));
        configuration ??= new MiniExcelRustCsvWriteOptions();
        if (configuration.Delimiter == '\0' || configuration.Delimiter > 0x7f)
            throw new ArgumentException("The CSV delimiter must be a single-byte ASCII character.", nameof(configuration));

        EnsureAbiVersion();
        var frame = EncodeRows(rows);
        using var nativePath = new Utf8String(Path.GetFullPath(path));
        var frameHandle = GCHandle.Alloc(frame, GCHandleType.Pinned);
        try
        {
            var result = append
                ? NativeMethods.AppendCsv(
                    nativePath.Pointer,
                    frameHandle.AddrOfPinnedObject(),
                    (UIntPtr)(uint)frame.Length,
                    (byte)configuration.Delimiter,
                    (byte)configuration.Encoding,
                    configuration.WriteBom ? (byte)1 : (byte)0,
                    configuration.PrintHeader ? (byte)1 : (byte)0,
                    out var rowCount)
                : NativeMethods.SaveCsv(
                    nativePath.Pointer,
                    frameHandle.AddrOfPinnedObject(),
                    (UIntPtr)(uint)frame.Length,
                    (byte)configuration.Delimiter,
                    (byte)configuration.Encoding,
                    configuration.WriteBom ? (byte)1 : (byte)0,
                    configuration.PrintHeader ? (byte)1 : (byte)0,
                    configuration.OverwriteFile ? (byte)1 : (byte)0,
                    out rowCount);
            if (result < 0)
                throw CreateNativeException(result);
            return checked((int)rowCount);
        }
        finally
        {
            frameHandle.Free();
        }
    }

    private static void WriteFrameString(BinaryWriter writer, string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        writer.Write(checked((uint)bytes.Length));
        writer.Write(bytes);
    }

    private static void WriteFrameValue(BinaryWriter writer, object? value)
    {
        switch (value)
        {
            case null:
            case DBNull:
                writer.Write((byte)0);
                break;
            case bool boolean:
                writer.Write((byte)1);
                writer.Write((byte)(boolean ? 1 : 0));
                break;
            case byte or sbyte or short or ushort or int or uint or long:
                writer.Write((byte)2);
                writer.Write(Convert.ToInt64(value, CultureInfo.InvariantCulture));
                break;
            case ulong unsigned when unsigned <= long.MaxValue:
                writer.Write((byte)2);
                writer.Write((long)unsigned);
                break;
            case float or double or decimal:
                writer.Write((byte)3);
                writer.Write(Convert.ToDouble(value, CultureInfo.InvariantCulture));
                break;
            case string text:
                writer.Write((byte)4);
                WriteFrameString(writer, text);
                break;
#if NET8_0_OR_GREATER
            case DateOnly date:
                writer.Write((byte)5);
                WriteFrameString(writer, date.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture));
                break;
            case TimeOnly time:
                writer.Write((byte)6);
                WriteFrameString(writer, time.ToString("HH:mm:ss.fffffff", CultureInfo.InvariantCulture));
                break;
#endif
            case DateTime dateTime:
                writer.Write((byte)7);
                WriteFrameString(writer, dateTime.ToString("yyyy-MM-dd'T'HH:mm:ss.fffffff", CultureInfo.InvariantCulture));
                break;
            case TimeSpan duration:
                writer.Write((byte)8);
                writer.Write(checked((long)duration.TotalMilliseconds));
                break;
            default:
                throw new NotSupportedException($"Values of type {value.GetType().FullName} are not supported by SaveAs yet.");
        }
    }

    private static TResult UseStagedStream<TResult>(
        Stream stream,
        bool leaveOpen,
        Func<string, TResult> operation)
    {
        ValidateReadableStream(stream);
        string? temporaryPath = null;
        try
        {
            temporaryPath = StageStream(stream);
            return operation(temporaryPath);
        }
        finally
        {
            if (!leaveOpen)
                stream.Dispose();
            DeleteTemporaryFile(temporaryPath);
        }
    }

    private static string StageStream(Stream stream)
    {
        var path = Path.Combine(Path.GetTempPath(), $"miniexcel-rust-{Guid.NewGuid():N}.xlsx");
        try
        {
            using var output = File.Create(path);
            stream.CopyTo(output);
            return path;
        }
        catch
        {
            DeleteTemporaryFile(path);
            throw;
        }
    }

    private static void ValidateReadableStream(Stream stream)
    {
        if (stream is null)
            throw new ArgumentNullException(nameof(stream));
        if (!stream.CanRead)
            throw new ArgumentException("The stream must be readable.", nameof(stream));
    }

    private static void ValidateWritableStream(Stream stream)
    {
        if (stream is null)
            throw new ArgumentNullException(nameof(stream));
        if (!stream.CanWrite)
            throw new ArgumentException("The stream must be writable.", nameof(stream));
    }

    private static void CopyFileToStream(string path, Stream destination)
    {
        if (destination.CanSeek)
        {
            destination.Position = 0;
            destination.SetLength(0);
        }
        using var input = File.OpenRead(path);
        input.CopyTo(destination);
    }

    private static void ValidateCsvConfiguration(MiniExcelRustCsvReadOptions? configuration)
    {
        if (configuration is not null && (configuration.Delimiter == '\0' || configuration.Delimiter > 0x7f))
            throw new ArgumentException("The CSV delimiter must be a single-byte ASCII character.", nameof(configuration));
    }

    private static void ValidatePathAndSheet(string path, string sheetName)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("The path is required.", nameof(path));
        if (string.IsNullOrWhiteSpace(sheetName))
            throw new ArgumentException("The sheet name is required.", nameof(sheetName));
    }

    private static string ToCellReference(int row, int column)
    {
        if (row is < 1 or > 1_048_576)
            throw new ArgumentOutOfRangeException(nameof(row));
        if (column is < 1 or > 16_384)
            throw new ArgumentOutOfRangeException(nameof(column));
        var letters = string.Empty;
        while (column > 0)
        {
            column--;
            letters = (char)('A' + column % 26) + letters;
            column /= 26;
        }
        return letters + row.ToString(CultureInfo.InvariantCulture);
    }

    private static void DeleteTemporaryFile(string? path)
    {
        if (path is not null && File.Exists(path))
            File.Delete(path);
    }

    private static void EnsureAbiVersion()
    {
        var version = NativeMethods.GetAbiVersion();
        if (version != 2)
            throw new NotSupportedException($"MiniExcel Rust ABI version {version} is not supported.");
    }

    private static Exception CreateNativeException(int result)
    {
        var data = NativeMethods.GetLastError(out var length);
        var byteLength = checked((int)length.ToUInt64());
        if (data == IntPtr.Zero || byteLength == 0)
            return new InvalidOperationException($"MiniExcel Rust query failed with native error {result}.");

        var bytes = new byte[byteLength];
        Marshal.Copy(data, bytes, 0, byteLength);
        return new InvalidOperationException(Encoding.UTF8.GetString(bytes));
    }

    private sealed class FrameReader(byte[] frame)
    {
        private int _offset;

        public int ReadLength()
        {
            var value = ReadUInt32();
            if (value > int.MaxValue)
                throw new InvalidDataException("The native MiniExcel frame contains an unsupported length.");
            return (int)value;
        }

        public string ReadString()
        {
            var length = ReadLength();
            EnsureAvailable(length);
            var value = Encoding.UTF8.GetString(frame, _offset, length);
            _offset += length;
            return value;
        }

        public string? ReadOptionalString()
        {
            return ReadByte() == 0 ? null : ReadString();
        }

        public object? ReadValue()
        {
            EnsureAvailable(1);
            return frame[_offset++] switch
            {
                0 => null,
                1 => ReadBoolean(),
                2 => Convert.ToDouble(ReadInt64(), CultureInfo.InvariantCulture),
                3 => BitConverter.Int64BitsToDouble(ReadInt64()),
                4 => ReadString(),
                5 => DateTime.ParseExact(ReadString(), "yyyy-MM-dd", CultureInfo.InvariantCulture),
                6 => TimeSpan.Parse(ReadString(), CultureInfo.InvariantCulture),
                7 => DateTime.Parse(ReadString(), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind),
                8 => TimeSpan.FromMilliseconds(ReadInt64()),
                9 => ReadString(),
                var tag => throw new InvalidDataException($"The native MiniExcel frame contains unknown value tag {tag}.")
            };
        }

        public void EnsureComplete()
        {
            if (_offset != frame.Length)
                throw new InvalidDataException("The native MiniExcel frame contains trailing data.");
        }

        private bool ReadBoolean()
        {
            EnsureAvailable(1);
            return frame[_offset++] != 0;
        }

        public byte ReadByte()
        {
            EnsureAvailable(1);
            return frame[_offset++];
        }

        public uint ReadUInt32()
        {
            EnsureAvailable(sizeof(uint));
            var value = (uint)(frame[_offset]
                | frame[_offset + 1] << 8
                | frame[_offset + 2] << 16
                | frame[_offset + 3] << 24);
            _offset += sizeof(uint);
            return value;
        }

        private long ReadInt64()
        {
            EnsureAvailable(sizeof(long));
            ulong value = 0;
            for (var index = 0; index < sizeof(long); index++)
                value |= (ulong)frame[_offset + index] << (index * 8);
            _offset += sizeof(long);
            return unchecked((long)value);
        }

        private void EnsureAvailable(int length)
        {
            if (length < 0 || _offset > frame.Length - length)
                throw new InvalidDataException("The native MiniExcel frame is truncated.");
        }
    }

    private sealed class Utf8String : IDisposable
    {
        public Utf8String(string? value)
        {
            if (value is null)
                return;

            var bytes = Encoding.UTF8.GetBytes(value);
            Pointer = Marshal.AllocHGlobal(bytes.Length + 1);
            Marshal.Copy(bytes, 0, Pointer, bytes.Length);
            Marshal.WriteByte(Pointer, bytes.Length, 0);
        }

        public IntPtr Pointer { get; private set; }

        public void Dispose()
        {
            if (Pointer == IntPtr.Zero)
                return;

            Marshal.FreeHGlobal(Pointer);
            Pointer = IntPtr.Zero;
        }
    }

    private sealed class NativeQueryHandle : SafeHandleZeroOrMinusOneIsInvalid
    {
        public NativeQueryHandle() : base(true) { }

        public NativeQueryHandle(IntPtr value) : this()
        {
            SetHandle(value);
        }

        protected override bool ReleaseHandle()
        {
            NativeMethods.QueryClose(handle);
            return true;
        }
    }

    private sealed class NativeBufferHandle : SafeHandleZeroOrMinusOneIsInvalid
    {
        public NativeBufferHandle() : base(true) { }

        public NativeBufferHandle(IntPtr value) : this()
        {
            SetHandle(value);
        }

        protected override bool ReleaseHandle()
        {
            NativeMethods.BufferClose(handle);
            return true;
        }
    }

    private sealed class NativeCancellationHandle : SafeHandleZeroOrMinusOneIsInvalid
    {
        private NativeCancellationHandle() : base(true) { }

        public static NativeCancellationHandle Create()
        {
            var result = NativeMethods.CreateCancellation(out var rawHandle);
            if (result < 0)
                throw CreateNativeException(result);
            var handle = new NativeCancellationHandle();
            handle.SetHandle(rawHandle);
            return handle;
        }

        protected override bool ReleaseHandle()
        {
            NativeMethods.CloseCancellation(handle);
            return true;
        }
    }

    private static class NativeMethods
    {
        private const string LibraryName = "miniexcel_ffi";

        [DllImport(LibraryName, EntryPoint = "miniexcel_abi_version", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern uint GetAbiVersion();

        [DllImport(LibraryName, EntryPoint = "miniexcel_query_open", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int QueryOpen(
            IntPtr path,
            byte useHeaderRow,
            IntPtr sheetName,
            IntPtr startCell,
            out IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_query_range_open", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int QueryRangeOpen(
            IntPtr path,
            byte useHeaderRow,
            IntPtr sheetName,
            IntPtr startCell,
            IntPtr endCell,
            out IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_query_options_open", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int QueryOptionsOpen(
            IntPtr path,
            byte useHeaderRow,
            IntPtr sheetName,
            IntPtr startCell,
            IntPtr endCell,
            byte ignoreEmptyRows,
            byte fillMergedCells,
            byte trimHeaders,
            byte enableSharedStringCache,
            ulong sharedStringCacheSize,
            IntPtr sharedStringCachePath,
            out IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_query_table_open", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int QueryTableOpen(
            IntPtr path,
            IntPtr sheetName,
            IntPtr tableName,
            out IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_query_csv_open", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int QueryCsvOpen(
            IntPtr path,
            byte useHeaderRow,
            byte delimiter,
            byte encoding,
            byte readEmptyAsNull,
            byte trimHeaders,
            out IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_get_csv_columns", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int GetCsvColumns(
            IntPtr path,
            byte useHeaderRow,
            byte delimiter,
            byte encoding,
            byte readEmptyAsNull,
            byte trimHeaders,
            out IntPtr handle,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_query_next_batch", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int QueryNextBatch(
            NativeQueryHandle handle,
            uint maxRows,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_query_close", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern void QueryClose(IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_get_sheet_names", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int GetSheetNames(
            IntPtr path,
            out IntPtr handle,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_get_columns", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int GetColumns(
            IntPtr path,
            byte useHeaderRow,
            IntPtr sheetName,
            IntPtr startCell,
            out IntPtr handle,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_get_sheet_dimensions", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int GetSheetDimensions(
            IntPtr path,
            out IntPtr handle,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_get_sheet_info", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int GetSheetInfo(
            IntPtr path,
            out IntPtr handle,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_get_comments", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int GetComments(
            IntPtr path,
            IntPtr sheetName,
            out IntPtr handle,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_read_mapped", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int ReadMapped(
            IntPtr path,
            IntPtr sheetName,
            IntPtr mappingData,
            UIntPtr mappingLength,
            out IntPtr handle,
            out IntPtr data,
            out UIntPtr length);

        [DllImport(LibraryName, EntryPoint = "miniexcel_save_as", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int SaveAs(
            IntPtr path,
            IntPtr data,
            UIntPtr dataLength,
            byte printHeader,
            IntPtr sheetName,
            byte overwriteFile,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_save_as_sheets", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int SaveAsSheets(
            IntPtr path,
            IntPtr data,
            UIntPtr dataLength,
            byte printHeader,
            byte overwriteFile,
            out IntPtr handle,
            out IntPtr resultData,
            out UIntPtr resultLength);

        [DllImport(LibraryName, EntryPoint = "miniexcel_save_as_configured", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int SaveAsConfigured(
            IntPtr path,
            IntPtr data,
            UIntPtr dataLength,
            IntPtr optionsData,
            UIntPtr optionsLength,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_save_csv", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int SaveCsv(
            IntPtr path,
            IntPtr data,
            UIntPtr dataLength,
            byte delimiter,
            byte encoding,
            byte writeBom,
            byte printHeader,
            byte overwriteFile,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_append_csv", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int AppendCsv(
            IntPtr path,
            IntPtr data,
            UIntPtr dataLength,
            byte delimiter,
            byte encoding,
            byte writeBom,
            byte printHeader,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_rename_sheet", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int RenameSheet(IntPtr path, IntPtr sheetName, IntPtr newSheetName);

        [DllImport(LibraryName, EntryPoint = "miniexcel_reorder_sheet", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int ReorderSheet(IntPtr path, IntPtr sheetName, int newSheetIndex);

        [DllImport(LibraryName, EntryPoint = "miniexcel_set_sheet_visibility", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int SetSheetVisibility(IntPtr path, IntPtr sheetName, byte visibility);

        [DllImport(LibraryName, EntryPoint = "miniexcel_insert_sheet", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int InsertSheet(
            IntPtr path,
            IntPtr data,
            UIntPtr dataLength,
            IntPtr sheetName,
            byte printHeader,
            byte replaceExisting,
            byte removeSupportedRelationships,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_copy_and_add_sheet", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int CopyAndAddSheet(
            IntPtr sourcePath,
            IntPtr destinationPath,
            IntPtr data,
            UIntPtr dataLength,
            IntPtr sheetName,
            byte printHeader,
            byte replaceExisting,
            byte removeSupportedRelationships,
            byte overwriteDestination,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_fill_template", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int FillTemplate(
            IntPtr destinationPath,
            IntPtr templatePath,
            IntPtr payloadData,
            UIntPtr payloadLength,
            byte overwriteFile,
            byte ignoreMissingVariables);

        [DllImport(LibraryName, EntryPoint = "miniexcel_fill_mapped_template", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int FillMappedTemplate(
            IntPtr destinationPath,
            IntPtr templatePath,
            IntPtr payloadData,
            UIntPtr payloadLength,
            byte overwriteFile);

        [DllImport(LibraryName, EntryPoint = "miniexcel_merge_same_cells", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int MergeSameCells(
            IntPtr destinationPath,
            IntPtr sourcePath,
            byte overwriteFile);

        [DllImport(LibraryName, EntryPoint = "miniexcel_add_picture", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int AddPicture(
            IntPtr path,
            IntPtr sheetName,
            IntPtr cellAddress,
            IntPtr imageData,
            UIntPtr imageLength,
            uint widthPx,
            uint heightPx,
            byte anchorType,
            int locationX,
            int locationY);

        [DllImport(LibraryName, EntryPoint = "miniexcel_save_as_spooled_async", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int SaveAsSpooledAsync(
            IntPtr path,
            IntPtr spoolPath,
            IntPtr optionsData,
            UIntPtr optionsLength,
            NativeCancellationHandle cancellation,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_save_csv_spooled_async", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int SaveCsvSpooledAsync(
            IntPtr path,
            IntPtr spoolPath,
            IntPtr optionsData,
            UIntPtr optionsLength,
            NativeCancellationHandle cancellation,
            out uint rowCount);

        [DllImport(LibraryName, EntryPoint = "miniexcel_cancellation_create", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern int CreateCancellation(out IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_cancellation_cancel", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern void Cancel(NativeCancellationHandle handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_cancellation_close", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern void CloseCancellation(IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_buffer_close", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern void BufferClose(IntPtr handle);

        [DllImport(LibraryName, EntryPoint = "miniexcel_last_error", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern IntPtr GetLastError(out UIntPtr length);
    }
}