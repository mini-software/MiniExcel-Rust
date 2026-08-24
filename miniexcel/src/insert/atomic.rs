use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Seek, Write};
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use super::donor::DonorWorksheet;
use super::package::PackageInventory;
use super::rewrite::{PackageRewriteStage, append_worksheet_to_writer_with_hook};
use crate::{Error, Result, SheetVisibility};

const WORKSHEET_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AtomicCommitStage {
    Preflight,
    RowGeneration,
    ZipCopy,
    ZipFinish,
    Validation,
    Commit,
}

pub(crate) fn append_to_path<F>(path: impl AsRef<Path>, donor_builder: F) -> Result<usize>
where
    F: FnOnce() -> Result<DonorWorksheet>,
{
    append_to_path_with_hook(path.as_ref(), donor_builder, |_| Ok(()))
}

fn append_to_path_with_hook<F, H>(path: &Path, donor_builder: F, mut checkpoint: H) -> Result<usize>
where
    F: FnOnce() -> Result<DonorWorksheet>,
    H: FnMut(AtomicCommitStage) -> Result<()>,
{
    checkpoint(AtomicCommitStage::Preflight)?;
    let source_metadata = fs::metadata(path)?;
    let mut source = File::open(path)?;
    PackageInventory::inspect(&mut source)?;

    checkpoint(AtomicCommitStage::RowGeneration)?;
    let donor = donor_builder()?;
    let row_count = donor.data_row_count;

    let parent = sibling_directory(path);
    let mut temporary =
        tempfile::Builder::new().prefix(".miniexcel-").suffix(".xlsx.tmp").tempfile_in(parent)?;
    source.rewind()?;
    append_worksheet_to_writer_with_hook(source, temporary.as_file_mut(), &donor, |stage| {
        match stage {
            PackageRewriteStage::Copy => checkpoint(AtomicCommitStage::ZipCopy),
            PackageRewriteStage::Finish => checkpoint(AtomicCommitStage::ZipFinish),
        }
    })?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;

    checkpoint(AtomicCommitStage::Validation)?;
    validate_rewritten_package(temporary.reopen()?, &donor.sheet_name)?;

    checkpoint(AtomicCommitStage::Commit)?;
    replace_temporary(temporary, path, source_metadata.permissions())?;
    Ok(row_count)
}

fn sibling_directory(path: &Path) -> &Path {
    path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."))
}

