use std::io::Cursor;

use chrono::NaiveDate;
use miniexcel::{CommentTimestamp, MiniExcel};
use uuid::Uuid;

mod common;

#[test]
fn retrieves_threaded_comments_replies_people_and_legacy_notes() {
    let path = common::fixture("TestCommentsAndNotes.xlsx");
    let comments = MiniExcel::get_comments(&path, Some("sheet1")).unwrap();
    assert_eq!(comments.sheet_name(), "Sheet1");
    assert_eq!(comments.threaded_comments().len(), 2);
    assert_eq!(comments.notes().len(), 2);

    let first = &comments.threaded_comments()[0];
    assert_eq!(first.cell().to_string(), "B3");
    assert_eq!(first.id(), uuid("8d44beaf-9259-4d6a-8559-58427a76727b"));
    assert_eq!(first.text(), "this is a comment");
    assert!(!first.resolved());
    assert_eq!(first.replies().len(), 2);
    assert_eq!(first.person().unwrap().display_name(), "John Doe");
    assert_eq!(first.person().unwrap().provider_id(), Some("google-sheets"));
    assert_eq!(first.replies()[0].text(), "this is a reply");
    assert_eq!(first.replies()[0].person().unwrap().display_name(), "Mary Sue");
    assert_eq!(
        first.replies()[0].person().unwrap().user_id(),
        Some("S::m.sue@contoso.com::88790059-46a6-4ed0-bc14-7e167c210d31")
    );
    assert_eq!(
        first.created_at(),
        Some(&CommentTimestamp::Local(
            NaiveDate::from_ymd_opt(2026, 3, 21).unwrap().and_hms_opt(12, 7, 24).unwrap()
        ))
    );

    assert_eq!(comments.notes()[0].cell().to_string(), "D6");
    assert_eq!(comments.notes()[0].author(), None);
    assert_eq!(comments.notes()[0].text(), "this is a simple note");
    assert_eq!(comments.notes()[1].id(), Some(uuid("4e01653b-66e0-48be-9390-2bddb28a7255")));
    assert_eq!(comments.notes()[1].author(), Some("local user"));
    assert_eq!(comments.notes()[1].text(), "local user:\nthis is a note from someone else");
}

#[test]
fn comments_match_for_path_bytes_and_borrowed_reader_across_sheets() {
    let path = common::fixture("TestCommentsAndNotes.xlsx");
    let bytes = std::fs::read(&path).unwrap();
    let path_result = MiniExcel::get_comments(&path, Some("sheet2")).unwrap();
    let byte_result = MiniExcel::get_comments_from_bytes(&bytes, Some("sheet2")).unwrap();
    let mut reader = Cursor::new(bytes);
    let reader_result = MiniExcel::get_comments_from_reader(&mut reader, Some("sheet2")).unwrap();
    assert_eq!(path_result, byte_result);
    assert_eq!(path_result, reader_result);
    assert_eq!(path_result.threaded_comments()[0].cell().to_string(), "A3");
    assert_eq!(path_result.notes()[0].cell().to_string(), "B11");

    let empty = MiniExcel::get_comments(&path, Some("sheet3")).unwrap();
    assert!(empty.threaded_comments().is_empty());
    assert!(empty.notes().is_empty());

    let resolved = MiniExcel::get_comments(&path, Some("sheet4")).unwrap();
    assert!(resolved.threaded_comments()[0].resolved());
    assert_eq!(resolved.threaded_comments()[0].cell().to_string(), "D2");
    assert_eq!(resolved.threaded_comments()[0].replies()[0].text(), "ok");
}

#[test]
fn missing_sheet_returns_a_specific_error() {
    let path = common::fixture("TestCommentsAndNotes.xlsx");
    let error = MiniExcel::get_comments(path, Some("Missing")).unwrap_err();
    assert!(error.to_string().contains("worksheet 'Missing' was not found"));
}

fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).unwrap()
}
