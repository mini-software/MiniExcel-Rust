<div align="center">

# MiniExcel para Rust

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Français](README.fr.md) | [日本語](README.ja.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Descargas](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Documentación](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![Licencia](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

**Procesamiento XLSX y CSV rápido, con memoria acotada.**

</div>

---

<div align="center">

Este proyecto forma parte de la familia [MiniExcel](https://github.com/mini-software/MiniExcel) y usa la biblioteca .NET como referencia de compatibilidad.

</div>

---

<div align="center">

**[Abrir Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)** para inspeccionar o generar XLSX localmente. Los datos permanecen en el navegador.

</div>

---

## Introducción

MiniExcel para Rust es una biblioteca de lectura/escritura XLSX y CSV con streaming de memoria acotada, Serde, análisis y exportaciones RAG.

## Instalación

```bash
cargo add miniexcel
```

Requiere Rust 1.85.0 o posterior.

## Inicio Rápido

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    println!("{:?}", row?["A"]);
}
```

```rust
use miniexcel::{CellValue, DynamicRow, MiniExcel};

let mut row = DynamicRow::new();
row.insert("Name".into(), CellValue::String("MiniExcel".into()));
MiniExcel::save_as("book.xlsx", &[row])?;
```

Ejecuta los ejemplos incluidos con tus propios archivos:

```bash
cargo run -p miniexcel --example read -- book.xlsx
cargo run -p miniexcel --example write -- output.xlsx
cargo run -p miniexcel --example rag_export -- book.xlsx
```

## Flujos De Trabajo Comunes

Todas las API devuelven `miniexcel::Result`. Los fragmentos siguientes se
pueden colocar dentro de `fn main() -> miniexcel::Result<()>`. Los ejemplos
tipados requieren `cargo add serde --features derive`; el ejemplo de template
también requiere `cargo add serde_json`.

### Seleccionar Una Worksheet Y Un Rango

`query()` no usa encabezados por defecto y emplea las letras de columna de
Excel como claves. Usa `HeaderMode::FirstRow` si la primera fila seleccionada
contiene nombres. Las celdas inicial y final están incluidas.

```rust
use miniexcel::{HeaderMode, MiniExcel, ReadOptions};

let options = ReadOptions::new()
    .with_sheet_name("Data")
    .with_header_mode(HeaderMode::FirstRow)
    .with_start_cell("A1".parse()?)
    .with_end_cell("F100".parse()?)
    .with_ignore_empty_rows(true);

for row in MiniExcel::query_with_options("book.xlsx", &options)? {
    let row = row?;
    println!("{:?}", row["Name"]);
}
```

El iterador posee un worker acotado. Si se detiene antes, por ejemplo con
`.take(10)`, al descartarlo también se detiene el resto de la consulta por path.

### Deserializar Filas Tipadas Con Serde

Las consultas tipadas tratan por defecto la primera fila seleccionada como
encabezados y realizan el mapping fila por fila.

```rust
use miniexcel::MiniExcel;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    version: u32,
}

for release in MiniExcel::query_as::<Release>("book.xlsx")? {
    let release = release?;
    println!("{} {}", release.name, release.version);
}
```

Usa `miniexcel::serde_helpers` para convertir estrictamente fechas y horas
seriales de Excel a tipos `chrono`.

### Crear Un Workbook Desde Filas Serde

```rust
use miniexcel::{MiniExcel, WriteOptions};

#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct Release<'a> {
    name: &'a str,
    version: u32,
}

let rows = [
    Release { name: "MiniExcel", version: 1 },
    Release { name: "MiniExcel Rust", version: 4 },
];
let options = WriteOptions::new()
    .with_sheet_name("Releases")
    .with_auto_width(true);
MiniExcel::save_as_serialized_with_options("releases.xlsx", &rows, &options)?;
```

Las API Save crean un workbook y rechazan un path de salida existente salvo que
se indique explícitamente `with_overwrite_file(true)`.

### Modificar Atómicamente Un Workbook Existente

```rust
use miniexcel::{InsertOptions, MiniExcel};

let inserted = MiniExcel::insert_serialized(
    "book.xlsx",
    &rows,
    &InsertOptions::new().with_sheet_name("Archive"),
)?;
MiniExcel::rename_sheet("book.xlsx", "Archive", "History")?;
MiniExcel::reorder_sheet("book.xlsx", "History", 0)?;
println!("inserted {inserted} rows");
```

Las mutaciones por path bloquean, reescriben y validan el workbook antes de
reemplazarlo atómicamente. Un nombre de worksheet existente se rechaza por
defecto; selecciona `ExistingSheetPolicy::Replace` de forma explícita para
reemplazarla.

### Rellenar Un Template XLSX

Coloca marcadores como `{{title}}`, `{{items.name}}` y `{{items.score}}` en el
workbook template. Una lista expande su fila de plantilla.

```rust
use miniexcel::{MiniExcel, TemplateOptions};
use serde_json::json;

