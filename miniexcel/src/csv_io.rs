use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Cursor, Read, Write};
use std::marker::PhantomData;
use std::path::Path;

use csv::{Reader, ReaderBuilder, StringRecord};
use encoding_rs_io::{DecodeReaderBytes, DecodeReaderBytesBuilder};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::reader::column_names;
use crate::{
    CellValue, CsvConfiguration, CsvReadOptions, CsvWriteOptions, DynamicRow, Error, Result,
};

type DecodedCsvReader<R> = Reader<DecodeReaderBytes<R, Vec<u8>>>;

pub(crate) struct CsvRows<R> {
    reader: DecodedCsvReader<R>,
    headers: Option<Vec<String>>,
    columns: Option<Vec<String>>,
    read_empty_as_null: bool,
    newline: &'static str,
    next_record: u64,
}

impl<R> CsvRows<R>
where
    R: Read,
{
    pub(crate) fn new(reader: R, options: &CsvReadOptions, typed: bool) -> Result<Self> {
        validate_configuration(options.configuration())?;
        let decoder = DecodeReaderBytesBuilder::new()
            .encoding(Some(options.configuration().encoding().encoding()))
            .bom_sniffing(true)
            .utf8_passthru(true)
            .build(reader);
        let mut reader = ReaderBuilder::new()
            .delimiter(options.configuration().delimiter())
            .has_headers(false)
            .flexible(false)
            .from_reader(decoder);
        let headers = if options.uses_headers(typed) {
            let mut record = StringRecord::new();
            if read_record(&mut reader, &mut record, 1)? {
                let headers = record
                    .iter()
                    .map(|value| {
                        if options.trim_headers() {
                            value.trim().to_owned()
                        } else {
                            value.to_owned()
                        }
                    })
                    .collect::<Vec<_>>();
                validate_schema(&headers)?;
                Some(headers)
            } else {
                Some(Vec::new())
            }
        } else {
            None
        };
        let next_record = if headers.is_some() { 2 } else { 1 };
        Ok(Self {
            reader,
            headers,
            columns: None,
            read_empty_as_null: options.configuration().read_empty_as_null(),
            newline: std::str::from_utf8(options.configuration().newline().bytes())
                .expect("CSV newline is ASCII"),
            next_record,
        })
    }

    pub(crate) fn headers(&self) -> Option<&[String]> {
        self.headers.as_deref()
    }

    fn next_record(&mut self) -> Option<Result<StringRecord>> {
        loop {
            let mut record = StringRecord::new();
            let record_number = self.next_record;
            self.next_record += 1;
            match read_record(&mut self.reader, &mut record, record_number) {
                Ok(false) => return None,
                Ok(true)
                    if record.len() == 1
                        && record.get(0).is_some_and(|value| value.trim().is_empty()) =>
                {
                    continue;
                }
                Ok(true) => {
                    let normalized = record
                        .iter()
                        .map(|value| normalize_field_newlines(value, self.newline))
                        .collect::<StringRecord>();
                    return Some(Ok(normalized));
                }
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

impl<R> Iterator for CsvRows<R>
where
    R: Read,
{
    type Item = Result<DynamicRow>;

    fn next(&mut self) -> Option<Self::Item> {
        let record = match self.next_record()? {
            Ok(record) => record,
            Err(error) => return Some(Err(error)),
        };
        let columns = if let Some(headers) = &self.headers {
            if record.len() != headers.len() {
                return Some(Err(Error::csv_read(
                    self.next_record - 1,
                    format!("record has {} fields but header has {}", record.len(), headers.len()),
                )));
            }
            headers.clone()
        } else {
            self.columns
                .get_or_insert_with(|| {
                    column_names(0, record.len())
                        .into_iter()
                        .map(|name| name.unwrap_or_default())
                        .collect()
                })
                .clone()
        };
        let mut row = DynamicRow::with_capacity(columns.len());
        for (column, value) in columns.into_iter().zip(record.iter()) {
            row.insert(
                column,
                if self.read_empty_as_null && value.is_empty() {
                    CellValue::Empty
                } else {
                    CellValue::String(value.to_owned())
                },
            );
        }
        Some(Ok(row))
    }
}

pub(crate) struct CsvTypedRows<R, T> {
    rows: CsvRows<R>,
    marker: PhantomData<fn() -> T>,
}

impl<R, T> CsvTypedRows<R, T>
where
    R: Read,
    T: DeserializeOwned,
{
    pub(crate) fn new(reader: R, options: &CsvReadOptions) -> Result<Self> {
        Ok(Self { rows: CsvRows::new(reader, options, true)?, marker: PhantomData })
    }
}

impl<R, T> Iterator for CsvTypedRows<R, T>
where
    R: Read,
    T: DeserializeOwned,
{
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let record = match self.rows.next_record()? {
            Ok(record) => record,
            Err(error) => return Some(Err(error)),
        };
        let record_number = self.rows.next_record - 1;
        let headers = self.rows.headers().map(|headers| StringRecord::from(headers.to_vec()));
        if let Some(headers) = &headers {
            if record.len() != headers.len() {
                return Some(Err(Error::csv_read(
                    record_number,
                    format!("record has {} fields but header has {}", record.len(), headers.len()),
                )));
            }
        }
        Some(
            record
                .deserialize(headers.as_ref())
                .map_err(|error| Error::csv_deserialize(record_number, error.to_string())),
        )
    }
}

pub(crate) fn query_path(
    path: impl AsRef<Path>,
    options: &CsvReadOptions,
) -> Result<CsvRows<BufReader<File>>> {
    CsvRows::new(BufReader::new(File::open(path)?), options, false)
}

pub(crate) fn query_path_as<T>(
    path: impl AsRef<Path>,
    options: &CsvReadOptions,
) -> Result<CsvTypedRows<BufReader<File>, T>>
where
    T: DeserializeOwned,
{
    CsvTypedRows::new(BufReader::new(File::open(path)?), options)
}

pub(crate) fn query_bytes(bytes: &[u8], options: &CsvReadOptions) -> Result<Vec<DynamicRow>> {
    CsvRows::new(Cursor::new(bytes), options, false)?.collect()
}

pub(crate) fn query_bytes_as<T>(bytes: &[u8], options: &CsvReadOptions) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    CsvTypedRows::new(Cursor::new(bytes), options)?.collect()
}

pub(crate) fn get_columns<R>(reader: R, options: &CsvReadOptions) -> Result<Vec<String>>
where
    R: Read,
{
    let mut rows = CsvRows::new(reader, options, false)?;
    if let Some(headers) = rows.headers() {
        return Ok(headers.to_vec());
    }
    let Some(record) = rows.next_record().transpose()? else {
        return Ok(Vec::new());
    };
    Ok(column_names(0, record.len()).into_iter().map(Option::unwrap_or_default).collect())
}

pub(crate) fn save_dynamic(
    path: impl AsRef<Path>,
    schema: Option<&[String]>,
    rows: &[DynamicRow],
    options: &CsvWriteOptions,
) -> Result<usize> {
    let path = path.as_ref();
    validate_configuration(options.configuration())?;
    let schema = prepare_schema(schema, rows, options)?;
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .create_new(!options.overwrite_file())
        .truncate(options.overwrite_file())
        .open(path)?;
    write_dynamic(file, Some(&schema), rows, options, true)
}

pub(crate) fn append_dynamic(
    path: impl AsRef<Path>,
    schema: Option<&[String]>,
    rows: &[DynamicRow],
    options: &CsvWriteOptions,
) -> Result<usize> {
    let path = path.as_ref();
    validate_configuration(options.configuration())?;
    let schema = prepare_schema(schema, rows, options)?;
    let empty = path.metadata().map_or(true, |metadata| metadata.len() == 0);
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    write_dynamic(file, Some(&schema), rows, options, empty)
}

pub(crate) fn write_dynamic<W>(
    mut writer: W,
    explicit_schema: Option<&[String]>,
    rows: &[DynamicRow],
    options: &CsvWriteOptions,
    allow_header_and_bom: bool,
) -> Result<usize>
where
    W: Write,
{
    validate_configuration(options.configuration())?;
    let schema = match explicit_schema {
        Some(schema) => schema.to_vec(),
        None => infer_schema(rows),
    };
    validate_schema(&schema)?;
    if schema.is_empty() && options.print_header() {
        return Err(Error::missing_schema());
    }
    if allow_header_and_bom && options.configuration().write_bom() {
        write_bom(&mut writer, options.configuration().encoding())?;
    }
    if allow_header_and_bom && options.print_header() && !schema.is_empty() {
        write_record(&mut writer, &schema, options.configuration())?;
    }
    for row in rows {
        let fields = schema
            .iter()
            .map(|column| row.get(column).map_or_else(String::new, csv_cell_text))
            .collect::<Vec<_>>();
        write_record(&mut writer, &fields, options.configuration())?;
    }
    writer.flush()?;
    Ok(rows.len())
}

pub(crate) fn save_serialized<T>(
    path: impl AsRef<Path>,
    rows: &[T],
    options: &CsvWriteOptions,
) -> Result<usize>
where
    T: Serialize,
{
    let dynamic = serialize_rows(rows)?;
    save_dynamic(path, None, &dynamic, options)
}

pub(crate) fn append_serialized<T>(
    path: impl AsRef<Path>,
    rows: &[T],
    options: &CsvWriteOptions,
) -> Result<usize>
where
    T: Serialize,
{
    let dynamic = serialize_rows(rows)?;
    append_dynamic(path, None, &dynamic, options)
}

pub(crate) fn write_serialized<W, T>(
    writer: W,
    rows: &[T],
    options: &CsvWriteOptions,
    allow_header_and_bom: bool,
) -> Result<usize>
where
    W: Write,
    T: Serialize,
{
    let dynamic = serialize_rows(rows)?;
    write_dynamic(writer, None, &dynamic, options, allow_header_and_bom)
}

fn serialize_rows<T>(rows: &[T]) -> Result<Vec<DynamicRow>>
where
    T: Serialize,
{
    rows.iter()
        .map(|row| {
            let value = serde_json::to_value(row)
                .map_err(|error| Error::csv_write(format!("cannot serialize CSV row: {error}")))?;
            let object = value
                .as_object()
                .ok_or_else(|| Error::csv_write("CSV rows must serialize as objects"))?;
            Ok(object.iter().map(|(name, value)| (name.clone(), json_cell(value))).collect())
        })
        .collect()
}

fn json_cell(value: &serde_json::Value) -> CellValue {
    match value {
        serde_json::Value::Null => CellValue::Empty,
        serde_json::Value::Bool(value) => {
            CellValue::String(if *value { "True" } else { "False" }.to_owned())
        }
        serde_json::Value::Number(value) => CellValue::String(value.to_string()),
        serde_json::Value::String(value) => CellValue::String(value.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            CellValue::String(value.to_string())
        }
    }
}

fn infer_schema(rows: &[DynamicRow]) -> Vec<String> {
    let mut seen = HashSet::new();
    rows.iter()
        .flat_map(|row| row.keys())
        .filter(|column| seen.insert((*column).clone()))
        .cloned()
        .collect()
}

fn validate_schema(schema: &[String]) -> Result<()> {
    let mut seen = HashSet::new();
    for column in schema {
        if !seen.insert(column) {
            return Err(Error::duplicate_column_name(column));
        }
    }
    Ok(())
}

fn validate_configuration(configuration: &CsvConfiguration) -> Result<()> {
    let delimiter = configuration.delimiter();
    if matches!(delimiter, b'\r' | b'\n' | b'"') {
        return Err(Error::invalid_write_options("CSV delimiter cannot be CR, LF, or quote"));
    }
    Ok(())
}

fn read_record<R>(reader: &mut Reader<R>, record: &mut StringRecord, number: u64) -> Result<bool>
where
    R: Read,
{
    reader.read_record(record).map_err(|error| Error::csv_read(number, error.to_string()))
}

fn csv_cell_text(value: &CellValue) -> String {
    match value {
        CellValue::Empty => String::new(),
        CellValue::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
        CellValue::Int(value) => value.to_string(),
        CellValue::Float(value) => value.to_string(),
        CellValue::String(value) | CellValue::Error(value) => value.clone(),
        CellValue::Date(value) => value.format("%Y-%m-%d").to_string(),
        CellValue::Time(value) => value.format("%H:%M:%S").to_string(),
        CellValue::DateTime(value) => value.format("%Y-%m-%d %H:%M:%S").to_string(),
        CellValue::Duration(value) => value.num_milliseconds().to_string(),
    }
}

fn write_record<W>(
    writer: &mut W,
    fields: &[String],
    configuration: &CsvConfiguration,
) -> Result<()>
where
    W: Write,
{
    let delimiter = char::from(configuration.delimiter()).to_string();
    let line = fields
        .iter()
        .map(|field| quote_field(field, configuration))
        .collect::<Vec<_>>()
        .join(&delimiter)
        + std::str::from_utf8(configuration.newline().bytes()).expect("ASCII newline");
    write_encoded(writer, &line, configuration.encoding())
}

fn quote_field(field: &str, configuration: &CsvConfiguration) -> String {
    let newline = std::str::from_utf8(configuration.newline().bytes()).expect("ASCII newline");
    let field = normalize_field_newlines(field, newline);
    let quote = configuration.always_quote()
        || field.contains(char::from(configuration.delimiter()))
        || field.contains(['"', '\r', '\n'])
        || (configuration.quote_whitespace() && field.contains(' '));
    if quote { format!("\"{}\"", field.replace('"', "\"\"")) } else { field }
}

fn write_bom<W>(writer: &mut W, encoding: crate::CsvEncoding) -> Result<()>
where
    W: Write,
{
    match encoding {
        crate::CsvEncoding::Utf8 => writer.write_all(b"\xEF\xBB\xBF")?,
        crate::CsvEncoding::Utf16Le => writer.write_all(b"\xFF\xFE")?,
        crate::CsvEncoding::Utf16Be => writer.write_all(b"\xFE\xFF")?,
        crate::CsvEncoding::Gbk | crate::CsvEncoding::Windows1252 => {}
    }
    Ok(())
}

fn write_encoded<W>(writer: &mut W, value: &str, encoding: crate::CsvEncoding) -> Result<()>
where
    W: Write,
{
    if matches!(encoding, crate::CsvEncoding::Utf16Le | crate::CsvEncoding::Utf16Be) {
        for unit in value.encode_utf16() {
            let bytes = if encoding == crate::CsvEncoding::Utf16Le {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            };
            writer.write_all(&bytes)?;
        }
        return Ok(());
    }
    let (encoded, _, had_errors) = encoding.encoding().encode(value);
    if had_errors {
        return Err(Error::csv_encoding(encoding.encoding().name()));
    }
    writer.write_all(&encoded)?;
    Ok(())
}

fn normalize_field_newlines(value: &str, newline: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n").replace('\n', newline)
}

fn prepare_schema(
    explicit_schema: Option<&[String]>,
    rows: &[DynamicRow],
    options: &CsvWriteOptions,
) -> Result<Vec<String>> {
    let schema = explicit_schema.map_or_else(|| infer_schema(rows), <[String]>::to_vec);
    validate_schema(&schema)?;
    if schema.is_empty() && options.print_header() {
        return Err(Error::missing_schema());
    }
    Ok(schema)
}
