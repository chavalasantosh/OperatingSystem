#![allow(clippy::module_name_repetitions)]

//! Allocation-free virtual-filesystem contracts for M6C.
//!
//! The first backend is RAMFS. The same inode, mount, path, directory, and
//! handle contracts form the boundary for the read-only FAT32 backend in M6D.

use core::str;

pub const MAX_PATH_BYTES: usize = 256;
pub const MAX_PATH_COMPONENTS: usize = 16;
pub const MAX_COMPONENT_BYTES: usize = 64;
pub const MAX_MOUNTS: usize = 4;
pub const MAX_USER_HANDLES: usize = 32;

/// Path parsing and normalization failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathError {
    Empty,
    NotAbsolute,
    InvalidCharacter,
    PathTooLong,
    ComponentTooLong,
    TooManyComponents,
}

/// Canonical absolute path stored without allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizedPath {
    bytes: [u8; MAX_PATH_BYTES],
    len: usize,
    components: usize,
}

impl NormalizedPath {
    /// Returns the canonical root path.
    #[must_use]
    pub const fn root() -> Self {
        let mut bytes = [0_u8; MAX_PATH_BYTES];
        bytes[0] = b'/';
        Self {
            bytes,
            len: 1,
            components: 0,
        }
    }

    /// Normalizes an absolute UTF-8 path.
    ///
    /// Repeated separators and `.` are removed. `..` is bounded at the root,
    /// so normalization can never escape the mounted namespace.
    ///
    /// # Errors
    ///
    /// Returns a [`PathError`] for relative, oversized, malformed, or overly
    /// deep paths.
    pub fn parse(raw: &str) -> Result<Self, PathError> {
        if raw.is_empty() {
            return Err(PathError::Empty);
        }
        if !raw.starts_with('/') {
            return Err(PathError::NotAbsolute);
        }
        if raw.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
            return Err(PathError::InvalidCharacter);
        }

        let mut path = Self::root();
        for component in raw[1..].split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                path.pop_component();
                continue;
            }
            if component.len() > MAX_COMPONENT_BYTES {
                return Err(PathError::ComponentTooLong);
            }
            if path.components == MAX_PATH_COMPONENTS {
                return Err(PathError::TooManyComponents);
            }

            let separator = usize::from(path.len > 1);
            let Some(required) = path
                .len
                .checked_add(separator)
                .and_then(|length| length.checked_add(component.len()))
            else {
                return Err(PathError::PathTooLong);
            };
            if required > MAX_PATH_BYTES {
                return Err(PathError::PathTooLong);
            }
            if separator != 0 {
                path.bytes[path.len] = b'/';
                path.len += 1;
            }
            let end = path.len + component.len();
            path.bytes[path.len..end].copy_from_slice(component.as_bytes());
            path.len = end;
            path.components += 1;
        }
        Ok(path)
    }

    /// Returns the canonical path text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        str::from_utf8(&self.bytes[..self.len]).unwrap_or("/")
    }

    /// Returns the number of retained components.
    #[must_use]
    pub const fn component_count(&self) -> usize {
        self.components
    }

    /// Iterates over canonical path components.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.as_str().split('/').filter(|part| !part.is_empty())
    }

    /// Returns the final component, or `None` for root.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.components().last()
    }

    /// Returns the canonical parent path.
    #[must_use]
    pub fn parent(&self) -> Self {
        let mut parent = *self;
        parent.pop_component();
        parent
    }

    fn pop_component(&mut self) {
        if self.components == 0 {
            return;
        }
        while self.len > 1 && self.bytes[self.len - 1] != b'/' {
            self.len -= 1;
        }
        if self.len > 1 {
            self.len -= 1;
        }
        self.bytes[self.len..].fill(0);
        self.components -= 1;
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        if self.len == 1 {
            return true;
        }
        if other.len < self.len || other.bytes[..self.len] != self.bytes[..self.len] {
            return false;
        }
        other.len == self.len || other.bytes[self.len] == b'/'
    }
}

