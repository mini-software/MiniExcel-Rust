#![cfg(all(feature = "async", not(target_arch = "wasm32")))]

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use futures_executor::block_on;
use futures_util::task::noop_waker_ref;
use futures_util::{StreamExt, stream};
use miniexcel::{
    CancellationToken, CellValue, DynamicRow, HeaderMode, MiniExcel, ReadOptions, WriteOptions,
};
use serde::Serialize;

#[derive(Serialize)]
struct TypedExportRow {
    #[serde(rename = "Name")]
    name: String,
    #[serde(
        rename = "Released",
        serialize_with = "miniexcel::serde_helpers::serialize_date_to_excel"
    )]
    released: chrono::NaiveDate,
}

#[test]
fn infers_schema_and_exports_serde_rows_from_async_streams() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("typed-async.xlsx");
    let rows = [
        TypedExportRow {
            name: "MiniExcel".to_owned(),
            released: chrono::NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
        },
        TypedExportRow {
            name: "Rust".to_owned(),
            released: chrono::NaiveDate::from_ymd_opt(2026, 8, 27).unwrap(),
        },
    ];

    let count = block_on(MiniExcel::save_as_serialized_async(
        &path,
        stream::iter(rows.map(Ok)),
        &WriteOptions::new().with_column_format("Released", "yyyy-mm-dd"),
    ))
    .unwrap();

    assert_eq!(count, 2);
    let rows = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows[0]["Name"], CellValue::String("MiniExcel".to_owned()));
    assert_eq!(rows[1]["Name"], CellValue::String("Rust".to_owned()));
    assert_eq!(
        rows[0]["Released"],
        CellValue::DateTime(
            chrono::NaiveDate::from_ymd_opt(2026, 8, 26).unwrap().and_hms_opt(0, 0, 0).unwrap()
        )
    );
}

#[test]
fn typed_async_export_handles_empty_streams_and_preflights_before_polling() {
    let directory = tempfile::tempdir().unwrap();
    let missing_schema = directory.path().join("missing-schema.xlsx");
    let error = block_on(MiniExcel::save_as_serialized_async::<TypedExportRow, _>(
        &missing_schema,
        stream::empty(),
        &WriteOptions::new(),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("without an explicit schema"));
    assert!(!missing_schema.exists());

    let empty = directory.path().join("empty.xlsx");
    let count = block_on(MiniExcel::save_as_serialized_async::<TypedExportRow, _>(
        &empty,
        stream::empty(),
        &WriteOptions::new().with_print_header(false),
    ))
    .unwrap();
    assert_eq!(count, 0);
    assert_eq!(MiniExcel::get_sheet_names(&empty).unwrap(), ["Sheet1"]);

    let existing = directory.path().join("existing.xlsx");
    std::fs::write(&existing, b"existing").unwrap();
    let polls = Rc::new(Cell::new(0_usize));
    let observed = Rc::clone(&polls);
    let rows = stream::poll_fn(move |_| {
        observed.set(observed.get() + 1);
        Poll::Ready(Some(Ok(TypedExportRow {
            name: "Unexpected".to_owned(),
            released: chrono::NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
        })))
    });
    let error =
        block_on(MiniExcel::save_as_serialized_async(&existing, rows, &WriteOptions::new()))
            .unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(polls.get(), 0);
    assert_eq!(std::fs::read(existing).unwrap(), b"existing");
}

#[test]
fn typed_async_export_rejects_schema_drift_and_honors_precancellation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("schema-drift.xlsx");
    std::fs::write(&path, b"existing").unwrap();
    let rows = stream::iter([
        Ok(serde_json::json!({"Name": "First"})),
        Ok(serde_json::json!({"Name": "Second", "Extra": 2})),
    ]);
    let error = block_on(MiniExcel::save_as_serialized_async(
        &path,
        rows,
        &WriteOptions::new().with_overwrite_file(true),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("fields do not match the inferred schema"));
    assert_eq!(std::fs::read(&path).unwrap(), b"existing");
    assert_no_temporary_files(directory.path());

    let cancelled = directory.path().join("cancelled-typed.xlsx");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&polls);
    let progress = Arc::new(AtomicUsize::new(0));
    let observed_progress = Arc::clone(&progress);
    let rows = stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Some(Ok(TypedExportRow {
            name: "Never".to_owned(),
            released: chrono::NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
        })))
    });
    let error = block_on(MiniExcel::save_as_serialized_async_with_cancellation_and_progress(
        &cancelled,
        rows,
        &WriteOptions::new(),
        cancellation,
        move |cells| {
            observed_progress.fetch_add(cells, Ordering::SeqCst);
        },
    ))
    .unwrap_err();
    assert!(error.is_cancelled());
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(progress.load(Ordering::SeqCst), 0);
    assert!(!cancelled.exists());
}

