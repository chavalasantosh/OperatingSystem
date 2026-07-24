#![allow(clippy::module_name_repetitions)]

//! SanjuOS syscall ABI and safe user-pointer validation.

use crate::fs::{FsError, RamFs};

pub const SYSCALL_WRITE: u64 = 0;
pub const SYSCALL_READ: u64 = 1;
pub const SYSCALL_EXIT: u64 = 2;
pub const SYSCALL_YIELD: u64 = 3;
pub const SYSCALL_GETPID: u64 = 4;
pub const SYSCALL_OPEN: u64 = 5;
pub const SYSCALL_CLOSE: u64 = 6;
pub const SYSCALL_SPAWN: u64 = 7;

pub const MAX_USER_REGIONS: usize = 16;
pub const MAX_FDS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyscallNumber {
    Write,
    Read,
    Exit,
    Yield,
    GetPid,
    Open,
    Close,
    Spawn,
}

impl TryFrom<u64> for SyscallNumber {
    type Error = SyscallError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            SYSCALL_WRITE => Ok(Self::Write),
            SYSCALL_READ => Ok(Self::Read),
            SYSCALL_EXIT => Ok(Self::Exit),
            SYSCALL_YIELD => Ok(Self::Yield),
            SYSCALL_GETPID => Ok(Self::GetPid),
            SYSCALL_OPEN => Ok(Self::Open),
            SYSCALL_CLOSE => Ok(Self::Close),
            SYSCALL_SPAWN => Ok(Self::Spawn),
            _ => Err(SyscallError::UnknownNumber),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyscallError {
    UnknownNumber,
    InvalidPointer,
    PermissionDenied,
    InvalidUtf8,
    BadFileDescriptor,
    FileNotFound,
    FileTableFull,
    SpawnUnavailable,
}

impl SyscallError {
    #[must_use]
    pub const fn errno(self) -> i64 {
        match self {
            Self::UnknownNumber => -38,
            Self::InvalidPointer => -14,
            Self::PermissionDenied => -13,
            Self::InvalidUtf8 => -84,
            Self::BadFileDescriptor => -9,
            Self::FileNotFound => -2,
            Self::FileTableFull => -24,
            Self::SpawnUnavailable => -11,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserRegion {
    pub start: usize,
    pub length: usize,
    pub readable: bool,
    pub writable: bool,
    occupied: bool,
}

impl UserRegion {
    const fn empty() -> Self {
        Self {
            start: 0,
            length: 0,
            readable: false,
            writable: false,
            occupied: false,
        }
    }

    #[must_use]
    fn contains(self, pointer: usize, length: usize, write: bool) -> bool {
        if !self.occupied || (!self.readable && !write) || (write && !self.writable) {
            return false;
        }
        let Some(region_end) = self.start.checked_add(self.length) else {
            return false;
        };
        let Some(request_end) = pointer.checked_add(length) else {
            return false;
        };
        pointer >= self.start && request_end <= region_end && pointer <= request_end
    }
}

/// Per-process list of readable and writable user mappings.
pub struct UserMemory {
    regions: [UserRegion; MAX_USER_REGIONS],
    region_count: usize,
}

impl UserMemory {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            regions: [UserRegion::empty(); MAX_USER_REGIONS],
            region_count: 0,
        }
    }

    #[must_use]
    pub fn add_region(
        &mut self,
        start: usize,
        length: usize,
        readable: bool,
        writable: bool,
    ) -> bool {
        if length == 0 || start.checked_add(length).is_none() {
            return false;
        }
        let Some(slot) = self.regions.iter_mut().find(|region| !region.occupied) else {
            return false;
        };
        *slot = UserRegion {
            start,
            length,
            readable,
            writable,
            occupied: true,
        };
        self.region_count += 1;
        true
    }

    #[must_use]
    pub const fn region_count(&self) -> usize {
        self.region_count
    }

    #[must_use]
    pub fn validates_read(&self, pointer: usize, length: usize) -> bool {
        length == 0
            || self
                .regions
                .iter()
                .any(|region| region.contains(pointer, length, false))
    }

    #[must_use]
    pub fn validates_write(&self, pointer: usize, length: usize) -> bool {
        length == 0
            || self
                .regions
                .iter()
                .any(|region| region.contains(pointer, length, true))
    }

    /// Copies bytes from a validated user range.
    ///
    /// # Safety
    ///
    /// Registered ranges must describe currently mapped process memory.
    ///
    /// # Errors
    ///
    /// Returns [`SyscallError::InvalidPointer`] when validation fails.
    pub unsafe fn copy_from_user(
        &self,
        pointer: usize,
        length: usize,
    ) -> Result<&[u8], SyscallError> {
        if !self.validates_read(pointer, length) {
            return Err(SyscallError::InvalidPointer);
        }
        if length == 0 {
            return Ok(&[]);
        }
        // SAFETY: The caller guarantees the registered mappings are live and
        // validation above confines the requested range to one readable region.
        Ok(unsafe { core::slice::from_raw_parts(pointer as *const u8, length) })
    }

    /// Copies kernel bytes into a validated user range.
    ///
    /// # Safety
    ///
    /// Registered ranges must describe currently mapped process memory.
    ///
    /// # Errors
    ///
    /// Returns [`SyscallError::InvalidPointer`] when validation fails.
    pub unsafe fn copy_to_user(&self, pointer: usize, source: &[u8]) -> Result<(), SyscallError> {
        if !self.validates_write(pointer, source.len()) {
            return Err(SyscallError::InvalidPointer);
        }
        if source.is_empty() {
            return Ok(());
        }
        // SAFETY: Validation confines the destination to one writable mapping
        // and `copy_from_slice` uses the exact source length.
        unsafe {
            core::slice::from_raw_parts_mut(pointer as *mut u8, source.len())
                .copy_from_slice(source);
        }
        Ok(())
    }
}

impl Default for UserMemory {
    fn default() -> Self {
        Self::new()
    }
}

pub trait SyscallOutput {
    fn write_user_bytes(&mut self, bytes: &[u8]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileDescriptor {
    open: bool,
    file_name: [u8; 24],
    file_name_len: usize,
}

impl FileDescriptor {
    const fn closed() -> Self {
        Self {
            open: false,
            file_name: [0; 24],
            file_name_len: 0,
        }
    }

    fn name(&self) -> &str {
        core::str::from_utf8(&self.file_name[..self.file_name_len]).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyscallAction {
    Return(i64),
    Exit(i32),
    Yield,
    SpawnRequested,
}

pub struct SyscallDispatcher {
    pid: u32,
    descriptors: [FileDescriptor; MAX_FDS],
    calls: u64,
}

impl SyscallDispatcher {
    #[must_use]
    pub const fn new(pid: u32) -> Self {
        let mut descriptors = [FileDescriptor::closed(); MAX_FDS];
        descriptors[0].open = true;
        descriptors[1].open = true;
        descriptors[2].open = true;
        Self {
            pid,
            descriptors,
            calls: 0,
        }
    }

    #[must_use]
    pub const fn calls(&self) -> u64 {
        self.calls
    }

    /// Dispatches one syscall using SanjuOS register arguments.
    ///
    /// # Safety
    ///
    /// `user_memory` must accurately describe the current process mappings.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn dispatch(
        &mut self,
        number: u64,
        arg0: usize,
        arg1: usize,
        _arg2: usize,
        user_memory: &UserMemory,
        fs: &mut RamFs,
        output: &mut dyn SyscallOutput,
    ) -> SyscallAction {
        self.calls = self.calls.saturating_add(1);
        let Ok(number) = SyscallNumber::try_from(number) else {
            return SyscallAction::Return(SyscallError::UnknownNumber.errno());
        };
        match number {
            SyscallNumber::Write => {
                // SAFETY: The dispatcher is called with the current process's
                // validated mapping registry.
                let bytes = match unsafe { user_memory.copy_from_user(arg0, arg1) } {
                    Ok(bytes) => bytes,
                    Err(error) => return SyscallAction::Return(error.errno()),
                };
                output.write_user_bytes(bytes);
                SyscallAction::Return(i64::try_from(bytes.len()).unwrap_or(i64::MAX))
            }
            SyscallNumber::Read => SyscallAction::Return(0),
            SyscallNumber::Exit => SyscallAction::Exit(i32::try_from(arg0).unwrap_or(i32::MAX)),
            SyscallNumber::Yield => SyscallAction::Yield,
            SyscallNumber::GetPid => SyscallAction::Return(i64::from(self.pid)),
            SyscallNumber::Open => {
                // SAFETY: Name bytes are validated against the current process.
                let name_bytes = match unsafe { user_memory.copy_from_user(arg0, arg1) } {
                    Ok(bytes) => bytes,
                    Err(error) => return SyscallAction::Return(error.errno()),
                };
                let Ok(name) = core::str::from_utf8(name_bytes) else {
                    return SyscallAction::Return(SyscallError::InvalidUtf8.errno());
                };
                if fs.read(name).is_err() {
                    return SyscallAction::Return(SyscallError::FileNotFound.errno());
                }
                match self.open_descriptor(name) {
                    Ok(fd) => SyscallAction::Return(i64::try_from(fd).unwrap_or(i64::MAX)),
                    Err(error) => SyscallAction::Return(error.errno()),
                }
            }
            SyscallNumber::Close => match self.close_descriptor(arg0) {
                Ok(()) => SyscallAction::Return(0),
                Err(error) => SyscallAction::Return(error.errno()),
            },
            SyscallNumber::Spawn => SyscallAction::SpawnRequested,
        }
    }

    fn open_descriptor(&mut self, name: &str) -> Result<usize, SyscallError> {
        if name.len() > self.descriptors[0].file_name.len() {
            return Err(SyscallError::InvalidUtf8);
        }
        let Some((index, descriptor)) = self
            .descriptors
            .iter_mut()
            .enumerate()
            .skip(3)
            .find(|(_, descriptor)| !descriptor.open)
        else {
            return Err(SyscallError::FileTableFull);
        };
        descriptor.open = true;
        descriptor.file_name[..name.len()].copy_from_slice(name.as_bytes());
        descriptor.file_name_len = name.len();
        Ok(index)
    }

    fn close_descriptor(&mut self, fd: usize) -> Result<(), SyscallError> {
        if fd < 3 {
            return Err(SyscallError::PermissionDenied);
        }
        let Some(descriptor) = self.descriptors.get_mut(fd) else {
            return Err(SyscallError::BadFileDescriptor);
        };
        if !descriptor.open {
            return Err(SyscallError::BadFileDescriptor);
        }
        *descriptor = FileDescriptor::closed();
        Ok(())
    }

    #[must_use]
    pub fn descriptor_name(&self, fd: usize) -> Option<&str> {
        self.descriptors
            .get(fd)
            .filter(|descriptor| descriptor.open)
            .map(FileDescriptor::name)
    }
}

impl From<FsError> for SyscallError {
    fn from(error: FsError) -> Self {
        match error {
            FsError::NotFound => Self::FileNotFound,
            _ => Self::FileTableFull,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SYSCALL_CLOSE, SYSCALL_GETPID, SYSCALL_OPEN, SYSCALL_WRITE, SyscallAction,
        SyscallDispatcher, SyscallOutput, UserMemory,
    };
    use crate::fs::RamFs;
    use std::vec::Vec;

    #[derive(Default)]
    struct Output(Vec<u8>);

    impl SyscallOutput for Output {
        fn write_user_bytes(&mut self, bytes: &[u8]) {
            self.0.extend_from_slice(bytes);
        }
    }

    #[test]
    fn dispatcher_validates_user_pointers_and_file_descriptors() {
        let message = b"SanjuOS";
        let name = b"welcome.txt";
        let mut memory = UserMemory::new();
        assert!(memory.add_region(message.as_ptr().addr(), message.len(), true, false));
        assert!(memory.add_region(name.as_ptr().addr(), name.len(), true, false));
        let mut fs = RamFs::with_defaults();
        let mut output = Output::default();
        let mut dispatcher = SyscallDispatcher::new(42);

        // SAFETY: The simulated memory and console are valid for the dispatch call.
        let write = unsafe {
            dispatcher.dispatch(
                SYSCALL_WRITE,
                message.as_ptr().addr(),
                message.len(),
                0,
                &memory,
                &mut fs,
                &mut output,
            )
        };
        assert_eq!(write, SyscallAction::Return(7));
        assert_eq!(output.0, message);

        // SAFETY: The arguments and memory state are valid for the test environment.
        let pid =
            unsafe { dispatcher.dispatch(SYSCALL_GETPID, 0, 0, 0, &memory, &mut fs, &mut output) };
        assert_eq!(pid, SyscallAction::Return(42));

        // SAFETY: Dispatching an open syscall is safe in this test.
        let open = unsafe {
            dispatcher.dispatch(
                SYSCALL_OPEN,
                name.as_ptr().addr(),
                name.len(),
                0,
                &memory,
                &mut fs,
                &mut output,
            )
        };
        let SyscallAction::Return(fd) = open else {
            panic!("open did not return a descriptor");
        };
        assert!(fd >= 3);
        let fd = usize::try_from(fd).unwrap();
        assert_eq!(dispatcher.descriptor_name(fd), Some("welcome.txt"));
        // SAFETY: Closing the file descriptor in the test environment is safe.
        let close =
            unsafe { dispatcher.dispatch(SYSCALL_CLOSE, fd, 0, 0, &memory, &mut fs, &mut output) };
        assert_eq!(close, SyscallAction::Return(0));
    }
}