/// Stable identifier for one filesystem inode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InodeId(pub u64);

/// Filesystem object type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    File,
    Directory,
}

/// Backend-independent inode metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Inode {
    pub id: InodeId,
    pub kind: NodeKind,
    pub size: u64,
}

/// Immutable metadata for one mounted filesystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Superblock {
    pub filesystem_name: &'static str,
    pub root_inode: InodeId,
    pub block_size: u32,
    pub read_only: bool,
}

/// Backend-independent filesystem failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VfsError {
    Path(PathError),
    NotFound,
    NotDirectory,
    IsDirectory,
    ReadOnly,
    FileTooLarge,
    NoSpace,
    InvalidOffset,
    MountTableFull,
    DuplicateMount,
    UnsupportedMount,
    HandleTableFull,
    InvalidHandle,
    StaleHandle,
    Backend,
}

impl From<PathError> for VfsError {
    fn from(value: PathError) -> Self {
        Self::Path(value)
    }
}

/// Contract implemented by every VFS backend.
pub trait FileSystem {
    fn superblock(&self) -> Superblock;

    fn lookup(&self, parent: InodeId, name: &str) -> Result<Inode, VfsError>;

    fn read(&self, inode: InodeId, offset: u64, destination: &mut [u8]) -> Result<usize, VfsError>;

    fn create_or_replace(
        &mut self,
        parent: InodeId,
        name: &str,
        data: &[u8],
    ) -> Result<Inode, VfsError>;

    fn visit_directory(
        &self,
        inode: InodeId,
        visitor: &mut dyn FnMut(&str, Inode),
    ) -> Result<(), VfsError>;
}

/// Stable identifier for one mount-table entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MountId(pub u16);

/// One occupied mount-table entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mount {
    pub id: MountId,
    pub path: NormalizedPath,
    pub superblock: Superblock,
}

#[derive(Clone, Copy)]
struct MountSlot {
    occupied: bool,
    mount: Mount,
}

impl MountSlot {
    const fn empty() -> Self {
        Self {
            occupied: false,
            mount: Mount {
                id: MountId(0),
                path: NormalizedPath::root(),
                superblock: Superblock {
                    filesystem_name: "",
                    root_inode: InodeId(0),
                    block_size: 0,
                    read_only: true,
                },
            },
        }
    }
}

/// Fixed-capacity mount table with longest-prefix resolution.
pub struct MountTable {
    slots: [MountSlot; MAX_MOUNTS],
    mount_count: usize,
}

impl MountTable {
    /// Creates a table with one root mount.
    #[must_use]
    pub fn with_root(superblock: Superblock) -> Self {
        let mut table = Self {
            slots: [MountSlot::empty(); MAX_MOUNTS],
            mount_count: 1,
        };
        table.slots[0] = MountSlot {
            occupied: true,
            mount: Mount {
                id: MountId(0),
                path: NormalizedPath::root(),
                superblock,
            },
        };
        table
    }

    /// Adds a mount contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is invalid, already mounted, or the
    /// table is full.
    pub fn mount(&mut self, raw_path: &str, superblock: Superblock) -> Result<MountId, VfsError> {
        let path = NormalizedPath::parse(raw_path)?;
        if self
            .slots
            .iter()
            .any(|slot| slot.occupied && slot.mount.path == path)
        {
            return Err(VfsError::DuplicateMount);
        }
        let Some(index) = self.slots.iter().position(|slot| !slot.occupied) else {
            return Err(VfsError::MountTableFull);
        };
        let id = MountId(u16::try_from(index).map_err(|_| VfsError::MountTableFull)?);
        self.slots[index] = MountSlot {
            occupied: true,
            mount: Mount {
                id,
                path,
                superblock,
            },
        };
        self.mount_count += 1;
        Ok(id)
    }