#[test]
fn reports_written_data_cells_for_async_export() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("progress.xlsx");
    let progress = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&progress);
    let mut partial = DynamicRow::new();
    partial.insert("Name".to_owned(), CellValue::String("Second".to_owned()));

    let count = block_on(MiniExcel::save_as_with_schema_async_with_progress(
        &path,
        &schema(),
        stream::iter([Ok(row("First", 1)), Ok(partial)]),
        &WriteOptions::new(),
        move |cells| {
            observed.fetch_add(cells, Ordering::SeqCst);
        },
    ))
    .unwrap();

    assert_eq!(count, 2);
    assert_eq!(progress.load(Ordering::SeqCst), 4);
}

#[test]
fn reports_serde_cells_and_suppresses_progress_before_writing() {
    let directory = tempfile::tempdir().unwrap();
    let typed = directory.path().join("typed-progress.xlsx");
    let progress = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&progress);
    let rows = [
        TypedExportRow {
            name: "MiniExcel".to_owned(),
            released: chrono::NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
        },
        TypedExportRow {
            name: "Rust".to_owned(),
            released: chrono::NaiveDate::from_ymd_opt(2026, 8, 27).unwrap(),
        },
    ];
    let count = block_on(MiniExcel::save_as_serialized_async_with_progress(
        &typed,
        stream::iter(rows.map(Ok)),
        &WriteOptions::new(),
        move |cells| {
            observed.fetch_add(cells, Ordering::SeqCst);
        },
    ))
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(progress.load(Ordering::SeqCst), 4);

    let existing = directory.path().join("existing.xlsx");
    std::fs::write(&existing, b"existing").unwrap();
    let progress = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&progress);
    let error = block_on(MiniExcel::save_as_with_schema_async_with_progress(
        &existing,
        &schema(),
        stream::iter([Ok(row("Never", 1))]),
        &WriteOptions::new(),
        move |cells| {
            observed.fetch_add(cells, Ordering::SeqCst);
        },
    ))
    .unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(progress.load(Ordering::SeqCst), 0);

    let producer_error = match MiniExcel::query(directory.path().join("missing.xlsx")) {
        Ok(_) => panic!("missing workbook should fail"),
        Err(error) => error,
    };
    let progress = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&progress);
    let error = block_on(MiniExcel::save_as_with_schema_async_with_cancellation_and_progress(
        &existing,
        &schema(),
        stream::iter([Ok(row("First", 1)), Err(producer_error)]),
        &WriteOptions::new().with_overwrite_file(true),
        CancellationToken::new(),
        move |cells| {
            observed.fetch_add(cells, Ordering::SeqCst);
        },
    ))
    .unwrap_err();
    assert!(!error.is_cancelled());
    assert_eq!(progress.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(existing).unwrap(), b"existing");
}

#[test]
fn exports_rows_and_header_only_workbooks_from_async_streams() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("async.xlsx");
    let count = block_on(MiniExcel::save_as_with_schema_async(
        &path,
        &schema(),
        stream::iter([Ok(row("First", 1)), Ok(row("Second", 2))]),
        &WriteOptions::new().with_sheet_name("Async"),
    ))
    .unwrap();
    assert_eq!(count, 2);
    let rows = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_sheet_name("Async").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["Name"], CellValue::String("Second".to_owned()));

    let empty = directory.path().join("empty.xlsx");
    let count = block_on(MiniExcel::save_as_with_schema_async(
        &empty,
        &schema(),
        stream::empty(),
        &WriteOptions::new(),
    ))
    .unwrap();
    assert_eq!(count, 0);
    let header = MiniExcel::query(&empty).unwrap().next().unwrap().unwrap();
    assert_eq!(header["A"], CellValue::String("Name".to_owned()));
    assert_eq!(header["B"], CellValue::String("Value".to_owned()));
}

