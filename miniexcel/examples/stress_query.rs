use std::env;
use std::error::Error;
use std::io;

use miniexcel::MiniExcel;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let path = args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "usage: stress_query <xlsx-path> [passes]")
    })?;
    let passes =
        args.next().map(|value| value.to_string_lossy().parse::<usize>()).transpose()?.unwrap_or(1);

    if passes == 0 || args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: stress_query <xlsx-path> [passes]",
        )
        .into());
    }

    let mut row_count = 0_u64;
    for _ in 0..passes {
        for row in MiniExcel::query(&path)? {
            let _ = row?;
            row_count += 1;
        }
    }

    println!("{row_count}");
    Ok(())
}