    /// Resolves a path to its most-specific mount.
    #[must_use]
    pub fn resolve(&self, path: &NormalizedPath) -> Option<Mount> {
        self.slots
            .iter()
            .filter(|slot| slot.occupied && slot.mount.path.is_prefix_of(path))
            .max_by_key(|slot| slot.mount.path.len)
            .map(|slot| slot.mount)
    }

    /// Returns the number of occupied entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.mount_count
    }

    /// Returns whether the table has no mounts.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.mount_count == 0
    }

    /// Visits every occupied mount.
    pub fn visit(&self, mut visitor: impl FnMut(Mount)) {
        for slot in self.slots.iter().filter(|slot| slot.occupied) {
            visitor(slot.mount);
        }
    }
}

/// Rights attached to one open handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandleRights {
    ReadOnly,
    ReadWrite,
}

/// Generation-protected user-visible handle identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileHandleId {
    pub slot: u16,
    pub generation: u16,
}

/// Open-file state retained by the kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileHandle {
    pub inode: InodeId,
    pub mount: MountId,
    pub offset: u64,
    pub rights: HandleRights,
}

#[derive(Clone, Copy)]
struct HandleSlot {
    generation: u16,
    handle: Option<FileHandle>,
}

impl HandleSlot {
    const fn empty() -> Self {
        Self {
            generation: 1,
            handle: None,
        }
    }
}

/// Fixed-capacity process-facing handle table.
pub struct UserHandleTable {
    slots: [HandleSlot; MAX_USER_HANDLES],
    active: usize,
}

impl UserHandleTable {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [HandleSlot::empty(); MAX_USER_HANDLES],
            active: 0,
        }
    }

    /// Allocates one handle slot.
    ///
    /// # Errors
    ///
    /// Returns [`VfsError::HandleTableFull`] when no slot is available.
    pub fn open(&mut self, handle: FileHandle) -> Result<FileHandleId, VfsError> {
        let Some(index) = self.slots.iter().position(|slot| slot.handle.is_none()) else {
            return Err(VfsError::HandleTableFull);
        };
        let slot = &mut self.slots[index];
        slot.handle = Some(handle);
        self.active += 1;
        Ok(FileHandleId {
            slot: u16::try_from(index).map_err(|_| VfsError::HandleTableFull)?,
            generation: slot.generation,
        })
    }

    /// Returns an immutable open handle.
    ///
    /// # Errors
    ///
    /// Rejects out-of-range, closed, and stale identifiers.
    pub fn get(&self, id: FileHandleId) -> Result<&FileHandle, VfsError> {
        let slot = self
            .slots
            .get(usize::from(id.slot))
            .ok_or(VfsError::InvalidHandle)?;
        if slot.generation != id.generation {
            return Err(VfsError::StaleHandle);
        }
        slot.handle.as_ref().ok_or(VfsError::InvalidHandle)
    }

    /// Returns a mutable open handle.
    ///
    /// # Errors
    ///
    /// Rejects out-of-range, closed, and stale identifiers.
    pub fn get_mut(&mut self, id: FileHandleId) -> Result<&mut FileHandle, VfsError> {
        let slot = self
            .slots
            .get_mut(usize::from(id.slot))
            .ok_or(VfsError::InvalidHandle)?;
        if slot.generation != id.generation {
            return Err(VfsError::StaleHandle);
        }
        slot.handle.as_mut().ok_or(VfsError::InvalidHandle)
    }

    /// Closes a handle and advances its generation.
    ///
    /// # Errors
    ///
    /// Rejects out-of-range, already-closed, and stale identifiers.
    pub fn close(&mut self, id: FileHandleId) -> Result<(), VfsError> {
        let slot = self
            .slots
            .get_mut(usize::from(id.slot))
            .ok_or(VfsError::InvalidHandle)?;
        if slot.generation != id.generation {
            return Err(VfsError::StaleHandle);
        }
        if slot.handle.take().is_none() {
            return Err(VfsError::InvalidHandle);
        }
        slot.generation = slot.generation.wrapping_add(1);
        if slot.generation == 0 {
            slot.generation = 1;
        }
        self.active -= 1;
        Ok(())
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.active
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.active == 0
    }

    #[must_use]
    pub const fn capacity(&self) -> usize {
        MAX_USER_HANDLES
    }
}

