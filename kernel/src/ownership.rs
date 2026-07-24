use crate::boot_info::{BootInfoV1, PhysicalRange};
use crate::memory::PAGE_SIZE;

pub const MAX_OWNERSHIP_RANGES: usize = 64;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnershipKind {
    KernelImage = 1,
    BootImage = 2,
    BootInfo = 3,
    MemoryMap = 4,
    ActivePageTable = 5,
    Framebuffer = 6,
    Acpi = 7,
    Smbios = 8,
    Initrd = 9,
    PageTableBootstrap = 10,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnershipError {
    InvalidRange,
    RangeOverflow,
    Overlap,
    CapacityExceeded,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedPhysicalRange {
    pub range: PhysicalRange,
    pub kind: OwnershipKind,
    pub reserved: [u8; 7],
}

impl OwnedPhysicalRange {
    const EMPTY: Self = Self {
        range: PhysicalRange::EMPTY,
        kind: OwnershipKind::KernelImage,
        reserved: [0; 7],
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalOwnershipMap {
    ranges: [OwnedPhysicalRange; MAX_OWNERSHIP_RANGES],
    count: usize,
}

impl PhysicalOwnershipMap {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ranges: [OwnedPhysicalRange::EMPTY; MAX_OWNERSHIP_RANGES],
            count: 0,
        }
    }

    pub fn from_boot_info(boot_info: &BootInfoV1) -> Result<Self, OwnershipError> {
        let mut map = Self::new();
        map.reserve_if_uncovered(boot_info.kernel_image, OwnershipKind::KernelImage)?;
        map.reserve_if_uncovered(boot_info.boot_image, OwnershipKind::BootImage)?;
        map.reserve_if_uncovered(boot_info.boot_info_range, OwnershipKind::BootInfo)?;
        map.reserve_if_uncovered(
            PhysicalRange {
                start: boot_info.memory_map.buffer_address,
                length: boot_info.memory_map.map_size,
            },
            OwnershipKind::MemoryMap,
        )?;
        if boot_info.active_page_table_root != 0 {
            map.reserve_if_uncovered(
                PhysicalRange {
                    start: boot_info.active_page_table_root,
                    length: PAGE_SIZE,
                },
                OwnershipKind::ActivePageTable,
            )?;
        }
        map.reserve_if_uncovered(
            boot_info.framebuffer.physical_range(),
            OwnershipKind::Framebuffer,
        )?;
        if boot_info.acpi_rsdp.is_present() {
            map.reserve_if_uncovered(
                page_containing(boot_info.acpi_rsdp.address),
                OwnershipKind::Acpi,
            )?;
        }
        if boot_info.smbios_entry.is_present() {
            map.reserve_if_uncovered(
                page_containing(boot_info.smbios_entry.address),
                OwnershipKind::Smbios,
            )?;
        }
        map.reserve_if_uncovered(boot_info.initrd, OwnershipKind::Initrd)?;
        Ok(map)
    }

    pub fn reserve(
        &mut self,
        range: PhysicalRange,
        kind: OwnershipKind,
    ) -> Result<(), OwnershipError> {
        if range.is_empty() {
            return Ok(());
        }
        let Some(end) = range.end_exclusive() else {
            return Err(OwnershipError::RangeOverflow);
        };
        if end <= range.start {
            return Err(OwnershipError::InvalidRange);
        }
        if self.overlaps(range) {
            return Err(OwnershipError::Overlap);
        }
        if self.count == self.ranges.len() {
            return Err(OwnershipError::CapacityExceeded);
        }

        self.ranges[self.count] = OwnedPhysicalRange {
            range,
            kind,
            reserved: [0; 7],
        };
        self.count += 1;
        Ok(())
    }

    pub fn reserve_if_uncovered(
        &mut self,
        range: PhysicalRange,
        kind: OwnershipKind,
    ) -> Result<(), OwnershipError> {
        if range.is_empty() || self.contains_range(range) {
            return Ok(());
        }
        self.reserve(range, kind)
    }

    #[must_use]
    pub fn contains_address(&self, address: u64) -> bool {
        self.entries().iter().any(|entry| {
            entry
                .range
                .end_exclusive()
                .is_some_and(|end| address >= entry.range.start && address < end)
        })
    }

    #[must_use]
    pub fn contains_range(&self, range: PhysicalRange) -> bool {
        if range.is_empty() {
            return true;
        }
        let Some(end) = range.end_exclusive() else {
            return false;
        };
        self.entries().iter().any(|entry| {
            entry
                .range
                .end_exclusive()
                .is_some_and(|entry_end| range.start >= entry.range.start && end <= entry_end)
        })
    }

    #[must_use]
    pub fn overlaps(&self, range: PhysicalRange) -> bool {
        let Some(end) = range.end_exclusive() else {
            return true;
        };
        self.entries().iter().any(|entry| {
            entry
                .range
                .end_exclusive()
                .is_none_or(|entry_end| range.start < entry_end && entry.range.start < end)
        })
    }

    #[must_use]
    pub fn entries(&self) -> &[OwnedPhysicalRange] {
        &self.ranges[..self.count]
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for PhysicalOwnershipMap {
    fn default() -> Self {
        Self::new()
    }
}

fn page_containing(address: u64) -> PhysicalRange {
    PhysicalRange {
        start: address - (address % PAGE_SIZE),
        length: PAGE_SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::{OwnershipError, OwnershipKind, PAGE_SIZE, PhysicalOwnershipMap};
    use crate::boot_info::{BootInfo, FramebufferInfo, MemoryMapInfo, PhysicalRange, PixelFormat};

    fn sample_boot_info() -> BootInfo {
        let memory_map = MemoryMapInfo {
            buffer_address: 0x18_0000,
            buffer_capacity: 0x20_000,
            map_size: 4_000,
            map_key: 1,
            descriptor_size: 40,
            descriptor_version: 1,
            reserved: 0,
            descriptor_count: 100,
        };
        BootInfo::new("x86_64", "UEFI", "test", memory_map).unwrap()
    }

    #[test]
    fn detects_overlapping_ranges() {
        let mut map = PhysicalOwnershipMap::new();
        map.reserve(
            PhysicalRange {
                start: 0x10_0000,
                length: 0x20_000,
            },
            OwnershipKind::KernelImage,
        )
        .unwrap();

        assert_eq!(
            map.reserve(
                PhysicalRange {
                    start: 0x11_0000,
                    length: 0x10_000,
                },
                OwnershipKind::BootInfo,
            ),
            Err(OwnershipError::Overlap)
        );
    }

    #[test]
    fn covered_ranges_do_not_create_duplicate_reservations() {
        let mut map = PhysicalOwnershipMap::new();
        let image = PhysicalRange {
            start: 0x20_0000,
            length: 0x40_000,
        };
        map.reserve(image, OwnershipKind::KernelImage).unwrap();
        map.reserve_if_uncovered(
            PhysicalRange {
                start: 0x21_0000,
                length: 0x1_000,
            },
            OwnershipKind::BootInfo,
        )
        .unwrap();
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn preserves_kernel_image() {
        let mut boot_info = sample_boot_info();
        boot_info.kernel_image = PhysicalRange {
            start: 0x20_0000,
            length: 0x40_000,
        };
        let map = PhysicalOwnershipMap::from_boot_info(&boot_info).unwrap();
        assert!(map.contains_range(boot_info.kernel_image));
    }

    #[test]
    fn preserves_active_page_tables() {
        let mut boot_info = sample_boot_info();
        boot_info.active_page_table_root = 0x30_0000;
        let map = PhysicalOwnershipMap::from_boot_info(&boot_info).unwrap();
        assert!(map.contains_range(PhysicalRange {
            start: 0x30_0000,
            length: PAGE_SIZE,
        }));
    }

    #[test]
    fn preserves_framebuffer() {
        let mut boot_info = sample_boot_info();
        boot_info.framebuffer = FramebufferInfo {
            present: 1,
            reserved: [0; 7],
            physical_start: 0x8000_0000,
            byte_length: 0x80_0000,
            width: 1920,
            height: 1080,
            stride: 1920,
            pixel_format: PixelFormat::Bgr,
            red_mask: 0x00ff_0000,
            green_mask: 0x0000_ff00,
            blue_mask: 0x0000_00ff,
            reserved_mask: 0xff00_0000,
        };
        let map = PhysicalOwnershipMap::from_boot_info(&boot_info).unwrap();
        assert!(map.contains_range(boot_info.framebuffer.physical_range()));
    }
}
