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