impl Default for UserHandleTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Root VFS instance for one concrete backend.
pub struct Vfs<F: FileSystem> {
    root: F,
    mounts: MountTable,
    handles: UserHandleTable,
}

impl<F: FileSystem> Vfs<F> {
    #[must_use]
    pub fn new(root: F) -> Self {
        let mounts = MountTable::with_root(root.superblock());
        Self {
            root,
            mounts,
            handles: UserHandleTable::new(),
        }
    }

    /// Resolves one canonical path to inode metadata.
    ///
    /// # Errors
    ///
    /// Returns path, mount, lookup, or type errors from the VFS/backend.
    pub fn resolve(&self, raw_path: &str) -> Result<Inode, VfsError> {
        let path = NormalizedPath::parse(raw_path)?;
        let mount = self
            .mounts
            .resolve(&path)
            .ok_or(VfsError::UnsupportedMount)?;
        if mount.id != MountId(0) {
            return Err(VfsError::UnsupportedMount);
        }

        let mut inode = Inode {
            id: mount.superblock.root_inode,
            kind: NodeKind::Directory,
            size: 0,
        };
        for component in path.components() {
            if inode.kind != NodeKind::Directory {
                return Err(VfsError::NotDirectory);
            }
            inode = self.root.lookup(inode.id, component)?;
        }
        Ok(inode)
    }

    /// Opens one non-directory object.
    ///
    /// # Errors
    ///
    /// Returns resolution, permission, type, or handle-capacity errors.
    pub fn open(&mut self, raw_path: &str, rights: HandleRights) -> Result<FileHandleId, VfsError> {
        let inode = self.resolve(raw_path)?;
        if inode.kind == NodeKind::Directory {
            return Err(VfsError::IsDirectory);
        }
        let superblock = self.root.superblock();
        if rights == HandleRights::ReadWrite && superblock.read_only {
            return Err(VfsError::ReadOnly);
        }
        self.handles.open(FileHandle {
            inode: inode.id,
            mount: MountId(0),
            offset: 0,
            rights,
        })
    }

    /// Reads from the current handle offset and advances it.
    ///
    /// # Errors
    ///
    /// Returns stale-handle, offset, or backend errors.
    pub fn read(&mut self, id: FileHandleId, destination: &mut [u8]) -> Result<usize, VfsError> {
        let handle = *self.handles.get(id)?;
        if handle.mount != MountId(0) {
            return Err(VfsError::UnsupportedMount);
        }
        let read = self.root.read(handle.inode, handle.offset, destination)?;
        let next_offset = handle
            .offset
            .checked_add(u64::try_from(read).map_err(|_| VfsError::InvalidOffset)?)
            .ok_or(VfsError::InvalidOffset)?;
        self.handles.get_mut(id)?.offset = next_offset;
        Ok(read)
    }

    /// Closes one open handle.
    ///
    /// # Errors
    ///
    /// Returns invalid or stale-handle errors.
    pub fn close(&mut self, id: FileHandleId) -> Result<(), VfsError> {
        self.handles.close(id)
    }

    /// Creates or replaces one file through the mounted backend.
    ///
    /// # Errors
    ///
    /// Returns path, mount, permission, or backend errors.
    pub fn create_or_replace(&mut self, raw_path: &str, data: &[u8]) -> Result<Inode, VfsError> {
        let path = NormalizedPath::parse(raw_path)?;
        let name = path.file_name().ok_or(VfsError::IsDirectory)?;
        let parent = self.resolve(path.parent().as_str())?;
        if parent.kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        if self.root.superblock().read_only {
            return Err(VfsError::ReadOnly);
        }
        self.root.create_or_replace(parent.id, name, data)
    }