fn validate_rewritten_package(mut file: File, sheet_name: &str) -> Result<()> {
    let inventory = PackageInventory::inspect(&mut file)?;
    if inventory.find_sheet(sheet_name).is_none() {
        return Err(Error::insert_package(format!(
            "rewritten worksheet '{sheet_name}' is missing"
        )));
    }
    if !inventory.sheets.iter().any(|sheet| sheet.visibility == SheetVisibility::Visible) {
        return Err(Error::no_visible_worksheets());
    }

    let mut override_paths = BTreeSet::new();
    for content_type in &inventory.content_types.overrides {
        if !override_paths.insert(content_type.part_name.as_str()) {
            return Err(Error::unsafe_package(format!(
                "content-type override '{}' is duplicated",
                content_type.part_name
            )));
        }
        if !inventory.entry_names.contains(&content_type.part_name) {
            return Err(Error::insert_package(format!(
                "content-type override '{}' has no package part",
                content_type.part_name
            )));
        }
    }
    let default_extensions = inventory
        .content_types
        .defaults
        .iter()
        .map(|entry| entry.extension.to_lowercase())
        .collect::<BTreeSet<_>>();
    for part_name in &inventory.entry_names {
        if part_name == "[Content_Types].xml" || part_name.ends_with('/') {
            continue;
        }
        let has_override = override_paths.contains(part_name.as_str());
        let has_default = part_name
            .rsplit_once('.')
            .is_some_and(|(_, extension)| default_extensions.contains(&extension.to_lowercase()));
        if !has_override && !has_default {
            return Err(Error::insert_package(format!(
                "package part '{part_name}' has no content type"
            )));
        }
    }
    for worksheet in &inventory.sheets {
        let content_type = inventory
            .content_types
            .overrides
            .iter()
            .find(|entry| entry.part_name == worksheet.target)
            .ok_or_else(|| {
                Error::insert_package(format!(
                    "worksheet '{}' has no content-type override",
                    worksheet.target
                ))
            })?;
        if content_type.content_type != WORKSHEET_CONTENT_TYPE {
            return Err(Error::insert_package(format!(
                "worksheet '{}' has incompatible content type '{}'",
                worksheet.target, content_type.content_type
            )));
        }
    }
    for relationship in &inventory.relationships {
        if let Some(source) = relationship.source.as_deref() {
            if !inventory.entry_names.contains(source) {
                return Err(Error::insert_package(format!(
                    "relationship source '{source}' is missing"
                )));
            }
        }
        if let Some(target) = relationship.normalized_target.as_deref() {
            if !inventory.entry_names.contains(target) {
                return Err(Error::insert_package(format!(
                    "relationship target '{target}' is missing"
                )));
            }
        }
    }
    for relationship_part in inventory.entry_names.iter().filter(|name| name.ends_with(".rels")) {
        if let Some(source) = relationship_part_source(relationship_part)? {
            if !inventory.entry_names.contains(&source) {
                return Err(Error::insert_package(format!(
                    "relationship part '{relationship_part}' has missing source '{source}'"
                )));
            }
        }
    }

    file.rewind()?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        Error::insert_package(format!("cannot reopen rewritten XLSX ZIP: {error}"))
    })?;
    if archive.has_overlapping_files().map_err(|error| {
        Error::unsafe_package(format!("cannot inspect overlapping ZIP entries: {error}"))
    })? {
        return Err(Error::unsafe_package("rewritten ZIP contains overlapping entries"));
    }
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            Error::insert_package(format!("cannot validate rewritten ZIP entry: {error}"))
        })?;
        std::io::copy(&mut entry, &mut std::io::sink())?;
    }
    Ok(())
}

fn relationship_part_source(path: &str) -> Result<Option<String>> {
    if path == "_rels/.rels" {
        return Ok(None);
    }
    let (prefix, name) = path
        .rsplit_once("_rels/")
        .ok_or_else(|| Error::insert_package(format!("invalid relationship part path '{path}'")))?;
    let name = name.strip_suffix(".rels").ok_or_else(|| {
        Error::insert_package(format!("invalid relationship part suffix '{path}'"))
    })?;
    Ok(Some(format!("{prefix}{name}")))
}

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn replace_temporary(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    source_permissions: fs::Permissions,
) -> Result<()> {
    let (staged_file, staged_path) = temporary.keep().map_err(|error| {
        Error::atomic_commit(format!(
            "cannot prepare temporary file '{}': {}",
            error.file.path().display(),
            error.error
        ))
    })?;
    let mut cleanup = StagedFileCleanup::new(staged_path);
    fs::set_permissions(cleanup.path(), source_permissions)?;
    staged_file.sync_all()?;

    let original_permissions = fs::metadata(path)?.permissions();
    if original_permissions.readonly() {
        let mut writable_permissions = original_permissions.clone();
        writable_permissions.set_readonly(false);
        fs::set_permissions(path, writable_permissions)?;
    }
    if let Err(error) = atomicwrites::replace_atomic(cleanup.path(), path) {
        let restore = fs::set_permissions(path, original_permissions);
        let message = match restore {
            Ok(()) => format!(
                "cannot replace '{}' with '{}': {error}",
                path.display(),
                cleanup.path().display()
            ),
            Err(restore_error) => format!(
                "cannot replace '{}' with '{}': {error}; cannot restore permissions: {restore_error}",
                path.display(),
                cleanup.path().display()
            ),
        };
        return Err(Error::atomic_commit(message));
    }
    cleanup.disarm();
    Ok(())
}

