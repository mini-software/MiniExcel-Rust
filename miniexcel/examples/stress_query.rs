use std::env;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use miniexcel::MiniExcel;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct QueryResult {
    rows: u64,
    cells: u64,
    query_elapsed_ms: f64,
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

    run_query(&path, warmup_passes)?;
    let started = Instant::now();
    let (rows, cells) = run_query(&path, measured_passes)?;
    let result =
        QueryResult { rows, cells, query_elapsed_ms: started.elapsed().as_secs_f64() * 1000.0 };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn run_query(path: &Path, passes: usize) -> Result<(u64, u64), Box<dyn Error>> {
    let mut rows = 0_u64;
    let mut cells = 0_u64;
    for _ in 0..passes {
        for row in MiniExcel::query(path)? {
            let row = row?;
            rows += 1;
            cells += row.len() as u64;
        }
    }
    Ok((rows, cells))
}