    /// Visits one directory through its backend.
    ///
    /// # Errors
    ///
    /// Returns path, type, mount, or backend errors.
    pub fn visit_directory(
        &self,
        raw_path: &str,
        visitor: &mut dyn FnMut(&str, Inode),
    ) -> Result<(), VfsError> {
        let inode = self.resolve(raw_path)?;
        if inode.kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        self.root.visit_directory(inode.id, visitor)
    }

    #[must_use]
    pub const fn mounts(&self) -> &MountTable {
        &self.mounts
    }

    #[must_use]
    pub const fn handles(&self) -> &UserHandleTable {
        &self.handles
    }

    #[must_use]
    pub const fn backend(&self) -> &F {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FileHandle, HandleRights, InodeId, MAX_PATH_COMPONENTS, MountId, MountTable,
        NormalizedPath, PathError, Superblock, UserHandleTable, Vfs, VfsError,
    };
    use crate::fs::RamFs;

    #[test]
    fn path_normalization_is_absolute_bounded_and_canonical() {
        let path = NormalizedPath::parse("/docs//./draft/../welcome.txt").unwrap();
        assert_eq!(path.as_str(), "/docs/welcome.txt");
        assert_eq!(path.component_count(), 2);
        assert_eq!(NormalizedPath::parse("/../../").unwrap().as_str(), "/");
        assert_eq!(
            NormalizedPath::parse("relative/path"),
            Err(PathError::NotAbsolute)
        );

        let too_deep = "/x".repeat(MAX_PATH_COMPONENTS + 1);
        assert_eq!(
            NormalizedPath::parse(&too_deep),
            Err(PathError::TooManyComponents)
        );
    }

    #[test]
    fn mount_resolution_uses_longest_component_boundary_prefix() {
        let root = Superblock {
            filesystem_name: "ramfs",
            root_inode: InodeId(1),
            block_size: 1,
            read_only: false,
        };
        let disk = Superblock {
            filesystem_name: "fat32",
            root_inode: InodeId(2),
            block_size: 512,
            read_only: true,
        };
        let mut mounts = MountTable::with_root(root);
        let disk_id = mounts.mount("/disk", disk).unwrap();

        let resolved = mounts
            .resolve(&NormalizedPath::parse("/disk/readme.txt").unwrap())
            .unwrap();
        assert_eq!(resolved.id, disk_id);
        let root_resolved = mounts
            .resolve(&NormalizedPath::parse("/diskette").unwrap())
            .unwrap();
        assert_eq!(root_resolved.id, MountId(0));
        assert_eq!(mounts.mount("/disk/", disk), Err(VfsError::DuplicateMount));
    }

    #[test]
    fn ramfs_resolves_and_reads_through_generation_checked_handle() {
        let mut vfs = Vfs::new(RamFs::with_defaults());
        let handle = vfs.open("/./welcome.txt", HandleRights::ReadOnly).unwrap();
        let mut data = [0_u8; 512];
        let read = vfs.read(handle, &mut data).unwrap();
        assert!(data[..read].starts_with(b"Welcome"));
        vfs.close(handle).unwrap();
        assert_eq!(vfs.read(handle, &mut data), Err(VfsError::StaleHandle));
    }

    #[test]
    fn handle_table_is_fixed_capacity_and_rejects_stale_ids() {
        let mut handles = UserHandleTable::new();
        let handle = FileHandle {
            inode: InodeId(7),
            mount: MountId(0),
            offset: 0,
            rights: HandleRights::ReadOnly,
        };
        let mut ids = [None; super::MAX_USER_HANDLES];
        for id in &mut ids {
            *id = Some(handles.open(handle).unwrap());
        }
        assert_eq!(handles.open(handle), Err(VfsError::HandleTableFull));
        let first = ids[0].unwrap();
        handles.close(first).unwrap();
        assert_eq!(handles.get(first), Err(VfsError::StaleHandle));
        assert!(handles.open(handle).is_ok());
    }
}