#[cfg(not(windows))]
fn replace_temporary(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    source_permissions: fs::Permissions,
) -> Result<()> {
    fs::set_permissions(temporary.path(), source_permissions)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| {
        Error::atomic_commit(format!(
            "cannot replace '{}' with '{}': {}",
            path.display(),
            error.file.path().display(),
            error.error
        ))
    })?;
    Ok(())
}

#[cfg(windows)]
struct StagedFileCleanup {
    path: PathBuf,
    active: bool,
}

#[cfg(windows)]
impl StagedFileCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

#[cfg(windows)]
impl Drop for StagedFileCleanup {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::{Cursor, Read};
    use std::rc::Rc;

    use sha2::{Digest, Sha256};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    use super::*;
    use crate::insert::donor::{DonorBuilder, build_from_dynamic_iter};
    use crate::writer::XlsxWriter;
    use crate::{CellValue, DynamicRow, WriteOptions};

    #[test]
    fn atomic_append_replaces_only_after_validation_and_preserves_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");
        fs::write(&path, source_package()).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();

        let count = append_to_path(&path, || donor("Inserted", 2)).unwrap();

        assert_eq!(count, 2);
        assert_eq!(fs::metadata(&path).unwrap().permissions().readonly(), permissions.readonly());
        let inventory = PackageInventory::inspect(File::open(&path).unwrap()).unwrap();
        assert_eq!(
            inventory.sheets.iter().map(|sheet| sheet.name.as_str()).collect::<Vec<_>>(),
            ["Data", "Inserted"]
        );
        assert_no_temporary_files(directory.path());
    }

    #[cfg(windows)]
    #[test]
    #[allow(clippy::permissions_set_readonly_false)]
    fn atomic_append_preserves_windows_readonly_attribute() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");
        fs::write(&path, source_package()).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();

        append_to_path(&path, || donor("Inserted", 1)).unwrap();

        assert!(fs::metadata(&path).unwrap().permissions().readonly());
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_append_preserves_unix_permission_bits() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");
        fs::write(&path, source_package()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        append_to_path(&path, || donor("Inserted", 1)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn every_precommit_failure_keeps_source_hash_and_cleans_temporary_package() {
        for failure in [
            AtomicCommitStage::Preflight,
            AtomicCommitStage::RowGeneration,
            AtomicCommitStage::ZipCopy,
            AtomicCommitStage::ZipFinish,
            AtomicCommitStage::Validation,
            AtomicCommitStage::Commit,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("book.xlsx");
            fs::write(&path, source_package()).unwrap();
            let before = file_hash(&path);
            let donor_calls = Rc::new(Cell::new(0));
            let calls = Rc::clone(&donor_calls);
            let observed_directory = directory.path().to_owned();
            let result = append_to_path_with_hook(
                &path,
                || {
                    calls.set(calls.get() + 1);
                    donor("Inserted", 1)
                },
                |stage| {
                    if stage == AtomicCommitStage::ZipCopy {
                        assert!(fs::read_dir(&observed_directory).unwrap().any(|entry| {
                            entry.unwrap().file_name().to_string_lossy().starts_with(".miniexcel-")
                        }));
                    }
                    if stage == failure {
                        Err(Error::atomic_commit(format!("injected {stage:?} failure")))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err(), "{failure:?} did not fail");
            assert_eq!(file_hash(&path), before, "{failure:?} changed the source file");
            assert_no_temporary_files(directory.path());
            assert_eq!(
                donor_calls.get(),
                usize::from(!matches!(
                    failure,
                    AtomicCommitStage::Preflight | AtomicCommitStage::RowGeneration
                ))
            );
        }
    }

    #[test]
    fn malformed_source_and_row_generation_errors_leave_no_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");
        fs::write(&path, b"not an xlsx").unwrap();
        let before = file_hash(&path);
        assert!(append_to_path(&path, || donor("Inserted", 1)).is_err());
        assert_eq!(file_hash(&path), before);
        assert_no_temporary_files(directory.path());

        fs::write(&path, source_package()).unwrap();
        let before = file_hash(&path);
        let schema = vec!["Label".to_owned(), "Value".to_owned()];
        let rows =
            [Ok(row("before failure", 1)), Err(Error::insert_package("row producer failed"))];
        assert!(
            append_to_path(&path, || {
                build_from_dynamic_iter(
                    &schema,
                    rows,
                    &WriteOptions::new().with_sheet_name("Inserted"),
                    Some(directory.path()),
                )
            })
            .is_err()
        );
        assert_eq!(file_hash(&path), before);
        assert_no_temporary_files(directory.path());
    }

    #[test]
    fn validation_rejects_missing_inserted_sheet() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");
        fs::write(&path, source_package()).unwrap();
        assert!(validate_rewritten_package(File::open(&path).unwrap(), "Missing").is_err());
    }

    #[test]
    fn validation_rejects_dangling_content_types_and_orphan_relationship_parts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");

        let dangling_override = mutate_package(
            &source_package(),
            Some(r#"<Override PartName="/missing.xml" ContentType="application/xml"/>"#),
            &[],
        );
        fs::write(&path, dangling_override).unwrap();
        assert!(validate_rewritten_package(File::open(&path).unwrap(), "Data").is_err());

        let missing_content_type =
            mutate_package(&source_package(), None, &[("custom/data.mystery", b"data")]);
        fs::write(&path, missing_content_type).unwrap();
        assert!(validate_rewritten_package(File::open(&path).unwrap(), "Data").is_err());

        let orphan_relationship = mutate_package(
            &source_package(),
            None,
            &[(
                "xl/worksheets/_rels/missing.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
            )],
        );
        fs::write(&path, orphan_relationship).unwrap();
        assert!(validate_rewritten_package(File::open(&path).unwrap(), "Data").is_err());
    }

    #[test]
    fn replacement_error_cleans_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let mut temporary =
            tempfile::Builder::new().prefix(".miniexcel-").tempfile_in(directory.path()).unwrap();
        temporary.write_all(b"staged").unwrap();

        let permissions = fs::metadata(temporary.path()).unwrap().permissions();
        assert!(replace_temporary(temporary, &destination, permissions).is_err());
        assert!(!fs::read_dir(directory.path()).unwrap().any(|entry| {
            entry.unwrap().file_name().to_string_lossy().starts_with(".miniexcel-")
        }));
    }

    fn source_package() -> Vec<u8> {
        let mut writer = XlsxWriter::new();
        writer
            .add_rows(&[row("Existing", 1)], &WriteOptions::new().with_sheet_name("Data"))
            .unwrap();
        writer.save_to_bytes().unwrap()
    }

    fn mutate_package(
        package: &[u8],
        content_type_addition: Option<&str>,
        extra_entries: &[(&str, &[u8])],
    ) -> Vec<u8> {
        let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            if entry.name() == "[Content_Types].xml" && content_type_addition.is_some() {
                let options = entry.options();
                let mut xml = String::new();
                entry.read_to_string(&mut xml).unwrap();
                let xml =
                    xml.replace("</Types>", &format!("{}</Types>", content_type_addition.unwrap()));
                writer.start_file("[Content_Types].xml", options).unwrap();
                writer.write_all(xml.as_bytes()).unwrap();
            } else {
                writer.raw_copy_file(entry).unwrap();
            }
        }
        for (name, payload) in extra_entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(payload).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn donor(sheet_name: &str, row_count: usize) -> Result<DonorWorksheet> {
        let rows = (0..row_count).map(|value| row("Inserted", value as i64)).collect::<Vec<_>>();
        DonorBuilder::from_dynamic(&rows, &WriteOptions::new().with_sheet_name(sheet_name))
    }

    fn row(label: &str, value: i64) -> DynamicRow {
        let mut row = DynamicRow::new();
        row.insert("Label".to_owned(), CellValue::String(label.to_owned()));
        row.insert("Value".to_owned(), CellValue::Int(value));
        row
    }

    fn file_hash(path: &Path) -> Vec<u8> {
        Sha256::digest(fs::read(path).unwrap()).to_vec()
    }

    fn assert_no_temporary_files(directory: &Path) {
        let names = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, [std::ffi::OsString::from("book.xlsx")]);
    }
}
