use std::env;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use miniexcel::{CellValue, MiniExcel};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct QueryResult {
    runtime: &'static str,
    dot_net_runtime: Option<String>,
    passes: usize,
    rows: u64,
    cells: u64,
    content_hash: String,
    elapsed_milliseconds: f64,
    first_row_milliseconds: f64,
    allocated_bytes: Option<u64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let path = PathBuf::from(args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: stress_query <xlsx-path> [measured-passes] [warmup-passes]",
        )
    })?);
    let measured_passes =
        args.next().map(|value| value.to_string_lossy().parse::<usize>()).transpose()?.unwrap_or(1);
    let warmup_passes =
        args.next().map(|value| value.to_string_lossy().parse::<usize>()).transpose()?.unwrap_or(0);

    if measured_passes == 0 || args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: stress_query <xlsx-path> [measured-passes] [warmup-passes]",
        )
        .into());
    }

    run_query(&path, warmup_passes, None)?;
    let started = Instant::now();
    let (rows, cells, content_hash, first_row_milliseconds) =
        run_query(&path, measured_passes, Some(started))?;
    let result = QueryResult {
        runtime: "MiniExcel.Rust",
        dot_net_runtime: None,
        passes: measured_passes,
        rows,
        cells,
        content_hash,
        elapsed_milliseconds: started.elapsed().as_secs_f64() * 1000.0,
        first_row_milliseconds,
        allocated_bytes: None,
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn run_query(
    path: &Path,
    passes: usize,
    started: Option<Instant>,
) -> Result<(u64, u64, String, f64), Box<dyn Error>> {
    let mut rows = 0_u64;
    let mut cells = 0_u64;
    let mut first_row_milliseconds = 0.0;
    let mut hasher = Sha256::new();
    for _ in 0..passes {
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
        }
    }
    Ok((rows, cells, format!("{:X}", hasher.finalize()), first_row_milliseconds))
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
