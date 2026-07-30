#![allow(clippy::module_name_repetitions)]

//! Bounded, read-only FAT32 filesystem.
//!
//! The implementation validates the on-disk geometry before trusting offsets,
//! never allocates, rejects malformed cluster chains, and exposes persistent
//! files only through the VFS contracts.

use core::cell::RefCell;
use core::char::decode_utf16;
use core::str;

use crate::block::{BlockDevice, BlockError, SECTOR_SIZE};
use crate::vfs::{FileSystem, Inode, InodeId, MAX_COMPONENT_BYTES, NodeKind, Superblock, VfsError};

const FAT32_MIN_CLUSTERS: u32 = 65_525;
const FAT32_MAX_DATA_CLUSTER: u32 = 0x0fff_ffef;
const FAT32_ENTRY_MASK: u32 = 0x0fff_ffff;
const FAT32_BAD_CLUSTER: u32 = 0x0fff_fff7;
const FAT32_END_OF_CHAIN: u32 = 0x0fff_fff8;
const DIRECTORY_ENTRY_BYTES: usize = 32;
const DIRECTORY_ENTRIES_PER_SECTOR: usize = SECTOR_SIZE / DIRECTORY_ENTRY_BYTES;
const ATTRIBUTE_DIRECTORY: u8 = 0x10;
const ATTRIBUTE_VOLUME_ID: u8 = 0x08;
const ATTRIBUTE_LONG_NAME: u8 = 0x0f;
const ROOT_INODE: InodeId = InodeId(1);
const ENTRY_INODE_FLAG: u64 = 1_u64 << 63;
const MAX_LFN_UTF16_UNITS: usize = 65;

/// Validated FAT32 geometry retained by the mounted backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fat32MountInfo {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub sectors_per_fat: u32,
    pub total_sectors: u32,
    pub cluster_count: u32,
    pub root_cluster: u32,
    pub volume_id: u32,
    pub fs_info_valid: bool,
    pub backup_boot_valid: bool,
}

/// FAT32 mount and traversal failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fat32Error {
    Device(BlockError),
    InvalidBootSignature,
    UnsupportedSectorSize,
    InvalidSectorsPerCluster,
    InvalidReservedSectors,
    InvalidFatCount,
    InvalidFatSize,
    InvalidTotalSectors,
    NotFat32,
    InvalidRootCluster,
    InvalidFsInfo,
    InvalidBackupBoot,
    InvalidCluster,
    CorruptFat,
    ClusterLoop,
    DirectoryCorrupt,
    InvalidInode,
    NotFound,
    NotDirectory,
    IsDirectory,
    ReadOnly,
}

impl From<BlockError> for Fat32Error {
    fn from(value: BlockError) -> Self {
        Self::Device(value)
    }
}

#[derive(Clone, Copy)]
struct Fat32Layout {
    info: Fat32MountInfo,
    active_fat_sector: u32,
    first_data_sector: u32,
    max_cluster: u32,
}

#[derive(Clone, Copy)]
struct DirectoryRecord {
    inode: InodeId,
    first_cluster: u32,
    size: u32,
    kind: NodeKind,
    name: [u8; MAX_COMPONENT_BYTES],
    name_len: usize,
}