MiniExcel::save_as_template(
    "report.xlsx",
    "template.xlsx",
    &json!({
        "title": "Quarterly report",
        "items": [
            { "name": "Ada", "score": 10 },
            { "name": "Linus", "score": 20 }
        ]
    }),
    &TemplateOptions::new(),
)?;
```

### Exportar Chunks RAG Con Origen Trazable

```rust
use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

let read = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let rag = RagExportOptions::new().with_chunk_rows(25).with_max_rows(500);
let mut export = MiniExcel::export_rag("book.xlsx", &read, &rag)?;

for chunk in export.by_ref() {
    let chunk = chunk?;
    println!("{} {}", chunk.chunk_id(), chunk.data_range());
}
println!("source SHA-256: {}", export.manifest().source_sha256());
```

Cada chunk conserva la identidad de worksheet/rango, direcciones A1, valores
tipados en caché, texto de fórmulas, style IDs y number formats. Las worksheets
ocultas requieren autorización explícita. Consulta el
[contrato RAG](docs/analytics-and-rag.md) para la salida JSONL y Markdown en
streaming.

### Usar El CLI Del Repositorio

El CLI es una herramienta local del workspace y no se publica como crate
independiente.

```bash
cargo run -p miniexcel-cli -- sheets book.xlsx
cargo run -p miniexcel-cli -- query book.xlsx --sheet Data --header --start-cell A1 --end-cell F100 --format jsonl
cargo run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --format both --output-prefix ./out/book
```

### Elegir Una Forma De I/O

| Entrada o salida | API principales | Comportamiento de memoria |
| --- | --- | --- |
| Path de archivo | `query*`, `query_as*`, `save_as*`, `insert*` | Pipeline de filas acotado; las shared strings grandes pueden usar un índice en disco |
| Bytes XLSX | `query_bytes`, `save_as_bytes`, `visit_rag_chunks_from_bytes` | Los bytes del workbook permanecen en memoria |
| Streams prestados | `visit_rows_from_reader`, `save_as_to_writer` | El llamador conserva la propiedad del stream |
| Navegador | `miniexcel-wasm` y [Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) | WebAssembly local; los bytes cargados y las descargas completas usan memoria del navegador |

Hay más programas ejecutables en [`miniexcel/examples`](miniexcel/examples).
Consulta la [matriz de compatibilidad](docs/compatibility.md) antes de depender
de la edición de workbooks, templates, fórmulas o formato.

## Capacidades

- Consultas dinámicas, tipadas, estructuradas, Table y CSV con memoria acotada.
- API por path, bytes y reader/writer prestado.
- Lectura/escritura Serde, helpers de fecha/hora y mapping exacto de cells.
- Creación multi-worksheet, opciones de formato y visibilidad.
- Insert/replace, rename, reorder, copy y visibilidad atómicos de worksheets.
- Templates, bloques condicionales/agrupados y combinación mediante markers.
- Análisis agrupados en streaming con límites explícitos.
- Exportaciones JSONL y Markdown con direcciones de origen para LLM/RAG.
- Streams async opcionales e independientes del runtime; ZIP/XML/filesystem siguen siendo bloqueantes.

## Semántica Clave

- Las consultas por path transmiten el XML sin conservar todas las filas.
- La worksheet predeterminada es la primera, no la pestaña activa.
- La lectura devuelve valores de fórmula en caché; la lectura estructurada también expone texto y formatos. MiniExcel no calcula fórmulas.
- Save crea un workbook y rechaza paths existentes por defecto; Insert modifica `.xlsx` atómicamente después de validarlo.
- Las shared strings grandes pueden usar archivos temporales indexados; las consultas bytes/WASM las mantienen en memoria.
- No compatible: `.xls`, `.xlsb`, `.ods`, macros, creación de imágenes, cálculo de fórmulas o sistema general de estilos.

Consulta la [matriz de compatibilidad](docs/compatibility.md), el [contrato de análisis/RAG](docs/analytics-and-rag.md) y la [guía de migración Insert](docs/insert-v1-migration.md).

## Benchmark De Rust Y .NET

Coloca este repositorio junto a [.NET MiniExcel](https://github.com/mini-software/MiniExcel) y ejecuta:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

El informe se escribe en `target/benchmarks/dotnet-v1-vs-rust.json`. Compara solo resultados de la misma máquina; consulta la [metodología](docs/dotnet-v1-query-benchmark.md).