#[test]
fn rejects_existing_destination_before_polling_and_overwrites_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.xlsx");
    std::fs::write(&path, b"existing").unwrap();
    let pulls = Rc::new(Cell::new(0_usize));
    let observed = Rc::clone(&pulls);
    let rows = stream::poll_fn(move |_| {
        observed.set(observed.get() + 1);
        Poll::Ready(Some(Ok(row("Unexpected", 1))))
    });

    let error = block_on(MiniExcel::save_as_with_schema_async(
        &path,
        &schema(),
        rows,
        &WriteOptions::new(),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(pulls.get(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), b"existing");

    let count = block_on(MiniExcel::save_as_with_schema_async(
        &path,
        &schema(),
        stream::iter([Ok(row("Replacement", 3))]),
        &WriteOptions::new().with_overwrite_file(true),
    ))
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(MiniExcel::get_sheet_names(path).unwrap(), ["Sheet1"]);
}

#[test]
fn producer_error_and_cancellation_preserve_destination_state() {
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("existing.xlsx");
    std::fs::write(&existing, b"existing").unwrap();
    let producer_error = match MiniExcel::query(directory.path().join("missing.xlsx")) {
        Ok(_) => panic!("missing workbook should fail"),
        Err(error) => error,
    };
    let result = block_on(MiniExcel::save_as_with_schema_async(
        &existing,
        &schema(),
        stream::iter([Ok(row("First", 1)), Err(producer_error), Ok(row("Never", 2))]),
        &WriteOptions::new().with_overwrite_file(true),
    ));
    assert!(result.is_err());
    assert_eq!(std::fs::read(&existing).unwrap(), b"existing");

    let missing = directory.path().join("cancelled.xlsx");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&polls);
    let rows = stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Some(Ok(row("Never", 1))))
    });
    let error = block_on(MiniExcel::save_as_with_schema_async_with_cancellation(
        &missing,
        &schema(),
        rows,
        &WriteOptions::new(),
        cancellation,
    ))
    .unwrap_err();
    assert!(error.is_cancelled());
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert!(!missing.exists());

    let cancellation = CancellationToken::new();
    let cancel_after_first = cancellation.clone();
    let rows = stream::iter([Ok(row("First", 1)), Ok(row("Second", 2))]).inspect(move |_| {
        cancel_after_first.cancel();
    });
    let error = block_on(MiniExcel::save_as_with_schema_async_with_cancellation(
        &missing,
        &schema(),
        rows,
        &WriteOptions::new(),
        cancellation,
    ))
    .unwrap_err();
    assert!(error.is_cancelled());
    assert!(!missing.exists());
    assert_no_temporary_files(directory.path());
}

#[test]
fn dropping_pending_export_cancels_worker_and_cleans_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dropped.xlsx");
    let schema = schema();
    let options = WriteOptions::new();
    let mut future =
        Box::pin(MiniExcel::save_as_with_schema_async(&path, &schema, stream::pending(), &options));
    let waker = noop_waker_ref();
    let mut context = Context::from_waker(waker);
    assert!(matches!(Future::poll(Pin::as_mut(&mut future), &mut context), Poll::Pending));
    drop(future);

    for _ in 0..10_000 {
        if !has_temporary_files(directory.path()) {
            break;
        }
        std::thread::yield_now();
    }
    assert!(!path.exists());
    assert_no_temporary_files(directory.path());
}

fn schema() -> Vec<String> {
    vec!["Name".to_owned(), "Value".to_owned()]
}

fn row(name: &str, value: i64) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Value".to_owned(), CellValue::Int(value));
    row
}

fn has_temporary_files(directory: &std::path::Path) -> bool {
    std::fs::read_dir(directory)
        .unwrap()
        .any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".miniexcel-"))
}

fn assert_no_temporary_files(directory: &std::path::Path) {
    assert!(!has_temporary_files(directory));
}