impl DirectoryRecord {
    fn name(&self) -> &str {
        str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    const fn inode(self) -> Inode {
        Inode {
            id: self.inode,
            kind: self.kind,
            size: self.size as u64,
        }
    }
}

#[derive(Clone, Copy)]
struct LongNameState {
    units: [u16; MAX_LFN_UTF16_UNITS],
    expected_ordinal: u8,
    checksum: u8,
    active: bool,
}

impl LongNameState {
    const fn new() -> Self {
        Self {
            units: [0xffff; MAX_LFN_UTF16_UNITS],
            expected_ordinal: 0,
            checksum: 0,
            active: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn consume(&mut self, entry: &[u8]) {
        let ordinal_raw = entry[0];
        let ordinal = ordinal_raw & 0x1f;
        let last = ordinal_raw & 0x40 != 0;
        if ordinal == 0 || entry[11] != ATTRIBUTE_LONG_NAME || entry[12] != 0 {
            self.reset();
            return;
        }
        if read_u16(entry, 26) != 0 {
            self.reset();
            return;
        }
        let start = usize::from(ordinal.saturating_sub(1)) * 13;
        if start >= self.units.len() || start.saturating_add(13) > self.units.len() {
            self.reset();
            return;
        }
        if last {
            self.units.fill(0xffff);
            self.expected_ordinal = ordinal;
            self.checksum = entry[13];
            self.active = true;
        }
        if !self.active || ordinal != self.expected_ordinal || entry[13] != self.checksum {
            self.reset();
            return;
        }

        const OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        for (index, offset) in OFFSETS.into_iter().enumerate() {
            self.units[start + index] = read_u16(entry, offset);
        }
        self.expected_ordinal = self.expected_ordinal.saturating_sub(1);
    }

    fn decode(&self, short_name: &[u8; 11]) -> Option<([u8; MAX_COMPONENT_BYTES], usize)> {
        if !self.active
            || self.expected_ordinal != 0
            || self.checksum != short_name_checksum(short_name)
        {
            return None;
        }

        let unit_len = self
            .units
            .iter()
            .position(|unit| *unit == 0 || *unit == 0xffff)
            .unwrap_or(self.units.len());
        let mut output = [0_u8; MAX_COMPONENT_BYTES];
        let mut output_len: usize = 0;
        for decoded in decode_utf16(self.units[..unit_len].iter().copied()) {
            let character = decoded.ok()?;
            if character == '\0' || character == '/' || character.is_control() {
                return None;
            }
            let mut encoded = [0_u8; 4];
            let text = character.encode_utf8(&mut encoded);
            let end = output_len.checked_add(text.len())?;
            if end > output.len() {
                return None;
            }
            output[output_len..end].copy_from_slice(text.as_bytes());
            output_len = end;
        }
        (output_len != 0).then_some((output, output_len))
    }
}

/// Mounted, read-only FAT32 backend.
pub struct Fat32<D: BlockDevice> {
    device: RefCell<D>,
    layout: Fat32Layout,
}

impl<D: BlockDevice> Fat32<D> {
    /// Validates and mounts a FAT32 volume from sector zero.
    ///
    /// # Errors
    ///
    /// Rejects unsupported geometry, invalid signatures, non-FAT32 cluster
    /// counts, out-of-device ranges, and inconsistent backup metadata.
    pub fn mount(mut device: D) -> Result<Self, Fat32Error> {
        let geometry = device.geometry();
        if geometry.sector_size != SECTOR_SIZE as u32 {
            return Err(Fat32Error::UnsupportedSectorSize);
        }

        let mut boot = [0_u8; SECTOR_SIZE];
        device.read_sector(0, &mut boot)?;
        if boot[510] != 0x55 || boot[511] != 0xaa {
            return Err(Fat32Error::InvalidBootSignature);
        }

        let bytes_per_sector = read_u16(&boot, 11);
        if bytes_per_sector != SECTOR_SIZE as u16 {
            return Err(Fat32Error::UnsupportedSectorSize);
        }
        let sectors_per_cluster = boot[13];
        if sectors_per_cluster == 0
            || !sectors_per_cluster.is_power_of_two()
            || sectors_per_cluster > 128
        {
            return Err(Fat32Error::InvalidSectorsPerCluster);
        }
        let reserved_sectors = read_u16(&boot, 14);
        if reserved_sectors == 0 {
            return Err(Fat32Error::InvalidReservedSectors);
        }
        let fat_count = boot[16];
        if !(1..=2).contains(&fat_count) {
            return Err(Fat32Error::InvalidFatCount);
        }
        if read_u16(&boot, 17) != 0 || read_u16(&boot, 22) != 0 {
            return Err(Fat32Error::NotFat32);
        }
        let total_sectors = read_u32(&boot, 32);
        if read_u16(&boot, 19) != 0
            || total_sectors == 0
            || u64::from(total_sectors) > geometry.sectors
        {
            return Err(Fat32Error::InvalidTotalSectors);
        }
        let sectors_per_fat = read_u32(&boot, 36);
        if sectors_per_fat == 0 {
            return Err(Fat32Error::InvalidFatSize);
        }

        let fat_region = u32::from(fat_count)
            .checked_mul(sectors_per_fat)
            .ok_or(Fat32Error::InvalidFatSize)?;
        let first_data_sector = u32::from(reserved_sectors)
            .checked_add(fat_region)
            .ok_or(Fat32Error::InvalidTotalSectors)?;
        if first_data_sector >= total_sectors {
            return Err(Fat32Error::InvalidTotalSectors);
        }
        let data_sectors = total_sectors - first_data_sector;
        let cluster_count = data_sectors / u32::from(sectors_per_cluster);
        if !(FAT32_MIN_CLUSTERS..=FAT32_MAX_DATA_CLUSTER - 1).contains(&cluster_count) {
            return Err(Fat32Error::NotFat32);
        }
        let max_cluster = cluster_count
            .checked_add(1)
            .ok_or(Fat32Error::InvalidTotalSectors)?;
        let required_fat_bytes = u64::from(cluster_count + 2)
            .checked_mul(4)
            .ok_or(Fat32Error::InvalidFatSize)?;
        if required_fat_bytes
            > u64::from(sectors_per_fat)
                .checked_mul(SECTOR_SIZE as u64)
                .ok_or(Fat32Error::InvalidFatSize)?
        {
            return Err(Fat32Error::InvalidFatSize);
        }

        let root_cluster = read_u32(&boot, 44);
        if root_cluster < 2 || root_cluster > max_cluster {
            return Err(Fat32Error::InvalidRootCluster);
        }
        let flags = read_u16(&boot, 40);
        let active_fat = if flags & 0x0080 == 0 {
            0
        } else {
            u32::from(flags & 0x000f)
        };
        if active_fat >= u32::from(fat_count) {
            return Err(Fat32Error::InvalidFatCount);
        }
        let active_fat_sector = u32::from(reserved_sectors)
            .checked_add(
                active_fat
                    .checked_mul(sectors_per_fat)
                    .ok_or(Fat32Error::InvalidFatSize)?,
            )
            .ok_or(Fat32Error::InvalidFatSize)?;

        let fs_info_sector = read_u16(&boot, 48);
        if fs_info_sector == 0 || fs_info_sector >= reserved_sectors {
            return Err(Fat32Error::InvalidFsInfo);
        }
        let mut fs_info = [0_u8; SECTOR_SIZE];
        device.read_sector(u64::from(fs_info_sector), &mut fs_info)?;
        let fs_info_valid = read_u32(&fs_info, 0) == 0x4161_5252
            && read_u32(&fs_info, 484) == 0x6141_7272
            && read_u32(&fs_info, 508) == 0xaa55_0000;
        if !fs_info_valid {
            return Err(Fat32Error::InvalidFsInfo);
        }

        let backup_boot_sector = read_u16(&boot, 50);
        if backup_boot_sector == 0 || backup_boot_sector >= reserved_sectors {
            return Err(Fat32Error::InvalidBackupBoot);
        }
        let mut backup_boot = [0_u8; SECTOR_SIZE];
        device.read_sector(u64::from(backup_boot_sector), &mut backup_boot)?;
        let backup_boot_valid = backup_boot == boot;
        if !backup_boot_valid {
            return Err(Fat32Error::InvalidBackupBoot);
        }

        let info = Fat32MountInfo {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            fat_count,
            sectors_per_fat,
            total_sectors,
            cluster_count,
            root_cluster,
            volume_id: read_u32(&boot, 67),
            fs_info_valid,
            backup_boot_valid,
        };
        Ok(Self {
            device: RefCell::new(device),
            layout: Fat32Layout {
                info,
                active_fat_sector,
                first_data_sector,
                max_cluster,
            },
        })
    }

    /// Returns the validated mount geometry.
    #[must_use]
    pub const fn mount_info(&self) -> Fat32MountInfo {
        self.layout.info
    }

    /// Inspects the wrapped read-only device without transferring ownership.
    pub fn inspect_device<T>(&self, inspector: impl FnOnce(&D) -> T) -> T {
        let device = self.device.borrow();
        inspector(&device)
    }

    fn read_sector(
        &self,
        sector: u32,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), Fat32Error> {
        if sector >= self.layout.info.total_sectors {
            return Err(Fat32Error::InvalidCluster);
        }
        self.device
            .borrow_mut()
            .read_sector(u64::from(sector), destination)
            .map_err(Fat32Error::Device)
    }

    fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, Fat32Error> {
        self.validate_cluster(cluster)?;
        let fat_offset = cluster.checked_mul(4).ok_or(Fat32Error::CorruptFat)?;
        let fat_sector = self
            .layout
            .active_fat_sector
            .checked_add(fat_offset / SECTOR_SIZE as u32)
            .ok_or(Fat32Error::CorruptFat)?;
        let offset =
            usize::try_from(fat_offset % SECTOR_SIZE as u32).map_err(|_| Fat32Error::CorruptFat)?;
        let mut sector = [0_u8; SECTOR_SIZE];
        self.read_sector(fat_sector, &mut sector)?;
        let value = read_u32(&sector, offset) & FAT32_ENTRY_MASK;
        match value {
            FAT32_END_OF_CHAIN..=FAT32_ENTRY_MASK => Ok(None),
            FAT32_BAD_CLUSTER | 0 | 1 => Err(Fat32Error::CorruptFat),
            2..=FAT32_MAX_DATA_CLUSTER => {
                self.validate_cluster(value)?;
                Ok(Some(value))
            }
            _ => Err(Fat32Error::CorruptFat),
        }
    }

    fn validate_cluster(&self, cluster: u32) -> Result<(), Fat32Error> {
        if cluster < 2 || cluster > self.layout.max_cluster {
            return Err(Fat32Error::InvalidCluster);
        }
        Ok(())
    }

    fn cluster_sector(&self, cluster: u32, sector_in_cluster: u32) -> Result<u32, Fat32Error> {
        self.validate_cluster(cluster)?;
        if sector_in_cluster >= u32::from(self.layout.info.sectors_per_cluster) {
            return Err(Fat32Error::InvalidCluster);
        }
        self.layout
            .first_data_sector
            .checked_add(
                (cluster - 2)
                    .checked_mul(u32::from(self.layout.info.sectors_per_cluster))
                    .ok_or(Fat32Error::InvalidCluster)?,
            )
            .and_then(|sector| sector.checked_add(sector_in_cluster))
            .filter(|sector| *sector < self.layout.info.total_sectors)
            .ok_or(Fat32Error::InvalidCluster)
    }

    fn cluster_at(&self, first: u32, index: u64) -> Result<u32, Fat32Error> {
        self.validate_cluster(first)?;
        if index >= u64::from(self.layout.info.cluster_count) {
            return Err(Fat32Error::InvalidCluster);
        }
        let mut current = first;
        let mut tortoise = first;
        let mut hare = Some(first);
        for _ in 0..index {
            current = self.next_cluster(current)?.ok_or(Fat32Error::CorruptFat)?;
            tortoise = self.next_cluster(tortoise)?.ok_or(Fat32Error::CorruptFat)?;
            hare = self.advance_twice(hare)?;
            if hare == Some(tortoise) {
                return Err(Fat32Error::ClusterLoop);
            }
        }
        Ok(current)
    }

    fn advance_twice(&self, cluster: Option<u32>) -> Result<Option<u32>, Fat32Error> {
        let Some(first) = cluster else {
            return Ok(None);
        };
        let Some(second) = self.next_cluster(first)? else {
            return Ok(None);
        };
        self.next_cluster(second)
    }

    fn entry_from_inode(&self, inode: InodeId) -> Result<DirectoryRecord, Fat32Error> {
        let (sector, slot) = decode_entry_inode(inode)?;
        if sector >= self.layout.info.total_sectors || slot >= DIRECTORY_ENTRIES_PER_SECTOR {
            return Err(Fat32Error::InvalidInode);
        }
        let mut data = [0_u8; SECTOR_SIZE];
        self.read_sector(sector, &mut data)?;
        let start = slot * DIRECTORY_ENTRY_BYTES;
        let entry = &data[start..start + DIRECTORY_ENTRY_BYTES];
        if entry[0] == 0 || entry[0] == 0xe5 || entry[11] == ATTRIBUTE_LONG_NAME {
            return Err(Fat32Error::InvalidInode);
        }
        self.short_record(sector, slot, entry, &LongNameState::new())
    }

    fn directory_cluster(&self, inode: InodeId) -> Result<u32, Fat32Error> {
        if inode == ROOT_INODE {
            return Ok(self.layout.info.root_cluster);
        }
        let record = self.entry_from_inode(inode)?;
        if record.kind != NodeKind::Directory {
            return Err(Fat32Error::NotDirectory);
        }
        self.validate_cluster(record.first_cluster)?;
        Ok(record.first_cluster)
    }

    fn scan_directory(
        &self,
        inode: InodeId,
        visitor: &mut dyn FnMut(DirectoryRecord) -> bool,
    ) -> Result<(), Fat32Error> {
        let first = self.directory_cluster(inode)?;
        let mut current = first;
        let mut hare = Some(first);
        let mut long_name = LongNameState::new();

        for _ in 0..self.layout.info.cluster_count {
            for sector_in_cluster in 0..u32::from(self.layout.info.sectors_per_cluster) {
                let sector_number = self.cluster_sector(current, sector_in_cluster)?;
                let mut sector = [0_u8; SECTOR_SIZE];
                self.read_sector(sector_number, &mut sector)?;
                for slot in 0..DIRECTORY_ENTRIES_PER_SECTOR {
                    let start = slot * DIRECTORY_ENTRY_BYTES;
                    let entry = &sector[start..start + DIRECTORY_ENTRY_BYTES];
                    match entry[0] {
                        0 => return Ok(()),
                        0xe5 => {
                            long_name.reset();
                            continue;
                        }
                        _ => {}
                    }
                    if entry[11] == ATTRIBUTE_LONG_NAME {
                        long_name.consume(entry);
                        continue;
                    }
                    if entry[11] & ATTRIBUTE_VOLUME_ID != 0 {
                        long_name.reset();
                        continue;
                    }

                    let record = self.short_record(sector_number, slot, entry, &long_name)?;
                    long_name.reset();
                    if record.name() == "." || record.name() == ".." {
                        continue;
                    }
                    if !visitor(record) {
                        return Ok(());
                    }
                }
            }

            let Some(next) = self.next_cluster(current)? else {
                return Ok(());
            };
            current = next;
            hare = self.advance_twice(hare)?;
            if hare == Some(current) {
                return Err(Fat32Error::ClusterLoop);
            }
        }
        Err(Fat32Error::ClusterLoop)
    }

    fn short_record(
        &self,
        sector: u32,
        slot: usize,
        entry: &[u8],
        long_name: &LongNameState,
    ) -> Result<DirectoryRecord, Fat32Error> {
        let short: [u8; 11] = entry[..11]
            .try_into()
            .map_err(|_| Fat32Error::DirectoryCorrupt)?;
        let (name, name_len) = long_name
            .decode(&short)
            .or_else(|| decode_short_name(&short, entry[12]))
            .ok_or(Fat32Error::DirectoryCorrupt)?;
        let first_cluster = (u32::from(read_u16(entry, 20)) << 16) | u32::from(read_u16(entry, 26));
        let size = read_u32(entry, 28);
        let kind = if entry[11] & ATTRIBUTE_DIRECTORY != 0 {
            NodeKind::Directory
        } else {
            NodeKind::File
        };
        let name_text = str::from_utf8(&name[..name_len]).unwrap_or("");
        let root_relative_dot =
            kind == NodeKind::Directory && first_cluster == 0 && name_text == "..";
        if !root_relative_dot && (kind == NodeKind::Directory || size != 0 || first_cluster != 0) {
            self.validate_cluster(first_cluster)?;
        }
        Ok(DirectoryRecord {
            inode: encode_entry_inode(sector, slot)?,
            first_cluster,
            size,
            kind,
            name,
            name_len,
        })
    }

    fn read_file(
        &self,
        record: DirectoryRecord,
        offset: u64,
        destination: &mut [u8],
    ) -> Result<usize, Fat32Error> {
        if record.kind == NodeKind::Directory {
            return Err(Fat32Error::IsDirectory);
        }
        if offset >= u64::from(record.size) || destination.is_empty() {
            return Ok(0);
        }
        let available = u64::from(record.size) - offset;
        let requested = usize::try_from(available)
            .unwrap_or(usize::MAX)
            .min(destination.len());
        if requested == 0 {
            return Ok(0);
        }
        self.validate_cluster(record.first_cluster)?;

        let cluster_bytes = u64::from(self.layout.info.sectors_per_cluster) * SECTOR_SIZE as u64;
        let starting_cluster_index = offset / cluster_bytes;
        let mut current_cluster = self.cluster_at(record.first_cluster, starting_cluster_index)?;
        let mut hare = Some(current_cluster);
        let mut within_cluster = offset % cluster_bytes;
        let mut copied = 0;
        while copied < requested {
            let sector_in_cluster = u32::try_from(within_cluster / SECTOR_SIZE as u64)
                .map_err(|_| Fat32Error::InvalidCluster)?;
            let sector_offset = usize::try_from(within_cluster % SECTOR_SIZE as u64)
                .map_err(|_| Fat32Error::InvalidCluster)?;
            let sector_number = self.cluster_sector(current_cluster, sector_in_cluster)?;
            let mut sector = [0_u8; SECTOR_SIZE];
            self.read_sector(sector_number, &mut sector)?;
            let chunk = (SECTOR_SIZE - sector_offset).min(requested - copied);
            destination[copied..copied + chunk]
                .copy_from_slice(&sector[sector_offset..sector_offset + chunk]);
            copied += chunk;
            within_cluster = within_cluster
                .checked_add(u64::try_from(chunk).map_err(|_| Fat32Error::InvalidCluster)?)
                .ok_or(Fat32Error::InvalidCluster)?;

            if within_cluster == cluster_bytes && copied < requested {
                current_cluster = self
                    .next_cluster(current_cluster)?
                    .ok_or(Fat32Error::CorruptFat)?;
                hare = self.advance_twice(hare)?;
                if hare == Some(current_cluster) {
                    return Err(Fat32Error::ClusterLoop);
                }
                within_cluster = 0;
            }
        }

        if offset.checked_add(u64::try_from(copied).map_err(|_| Fat32Error::InvalidCluster)?)
            == Some(u64::from(record.size))
            && self.next_cluster(current_cluster)?.is_some()
        {
            return Err(Fat32Error::CorruptFat);
        }
        Ok(copied)
    }
}

impl<D: BlockDevice> FileSystem for Fat32<D> {
    fn superblock(&self) -> Superblock {
        Superblock {
            filesystem_name: "fat32",
            root_inode: ROOT_INODE,
            block_size: u32::from(self.layout.info.sectors_per_cluster) * SECTOR_SIZE as u32,
            read_only: true,
        }
    }

    fn lookup(&self, parent: InodeId, name: &str) -> Result<Inode, VfsError> {
        let mut found = None;
        self.scan_directory(parent, &mut |record| {
            if record.name().eq_ignore_ascii_case(name) {
                found = Some(record.inode());
                false
            } else {
                true
            }
        })
        .map_err(fat_vfs_error)?;
        found.ok_or(VfsError::NotFound)
    }

    fn read(&self, inode: InodeId, offset: u64, destination: &mut [u8]) -> Result<usize, VfsError> {
        let record = self.entry_from_inode(inode).map_err(fat_vfs_error)?;
        self.read_file(record, offset, destination)
            .map_err(fat_vfs_error)
    }

    fn create_or_replace(
        &mut self,
        _parent: InodeId,
        _name: &str,
        _data: &[u8],
    ) -> Result<Inode, VfsError> {
        Err(VfsError::ReadOnly)
    }

    fn visit_directory(
        &self,
        inode: InodeId,
        visitor: &mut dyn FnMut(&str, Inode),
    ) -> Result<(), VfsError> {
        self.scan_directory(inode, &mut |record| {
            visitor(record.name(), record.inode());
            true
        })
        .map_err(fat_vfs_error)
    }
}

const fn fat_vfs_error(error: Fat32Error) -> VfsError {
    match error {
        Fat32Error::NotFound => VfsError::NotFound,
        Fat32Error::NotDirectory => VfsError::NotDirectory,
        Fat32Error::IsDirectory => VfsError::IsDirectory,
        Fat32Error::ReadOnly => VfsError::ReadOnly,
        _ => VfsError::Backend,
    }
}

fn encode_entry_inode(sector: u32, slot: usize) -> Result<InodeId, Fat32Error> {
    if slot >= DIRECTORY_ENTRIES_PER_SECTOR {
        return Err(Fat32Error::InvalidInode);
    }
    Ok(InodeId(
        ENTRY_INODE_FLAG | (u64::from(sector) << 4) | slot as u64,
    ))
}

fn decode_entry_inode(inode: InodeId) -> Result<(u32, usize), Fat32Error> {
    if inode.0 & ENTRY_INODE_FLAG == 0 || inode == ROOT_INODE {
        return Err(Fat32Error::InvalidInode);
    }
    let raw_sector = (inode.0 & !ENTRY_INODE_FLAG) >> 4;
    let sector = u32::try_from(raw_sector).map_err(|_| Fat32Error::InvalidInode)?;
    let slot = usize::try_from(inode.0 & 0x0f).map_err(|_| Fat32Error::InvalidInode)?;
    Ok((sector, slot))
}

fn decode_short_name(
    short: &[u8; 11],
    case_flags: u8,
) -> Option<([u8; MAX_COMPONENT_BYTES], usize)> {
    let base_end = short[..8]
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |index| index + 1);
    if base_end == 0 {
        return None;
    }
    let extension_end = short[8..]
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |index| index + 1);
    let mut output = [0_u8; MAX_COMPONENT_BYTES];
    let mut output_len = 0;
    for (index, byte) in short[..base_end].iter().copied().enumerate() {
        let byte = if index == 0 && byte == 0x05 {
            0xe5
        } else {
            byte
        };
        if !byte.is_ascii() || byte.is_ascii_control() || byte == b'/' {
            return None;
        }
        output[output_len] = if case_flags & 0x08 != 0 {
            byte.to_ascii_lowercase()
        } else {
            byte
        };
        output_len += 1;
    }
    if extension_end != 0 {
        output[output_len] = b'.';
        output_len += 1;
        for byte in short[8..8 + extension_end].iter().copied() {
            if !byte.is_ascii() || byte.is_ascii_control() || byte == b'/' {
                return None;
            }
            output[output_len] = if case_flags & 0x10 != 0 {
                byte.to_ascii_lowercase()
            } else {
                byte
            };
            output_len += 1;
        }
    }
    Some((output, output_len))
}

fn short_name_checksum(short: &[u8; 11]) -> u8 {
    short.iter().fold(0_u8, |sum, byte| {
        ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(*byte)
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    bytes
        .get(offset..offset + 2)
        .and_then(|value| value.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{DIRECTORY_ENTRY_BYTES, Fat32, Fat32Error, short_name_checksum};
    use crate::block::{BlockDevice, BlockError, BlockGeometry, SECTOR_SIZE};
    use crate::fs::RamFs;
    use crate::vfs::{FileSystem, HandleRights, Vfs, VfsError};
    use std::vec::Vec;

    const TOTAL_SECTORS: u32 = 70_000;
    const RESERVED: u16 = 32;
    const FAT_SECTORS: u32 = 544;
    const DATA_START: u32 = RESERVED as u32 + FAT_SECTORS;

    struct SparseDevice {
        sectors: Vec<(u64, [u8; SECTOR_SIZE])>,
        geometry: BlockGeometry,
    }

    impl SparseDevice {
        fn valid() -> Self {
            let mut device = Self {
                sectors: Vec::new(),
                geometry: BlockGeometry::new(u64::from(TOTAL_SECTORS), true),
            };
            let boot = boot_sector();
            device.set(0, boot);
            device.set(6, boot);
            device.set(1, fs_info_sector());

            let mut fat = [0_u8; SECTOR_SIZE];
            set_u32(&mut fat, 0, 0x0fff_fff8);
            set_u32(&mut fat, 4, 0xffff_ffff);
            set_u32(&mut fat, 8, 0x0fff_ffff);
            set_u32(&mut fat, 12, 4);
            set_u32(&mut fat, 16, 0x0fff_ffff);
            set_u32(&mut fat, 20, 0x0fff_ffff);
            device.set(u64::from(RESERVED), fat);

            let mut root = [0_u8; SECTOR_SIZE];
            write_short_entry(&mut root, 0, *b"README  TXT", 0x20, 3, 700);
            write_lfn_entry(&mut root, 1, 0x41, *b"GETTIN~1TXT", "Getting.txt");
            write_short_entry(&mut root, 2, *b"GETTIN~1TXT", 0x20, 5, 15);
            root[3 * DIRECTORY_ENTRY_BYTES] = 0;
            device.set(u64::from(DATA_START), root);

            let mut first = [b'A'; SECTOR_SIZE];
            first[..17].copy_from_slice(b"Persistent FAT32\n");
            device.set(u64::from(DATA_START + 1), first);
            device.set(u64::from(DATA_START + 2), [b'B'; SECTOR_SIZE]);

            let mut long_file = [0_u8; SECTOR_SIZE];
            long_file[..15].copy_from_slice(b"Long name works");
            device.set(u64::from(DATA_START + 3), long_file);
            device
        }

        fn set(&mut self, sector: u64, data: [u8; SECTOR_SIZE]) {
            if let Some((_, existing)) = self
                .sectors
                .iter_mut()
                .find(|(existing_sector, _)| *existing_sector == sector)
            {
                *existing = data;
            } else {
                self.sectors.push((sector, data));
            }
        }
    }

    impl BlockDevice for SparseDevice {
        fn geometry(&self) -> BlockGeometry {
            self.geometry
        }

        fn read_sector(
            &mut self,
            sector: u64,
            destination: &mut [u8; SECTOR_SIZE],
        ) -> Result<(), BlockError> {
            if sector >= self.geometry.sectors {
                return Err(BlockError::OutOfBounds);
            }
            destination.fill(0);
            if let Some((_, data)) = self
                .sectors
                .iter()
                .find(|(existing_sector, _)| *existing_sector == sector)
            {
                destination.copy_from_slice(data);
            }
            Ok(())
        }

        fn write_sector(
            &mut self,
            _sector: u64,
            _source: &[u8; SECTOR_SIZE],
        ) -> Result<(), BlockError> {
            Err(BlockError::ReadOnly)
        }
    }

    #[test]
    fn valid_volume_mounts_and_reads_multicluster_file() {
        let fat = Fat32::mount(SparseDevice::valid()).unwrap();
        assert!(fat.mount_info().fs_info_valid);
        let inode = fat.lookup(super::ROOT_INODE, "readme.txt").unwrap();
        let mut data = [0_u8; 700];
        assert_eq!(fat.read(inode.id, 0, &mut data).unwrap(), data.len());
        assert!(data.starts_with(b"Persistent FAT32\n"));
        assert!(data[512..].iter().all(|byte| *byte == b'B'));
    }

    #[test]
    fn long_names_and_secondary_vfs_dispatch_are_active() {
        let fat = Fat32::mount(SparseDevice::valid()).unwrap();
        let mut vfs = Vfs::new(RamFs::with_defaults())
            .mount("/disk", fat)
            .unwrap();
        let handle = vfs
            .open("/disk/Getting.txt", HandleRights::ReadOnly)
            .unwrap();
        let mut data = [0_u8; 32];
        let read = vfs.read(handle, &mut data).unwrap();
        assert_eq!(&data[..read], b"Long name works");
        vfs.close(handle).unwrap();
        assert_eq!(
            vfs.create_or_replace("/disk/new.txt", b"blocked"),
            Err(VfsError::ReadOnly)
        );
    }

    #[test]
    fn invalid_metadata_and_cluster_loops_are_rejected() {
        let mut invalid = SparseDevice::valid();
        let mut boot = boot_sector();
        boot[510] = 0;
        invalid.set(0, boot);
        assert!(matches!(
            Fat32::mount(invalid),
            Err(Fat32Error::InvalidBootSignature)
        ));

        let mut invalid_backup = SparseDevice::valid();
        let mut backup = boot_sector();
        backup[100] = 1;
        invalid_backup.set(6, backup);
        assert!(matches!(
            Fat32::mount(invalid_backup),
            Err(Fat32Error::InvalidBackupBoot)
        ));

        let mut looped = SparseDevice::valid();
        let mut fat = [0_u8; SECTOR_SIZE];
        set_u32(&mut fat, 0, 0x0fff_fff8);
        set_u32(&mut fat, 4, 0xffff_ffff);
        set_u32(&mut fat, 8, 0x0fff_ffff);
        set_u32(&mut fat, 12, 4);
        set_u32(&mut fat, 16, 3);
        set_u32(&mut fat, 20, 0x0fff_ffff);
        looped.set(u64::from(RESERVED), fat);
        let mounted = Fat32::mount(looped).unwrap();
        let inode = mounted.lookup(super::ROOT_INODE, "README.TXT").unwrap();
        let mut data = [0_u8; 700];
        assert_eq!(mounted.read(inode.id, 0, &mut data), Err(VfsError::Backend));

        let record = mounted.entry_from_inode(inode.id).unwrap();
        assert_eq!(
            mounted.cluster_at(record.first_cluster, 2),
            Err(Fat32Error::ClusterLoop)
        );
    }

    #[test]
    fn offset_reads_cross_cluster_boundaries_without_rewalking_the_chain() {
        let mounted = Fat32::mount(SparseDevice::valid()).unwrap();
        let inode = mounted.lookup(super::ROOT_INODE, "README.TXT").unwrap();
        let mut data = [0_u8; 200];
        assert_eq!(mounted.read(inode.id, 500, &mut data), Ok(data.len()));
        assert!(data[..12].iter().all(|byte| *byte == b'A'));
        assert!(data[12..].iter().all(|byte| *byte == b'B'));
    }

    fn boot_sector() -> [u8; SECTOR_SIZE] {
        let mut boot = [0_u8; SECTOR_SIZE];
        boot[0..3].copy_from_slice(&[0xeb, 0x58, 0x90]);
        boot[3..11].copy_from_slice(b"SOMAOS  ");
        set_u16(&mut boot, 11, SECTOR_SIZE as u16);
        boot[13] = 1;
        set_u16(&mut boot, 14, RESERVED);
        boot[16] = 1;
        set_u16(&mut boot, 17, 0);
        boot[21] = 0xf8;
        set_u16(&mut boot, 22, 0);
        set_u32(&mut boot, 32, TOTAL_SECTORS);
        set_u32(&mut boot, 36, FAT_SECTORS);
        set_u32(&mut boot, 44, 2);
        set_u16(&mut boot, 48, 1);
        set_u16(&mut boot, 50, 6);
        set_u32(&mut boot, 67, 0x534f_4d41);
        boot[82..90].copy_from_slice(b"FAT32   ");
        boot[510] = 0x55;
        boot[511] = 0xaa;
        boot
    }

    fn fs_info_sector() -> [u8; SECTOR_SIZE] {
        let mut sector = [0_u8; SECTOR_SIZE];
        set_u32(&mut sector, 0, 0x4161_5252);
        set_u32(&mut sector, 484, 0x6141_7272);
        set_u32(&mut sector, 488, 0xffff_ffff);
        set_u32(&mut sector, 492, 6);
        set_u32(&mut sector, 508, 0xaa55_0000);
        sector
    }

    fn write_short_entry(
        sector: &mut [u8; SECTOR_SIZE],
        slot: usize,
        name: [u8; 11],
        attributes: u8,
        cluster: u32,
        size: u32,
    ) {
        let start = slot * DIRECTORY_ENTRY_BYTES;
        sector[start..start + 11].copy_from_slice(&name);
        sector[start + 11] = attributes;
        set_u16(sector, start + 20, (cluster >> 16) as u16);
        set_u16(sector, start + 26, cluster as u16);
        set_u32(sector, start + 28, size);
    }

    fn write_lfn_entry(
        sector: &mut [u8; SECTOR_SIZE],
        slot: usize,
        ordinal: u8,
        short: [u8; 11],
        name: &str,
    ) {
        let start = slot * DIRECTORY_ENTRY_BYTES;
        let entry = &mut sector[start..start + DIRECTORY_ENTRY_BYTES];
        entry.fill(0xff);
        entry[0] = ordinal;
        entry[11] = super::ATTRIBUTE_LONG_NAME;
        entry[12] = 0;
        entry[13] = short_name_checksum(&short);
        entry[26] = 0;
        entry[27] = 0;
        const OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        let mut units = name.encode_utf16();
        let mut ended = false;
        for offset in OFFSETS {
            let value = if ended {
                0xffff
            } else if let Some(unit) = units.next() {
                unit
            } else {
                ended = true;
                0
            };
            entry[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
