use std::cell::RefCell;
use std::fs::{File, OpenOptions, remove_file};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Error, Result};

const INDEX_RECORD_SIZE: u64 = 16;
static NEXT_CACHE_ID: AtomicU64 = AtomicU64::new(0);

pub(super) enum SharedStrings {
    Memory(MemorySharedStrings),
    Disk(DiskSharedStrings),
}

impl SharedStrings {
    pub(super) fn memory() -> Self {
        Self::Memory(MemorySharedStrings::default())
    }

    pub(super) fn disk(directory: &Path) -> Result<Self> {
        Ok(Self::Disk(DiskSharedStrings::new(directory)?))
    }

    pub(super) fn push(&mut self, value: String) -> Result<()> {
        match self {
            Self::Memory(strings) => {
                strings.push(value);
                Ok(())
            }
            Self::Disk(strings) => strings.push(&value),
        }
    }

    pub(super) fn reserve(&mut self, entries: usize) {
        if let Self::Memory(strings) = self {
            strings.ends.reserve(entries);
        }
    }

    pub(super) fn get(&self, index: usize) -> Result<Option<String>> {
        match self {
            Self::Memory(strings) => Ok(strings.get(index)),
            Self::Disk(strings) => strings.get(index),
        }
    }
}

#[derive(Default)]
pub(super) struct MemorySharedStrings {
    data: String,
    ends: Vec<usize>,
}

impl MemorySharedStrings {
    fn push(&mut self, value: String) {
        self.data.push_str(&value);
        self.ends.push(self.data.len());
    }

    fn get(&self, index: usize) -> Option<String> {
        let end = *self.ends.get(index)?;
        let start = index.checked_sub(1).map_or(0, |previous| self.ends[previous]);
        Some(self.data[start..end].to_owned())
    }
}

pub(super) struct DiskSharedStrings {
    index: Option<RefCell<File>>,
    data: Option<RefCell<File>>,
    index_path: PathBuf,
    data_path: PathBuf,
    count: usize,
    data_length: u64,
}

impl DiskSharedStrings {
    fn new(directory: &Path) -> Result<Self> {
        if !directory.is_dir() {
            return Err(Error::stream(format!(
                "shared-string cache directory '{}' does not exist",
                directory.display()
            )));
        }

        for _ in 0..100 {
            let id = NEXT_CACHE_ID.fetch_add(1, Ordering::Relaxed);
            let prefix = format!("miniexcel-shared-{}-{id}", std::process::id());
            let index_path = directory.join(format!("{prefix}.index"));
            let data_path = directory.join(format!("{prefix}.data"));
            let index = match create_cache_file(&index_path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            let data = match create_cache_file(&data_path) {
                Ok(file) => file,
                Err(error) => {
                    drop(index);
                    let _ = remove_file(&index_path);
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(error.into());
                }
            };
            return Ok(Self {
                index: Some(RefCell::new(index)),
                data: Some(RefCell::new(data)),
                index_path,
                data_path,
                count: 0,
                data_length: 0,
            });
        }
        Err(Error::stream("cannot create unique shared-string cache files"))
    }

    fn push(&mut self, value: &str) -> Result<()> {
        let bytes = value.as_bytes();
        let length = u64::try_from(bytes.len())
            .map_err(|_| Error::stream("shared string is too large for the disk cache"))?;
        let mut index = self.index.as_ref().expect("cache index is open").borrow_mut();
        index.write_all(&self.data_length.to_le_bytes())?;
        index.write_all(&length.to_le_bytes())?;
        self.data.as_ref().expect("cache data is open").borrow_mut().write_all(bytes)?;
        self.data_length = self.data_length.saturating_add(length);
        self.count += 1;
        Ok(())
    }

    fn get(&self, index: usize) -> Result<Option<String>> {
        if index >= self.count {
            return Ok(None);
        }
        let position = u64::try_from(index)
            .map_err(|_| Error::stream("shared string index is too large"))?
            .saturating_mul(INDEX_RECORD_SIZE);
        let mut record = [0_u8; INDEX_RECORD_SIZE as usize];
        let mut index_file = self.index.as_ref().expect("cache index is open").borrow_mut();
        index_file.seek(SeekFrom::Start(position))?;
        index_file.read_exact(&mut record)?;
        let offset = u64::from_le_bytes(record[..8].try_into().expect("fixed offset bytes"));
        let length = u64::from_le_bytes(record[8..].try_into().expect("fixed length bytes"));
        let length = usize::try_from(length)
            .map_err(|_| Error::stream("shared string is too large to read"))?;
        let mut bytes = vec![0_u8; length];
        let mut data_file = self.data.as_ref().expect("cache data is open").borrow_mut();
        data_file.seek(SeekFrom::Start(offset))?;
        data_file.read_exact(&mut bytes)?;
        String::from_utf8(bytes).map(Some).map_err(|error| {
            Error::stream(format!("shared-string cache contains invalid UTF-8: {error}"))
        })
    }
}

impl Drop for DiskSharedStrings {
    fn drop(&mut self) {
        self.index.take();
        self.data.take();
        let _ = remove_file(&self.index_path);
        let _ = remove_file(&self.data_path);
    }
}

fn create_cache_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).create_new(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::MemorySharedStrings;

    #[test]
    fn memory_shared_strings_preserve_empty_and_unicode_values() {
        let mut strings = MemorySharedStrings::default();
        strings.push("alpha".to_owned());
        strings.push(String::new());
        strings.push("中文".to_owned());

        assert_eq!(strings.get(0).as_deref(), Some("alpha"));
        assert_eq!(strings.get(1).as_deref(), Some(""));
        assert_eq!(strings.get(2).as_deref(), Some("中文"));
        assert_eq!(strings.get(3), None);
    }
}
