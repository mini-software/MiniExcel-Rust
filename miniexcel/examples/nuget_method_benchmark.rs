use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use miniexcel::{CellValue, DynamicRow, MiniExcel, TemplateOptions};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct BenchmarkResult {
    method: String,
    runtime: &'static str,
    dot_net_runtime: Option<String>,
    passes: usize,
    rows: u64,
    cells: u64,
    content_hash: Option<String>,
    output_path: Option<String>,
    elapsed_milliseconds: f64,
    first_row_milliseconds: Option<f64>,
    allocated_bytes: Option<u64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 7 {
        return Err(usage().into());
    }
    let method = arguments[0].as_str();
    let input_path = PathBuf::from(&arguments[1]);
    let output_path = PathBuf::from(&arguments[2]);
    let rows = arguments[3].parse::<usize>()?;
    let columns = arguments[4].parse::<usize>()?;
    let passes = arguments[5].parse::<usize>()?;
    let warmups = arguments[6].parse::<usize>()?;
    if rows == 0 || columns == 0 || passes == 0 {
        return Err(usage().into());
    }

    let prepared_rows = (method == "create").then(|| create_rows(rows, columns));
    let template_value = (method == "template").then(|| {
        json!({
            "employees": (0..rows)
                .map(|_| json!({ "name": "Jack", "department": "HR" }))
                .collect::<Vec<_>>()
        })
    });
    for _ in 0..warmups {
        if matches!(method, "query" | "query-first") {
            query(&input_path, method == "query-first", None)?;
        } else {
            run_write_method(
                method,
                &input_path,
                &output_path,
                prepared_rows.as_deref(),
                template_value.as_ref(),
            )?;
        }
    }

    let started = Instant::now();
    let mut total_rows = 0_u64;
    let mut total_cells = 0_u64;
    let mut first_row_milliseconds = None;
    let mut content_hash = None;
    for _ in 0..passes {
        match method {
            "query" | "query-first" => {
                let (result_rows, result_cells, hash, first_row) =
                    query(&input_path, method == "query-first", Some(started))?;
                total_rows += result_rows;
                total_cells += result_cells;
                if first_row_milliseconds.is_none() {
                    first_row_milliseconds = Some(first_row);
                }
                content_hash = Some(hash);
            }
            "create" | "template" => {
                run_write_method(
                    method,
                    &input_path,
                    &output_path,
                    prepared_rows.as_deref(),
                    template_value.as_ref(),
                )?;
                total_rows += rows as u64;
                total_cells += (rows * columns) as u64;
            }
            _ => return Err(usage().into()),
        }
    }
    let elapsed_milliseconds = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{}",
        serde_json::to_string(&BenchmarkResult {
            method: method.to_owned(),
            runtime: "MiniExcel.Rust",
            dot_net_runtime: None,
            passes,
            rows: total_rows,
            cells: total_cells,
            content_hash,
            output_path: matches!(method, "create" | "template")
                .then(|| output_path.to_string_lossy().into_owned()),
            elapsed_milliseconds,
            first_row_milliseconds,
            allocated_bytes: None,
        })?
    );
    Ok(())
}

fn query(
    path: &Path,
    first_only: bool,
    started: Option<Instant>,
) -> Result<(u64, u64, String, f64), Box<dyn Error>> {
    let mut rows = 0_u64;
    let mut cells = 0_u64;
    let mut first_row_milliseconds = 0.0;
    let mut hasher = Sha256::new();
    for row in MiniExcel::query(path)? {
        let row = row?;
        if rows == 0 {
            first_row_milliseconds =
                started.map_or(0.0, |value| value.elapsed().as_secs_f64() * 1000.0);
        }
        rows += 1;
        cells += row.len() as u64;
        for (name, value) in &row {
            append_text(&mut hasher, name);
            append_text(&mut hasher, &normalize(value));
        }
        if first_only {
            break;
        }
    }
    Ok((rows, cells, format!("{:X}", hasher.finalize()), first_row_milliseconds))
}

fn run_write_method(
    method: &str,
    input_path: &Path,
    output_path: &Path,
    rows: Option<&[DynamicRow]>,
    template_value: Option<&serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    if output_path.exists() {
        fs::remove_file(output_path)?;
    }
    match method {
        "create" => MiniExcel::save_as(output_path, rows.ok_or_else(usage)?),
        "template" => MiniExcel::save_as_template(
            output_path,
            input_path,
            template_value.ok_or_else(usage)?,
            &TemplateOptions::new(),
        ),
        _ => return Err(usage().into()),
    }?;
    Ok(())
}

fn create_rows(row_count: usize, column_count: usize) -> Vec<DynamicRow> {
    (0..row_count)
        .map(|_| {
            (1..=column_count)
                .map(|column| {
                    (format!("Column{column}"), CellValue::String("Hello World".to_owned()))
                })
                .collect()
        })
        .collect()
}

fn append_text(hasher: &mut Sha256, value: &str) {
    let bytes = value.as_bytes();
    hasher.update((bytes.len() as i32).to_le_bytes());
    hasher.update(bytes);
}

fn normalize(value: &CellValue) -> String {
    match value {
        CellValue::Empty => "null".to_owned(),
        CellValue::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
        CellValue::Int(value) => value.to_string(),
        CellValue::Float(value) => value.to_string(),
        CellValue::String(value) | CellValue::Error(value) => value.clone(),
        CellValue::Date(value) => value.to_string(),
        CellValue::Time(value) => value.to_string(),
        CellValue::DateTime(value) => value.to_string(),
        CellValue::Duration(value) => value.to_string(),
    }
}

fn usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: nuget_method_benchmark <query-first|query|create|template> <input-path> <output-path> <rows> <columns> <passes> <warmups>",
    )
}