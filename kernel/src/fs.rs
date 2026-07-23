#![allow(clippy::module_name_repetitions)]

//! Fixed-capacity in-memory filesystem for the first interactive environment.

use core::str;

pub const MAX_FILES: usize = 8;
const MAX_NAME_LEN: usize = 24;
const MAX_FILE_BYTES: usize = 512;

/// Filesystem operation failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FsError {
    EmptyName,
    NameTooLong,
    DataTooLarge,
    FileTableFull,
    NotFound,
}

#[derive(Clone, Copy)]
struct FileEntry {
    occupied: bool,
    name: [u8; MAX_NAME_LEN],
    name_len: usize,
    data: [u8; MAX_FILE_BYTES],
    data_len: usize,
}

impl FileEntry {
    const fn empty() -> Self {
        Self {
            occupied: false,
            name: [0; MAX_NAME_LEN],
            name_len: 0,
            data: [0; MAX_FILE_BYTES],
            data_len: 0,
        }
    }

    fn name(&self) -> &str {
        str::from_utf8(&self.name[..self.name_len]).unwrap_or("<invalid>")
    }

    fn data(&self) -> &[u8] {
        &self.data[..self.data_len]
    }
}

/// Small RAM-backed filesystem with deterministic storage limits.
pub struct RamFs {
    files: [FileEntry; MAX_FILES],
    file_count: usize,
}

impl RamFs {
    /// Creates an empty RAM filesystem.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            files: [FileEntry::empty(); MAX_FILES],
            file_count: 0,
        }
    }

    /// Creates the standard files used by the first shell environment.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut fs = Self::new();
        let _ = fs.write(
            "welcome.txt",
            concat!(
                "Welcome to SanjuOS. The kernel shell, scheduler, timer, ",
                "keyboard pipeline, and RAM filesystem are active.\n"
            )
            .as_bytes(),
        );
        let _ = fs.write(
            "system.txt",
            b"SanjuOS M5: protected userspace, syscalls, ELF loading, and branded startup.\n",
        );
        fs
    }

    /// Creates or overwrites a file.
    ///
    /// # Errors
    ///
    /// Returns an [`FsError`] when the name or data exceeds the fixed limits,
    /// the name is empty, or the file table has no free slot.
    pub fn write(&mut self, name: &str, data: &[u8]) -> Result<(), FsError> {
        validate(name, data)?;

        let index = if let Some(index) = self.find_index(name) {
            index
        } else {
            let Some(index) = self.files.iter().position(|entry| !entry.occupied) else {
                return Err(FsError::FileTableFull);
            };
            self.file_count += 1;
            index
        };

        let entry = &mut self.files[index];
        entry.occupied = true;
        entry.name.fill(0);
        entry.name[..name.len()].copy_from_slice(name.as_bytes());
        entry.name_len = name.len();
        entry.data.fill(0);
        entry.data[..data.len()].copy_from_slice(data);
        entry.data_len = data.len();
        Ok(())
    }

    /// Returns immutable file contents.
    ///
    /// # Errors
    ///
    /// Returns [`FsError::NotFound`] when no file has the requested name.
    pub fn read(&self, name: &str) -> Result<&[u8], FsError> {
        self.find_index(name)
            .map(|index| self.files[index].data())
            .ok_or(FsError::NotFound)
    }

    /// Calls `visitor` for each stored filename.
    pub fn visit_names(&self, mut visitor: impl FnMut(&str)) {
        for entry in self.files.iter().filter(|entry| entry.occupied) {
            visitor(entry.name());
        }
    }

    /// Returns the number of active files.
    #[must_use]
    pub const fn file_count(&self) -> usize {
        self.file_count
    }

    fn find_index(&self, name: &str) -> Option<usize> {
        self.files
            .iter()
            .position(|entry| entry.occupied && entry.name() == name)
    }
}

impl Default for RamFs {
    fn default() -> Self {
        Self::new()
    }
}

fn validate(name: &str, data: &[u8]) -> Result<(), FsError> {
    if name.is_empty() {
        return Err(FsError::EmptyName);
    }
    if name.len() > MAX_NAME_LEN {
        return Err(FsError::NameTooLong);
    }
    if data.len() > MAX_FILE_BYTES {
        return Err(FsError::DataTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{FsError, RamFs};

    #[test]
    fn ramfs_creates_reads_and_overwrites_files() {
        let mut fs = RamFs::new();
        fs.write("note.txt", b"one").unwrap();
        assert_eq!(fs.read("note.txt").unwrap(), b"one");
        fs.write("note.txt", b"two").unwrap();
        assert_eq!(fs.read("note.txt").unwrap(), b"two");
        assert_eq!(fs.file_count(), 1);
    }

    #[test]
    fn ramfs_reports_missing_files() {
        let fs = RamFs::new();
        assert_eq!(fs.read("missing"), Err(FsError::NotFound));
    }
}
