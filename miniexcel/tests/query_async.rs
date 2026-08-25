#![cfg(all(feature = "async", not(target_arch = "wasm32")))]

mod common;

use std::time::{Duration, Instant};

use chrono::NaiveDate;
use futures_executor::block_on;
use futures_util::StreamExt;
use miniexcel::{CancellationToken, CellValue, HeaderMode, MiniExcel, ReadOptions};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct UserAccount {
    #[serde(rename = "ID")]
    id: String,
    name: String,
    #[serde(rename = "BoD", deserialize_with = "miniexcel::serde_helpers::deserialize_date")]
    born_on: NaiveDate,
    age: u32,
    #[serde(rename = "VIP")]
    vip: bool,
    points: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
struct InvalidSequence {
    #[serde(rename = "ID")]
    id: u32,
    name: Option<String>,
    #[serde(rename = "SEQ")]
    sequence: u32,
}

#[test]
fn dynamic_and_typed_async_queries_match_synchronous_results() {
    block_on(async {
        let dynamic_path = common::fixture("TestDynamicQueryBasic_WithoutHead.xlsx");
        let expected = MiniExcel::query(&dynamic_path)
            .unwrap()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
        let actual = MiniExcel::query_async(&dynamic_path)
            .unwrap()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(actual, expected);

        let typed_path = common::fixture("TestTypeMapping.xlsx");
        let expected = MiniExcel::query_as::<UserAccount>(&typed_path)
            .unwrap()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
        let actual = MiniExcel::query_as_async::<UserAccount>(&typed_path)
            .unwrap()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(actual, expected);
    });
}

#[test]
fn typed_async_query_preserves_mapping_error_context() {
    block_on(async {
        let mut rows =
            MiniExcel::query_as_async::<InvalidSequence>(common::fixture("TestIssue309.xlsx"))
                .unwrap();
        assert!(rows.next().await.unwrap().is_ok());
        assert!(rows.next().await.unwrap().is_ok());
        let error = rows.next().await.unwrap().unwrap_err();
        let message = error.to_string();
        assert!(message.contains("Sheet1"));
        assert!(message.contains("row 4"));
    });
}

#[test]
fn cancellation_is_deterministic_before_and_during_iteration() {
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let result = MiniExcel::query_async_with_options_and_cancellation(
        common::fixture("TestTypeMapping.xlsx"),
        &ReadOptions::new(),
        cancelled,
    );
    assert!(matches!(result, Err(error) if error.is_cancelled()));

    block_on(async {
        let cancellation = CancellationToken::new();
        let mut rows = MiniExcel::query_async_with_options_and_cancellation(
            common::fixture("TestTypeMapping.xlsx"),
            &ReadOptions::new().with_header_mode(HeaderMode::FirstRow),
            cancellation.clone(),
        )
        .unwrap();
        let first = rows.next().await.unwrap().unwrap();
        assert_eq!(first["Name"], CellValue::String("Wade".to_owned()));

        cancellation.cancel();
        assert!(rows.next().await.unwrap().unwrap_err().is_cancelled());
        assert!(rows.next().await.is_none());
    });
}

#[test]
fn invalid_paths_are_reported_by_the_stream() {
    block_on(async {
        let mut rows = MiniExcel::query_async("missing-async-query.xlsx").unwrap();
        let error = rows.next().await.unwrap().unwrap_err();
        assert!(error.to_string().contains("I/O error"));
        assert!(rows.next().await.is_none());
    });
}

#[test]
fn dropping_the_async_stream_releases_disk_cached_shared_strings() {
    let cache_dir = tempfile::tempdir().unwrap();
    let options = ReadOptions::new()
        .with_header_mode(HeaderMode::FirstRow)
        .with_shared_string_cache_size(1)
        .with_shared_string_cache_path(cache_dir.path());

    block_on(async {
        let mut rows =
            MiniExcel::query_async_with_options(common::fixture("TestTypeMapping.xlsx"), &options)
                .unwrap();
        assert!(rows.next().await.unwrap().is_ok());
        assert!(cache_dir.path().read_dir().unwrap().next().is_some());
        drop(rows);
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    while cache_dir.path().read_dir().unwrap().next().is_some() && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(cache_dir.path().read_dir().unwrap().next().is_none());
}
