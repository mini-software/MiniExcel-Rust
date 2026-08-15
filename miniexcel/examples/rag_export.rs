use std::io::{self, Write};

use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

fn main() -> miniexcel::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "book.xlsx".to_owned());
    let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let export_options = RagExportOptions::new().with_chunk_rows(25);
    let mut export = MiniExcel::export_rag(path, &options, &export_options)?;
    let stdout = io::stdout();
    let mut output = stdout.lock();

    for chunk in export.by_ref() {
        serde_json::to_writer(&mut output, &chunk?)
            .map_err(|error| miniexcel::Error::from(io::Error::other(error)))?;
        output.write_all(b"\n")?;
    }

    eprintln!("{}", serde_json::to_string_pretty(export.manifest()).expect("serialize manifest"));
    Ok(())
}
