#![allow(clippy::module_name_repetitions)]

//! SanjuOS x86-64 virtual-memory policy and shared paging types.
//!
//! Hardware page-table mutation lives in the architecture layer. This module
//! freezes the address-space contract, validates mapping flags, and keeps the
//! early acceptance model used by host tests and M5 regression checks.

use crate::memory::{PAGE_SIZE, PhysicalFrame};

pub const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
pub const PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;
pub const USER_SPACE_START: u64 = 0x0000_0000_0040_0000;
pub const USER_SPACE_END: u64 = 0x0000_7fff_ffff_f000;
pub const PHYSICAL_DIRECT_MAP_START: u64 = 0xffff_8000_0000_0000;
pub const PHYSICAL_DIRECT_MAP_END: u64 = PHYSICAL_DIRECT_MAP_START + MAX_DIRECT_MAP_BYTES - 1;
pub const KERNEL_SPACE_START: u64 = PHYSICAL_DIRECT_MAP_START;
pub const KERNEL_HEAP_START: u64 = 0xffff_9000_0000_0000;
pub const KERNEL_STACK_START: u64 = 0xffff_a000_0000_0000;
pub const DEVICE_SPACE_START: u64 = 0xffff_b000_0000_0000;
pub const TEMPORARY_MAPPING_START: u64 = 0xffff_c000_0000_0000;
pub const MAX_DIRECT_MAP_BYTES: u64 = 32 * PAGE_SIZE_1G;
pub const MAX_MAPPINGS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageFlags(u64);

impl PageFlags {
    pub const PRESENT: Self = Self(1 << 0);
    pub const WRITABLE: Self = Self(1 << 1);
    pub const USER: Self = Self(1 << 2);
    pub const WRITE_THROUGH: Self = Self(1 << 3);
    pub const CACHE_DISABLE: Self = Self(1 << 4);
    pub const ACCESSED: Self = Self(1 << 5);
    pub const DIRTY: Self = Self(1 << 6);
    pub const HUGE: Self = Self(1 << 7);
    pub const GLOBAL: Self = Self(1 << 8);
    pub const NO_EXECUTE: Self = Self(1 << 63);

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    #[must_use]
    pub const fn is_present(self) -> bool {
        self.contains(Self::PRESENT)
    }

    #[must_use]
    pub const fn is_writable(self) -> bool {
        self.contains(Self::WRITABLE)
    }

    #[must_use]
    pub const fn is_user(self) -> bool {
        self.contains(Self::USER)
    }

    #[must_use]
    pub const fn is_executable(self) -> bool {
        !self.contains(Self::NO_EXECUTE)
    }

    #[must_use]
    pub const fn is_huge(self) -> bool {
        self.contains(Self::HUGE)
    }

    /// Validates SanjuOS mapping policy.
    ///
    /// # Errors
    ///
    /// Returns [`PagingError::WriteExecuteViolation`] when a page would be
    /// writable and executable through the same virtual mapping.
    pub const fn validate(self) -> Result<(), PagingError> {
        if self.is_writable() && self.is_executable() {
            Err(PagingError::WriteExecuteViolation)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualPage {
    start_address: u64,
}

impl VirtualPage {
    #[must_use]
    pub const fn containing(address: u64) -> Self {
        Self {
            start_address: address & !(PAGE_SIZE - 1),
        }
    }

    #[must_use]
    pub const fn from_start_address(address: u64) -> Option<Self> {
        if address.is_multiple_of(PAGE_SIZE) && is_canonical(address) {
            Some(Self {
                start_address: address,
            })
        } else {
            None
        }
    }

    #[must_use]
    pub const fn start_address(self) -> u64 {
        self.start_address
    }

    #[must_use]
    pub const fn indices(self) -> PageTableIndices {
        PageTableIndices::from_address(self.start_address)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageTableIndices {
    pub pml4: usize,
    pub pdpt: usize,
    pub page_directory: usize,
    pub page_table: usize,
}

impl PageTableIndices {
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn from_address(address: u64) -> Self {
        Self {
            pml4: ((address >> 39) & 0x1ff) as usize,
            pdpt: ((address >> 30) & 0x1ff) as usize,
            page_directory: ((address >> 21) & 0x1ff) as usize,
            page_table: ((address >> 12) & 0x1ff) as usize,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PagingError {
    Unaligned,
    NonCanonicalAddress,
    AddressOverflow,
    AlreadyMapped,
    NotMapped,
    MappingTableFull,
    WriteExecuteViolation,
    InvalidUserAddress,
    BootstrapPoolExhausted,
    HugePageConflict,
    CorruptHierarchy,
    UnsupportedImage,
    PermissionConflict,
    UnsupportedCpuFeature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Mapping {
    page: VirtualPage,
    frame: PhysicalFrame,
    flags: PageFlags,
    occupied: bool,
}

impl Mapping {
    const fn empty() -> Self {
        Self {
            page: VirtualPage { start_address: 0 },
            frame: PhysicalFrame::from_start_address_unchecked(0),
            flags: PageFlags::empty(),
            occupied: false,
        }
    }
}

/// Fixed-capacity acceptance model retained for host-side policy tests.
///
/// The hardware implementation is `boot/uefi/src/arch/x86_64/paging.rs`.
pub struct PageTableManager {
    root_frame: u64,
    mappings: [Mapping; MAX_MAPPINGS],
    mapping_count: usize,
}

impl PageTableManager {
    #[must_use]
    pub const fn new(root_frame: u64) -> Self {
        Self {
            root_frame: root_frame & !(PAGE_SIZE - 1),
            mappings: [Mapping::empty(); MAX_MAPPINGS],
            mapping_count: 0,
        }
    }

    #[must_use]
    pub const fn root_frame(&self) -> u64 {
        self.root_frame
    }

    #[must_use]
    pub const fn mapping_count(&self) -> usize {
        self.mapping_count
    }

    /// Adds one 4 KiB mapping while enforcing W^X.
    ///
    /// # Errors
    ///
    /// Returns an error for unaligned addresses, duplicate pages, a full table,
    /// or writable+executable mappings.
    pub fn map(
        &mut self,
        page: VirtualPage,
        frame: PhysicalFrame,
        flags: PageFlags,
    ) -> Result<(), PagingError> {
        if !page.start_address().is_multiple_of(PAGE_SIZE)
            || !frame.start_address().is_multiple_of(PAGE_SIZE)
        {
            return Err(PagingError::Unaligned);
        }
        flags.validate()?;
        if self
            .mappings
            .iter()
            .any(|mapping| mapping.occupied && mapping.page == page)
        {
            return Err(PagingError::AlreadyMapped);
        }
        let Some(slot) = self.mappings.iter_mut().find(|mapping| !mapping.occupied) else {
            return Err(PagingError::MappingTableFull);
        };
        *slot = Mapping {
            page,
            frame,
            flags: flags.union(PageFlags::PRESENT),
            occupied: true,
        };
        self.mapping_count += 1;
        Ok(())
    }

    /// Removes and returns a mapped frame.
    ///
    /// # Errors
    ///
    /// Returns [`PagingError::NotMapped`] when the virtual page is absent.
    pub fn unmap(&mut self, page: VirtualPage) -> Result<PhysicalFrame, PagingError> {
        let Some(mapping) = self
            .mappings
            .iter_mut()
            .find(|mapping| mapping.occupied && mapping.page == page)
        else {
            return Err(PagingError::NotMapped);
        };
        mapping.occupied = false;
        self.mapping_count = self.mapping_count.saturating_sub(1);
        Ok(mapping.frame)
    }

    #[must_use]
    pub fn flags_for(&self, page: VirtualPage) -> Option<PageFlags> {
        self.mappings
            .iter()
            .find(|mapping| mapping.occupied && mapping.page == page)
            .map(|mapping| mapping.flags)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualMemoryLayout {
    pub user_start: u64,
    pub user_end: u64,
    pub physical_direct_map_start: u64,
    pub physical_direct_map_end: u64,
    pub heap_start: u64,
    pub stack_start: u64,
    pub device_start: u64,
    pub temporary_mapping_start: u64,
}

impl VirtualMemoryLayout {
    #[must_use]
    pub const fn sanjuos() -> Self {
        Self {
            user_start: USER_SPACE_START,
            user_end: USER_SPACE_END,
            physical_direct_map_start: PHYSICAL_DIRECT_MAP_START,
            physical_direct_map_end: PHYSICAL_DIRECT_MAP_END,
            heap_start: KERNEL_HEAP_START,
            stack_start: KERNEL_STACK_START,
            device_start: DEVICE_SPACE_START,
            temporary_mapping_start: TEMPORARY_MAPPING_START,
        }
    }

    #[must_use]
    pub const fn is_frozen(self) -> bool {
        self.user_start == USER_SPACE_START
            && self.user_end == USER_SPACE_END
            && self.physical_direct_map_start == PHYSICAL_DIRECT_MAP_START
            && self.physical_direct_map_end == PHYSICAL_DIRECT_MAP_END
            && self.physical_direct_map_end < self.heap_start
            && self.heap_start == KERNEL_HEAP_START
            && self.heap_start < self.stack_start
            && self.stack_start == KERNEL_STACK_START
            && self.stack_start < self.device_start
            && self.device_start == DEVICE_SPACE_START
            && self.device_start < self.temporary_mapping_start
            && self.temporary_mapping_start == TEMPORARY_MAPPING_START
    }

    #[must_use]
    pub fn is_user_range(self, start: u64, length: usize) -> bool {
        let Ok(length) = u64::try_from(length) else {
            return false;
        };
        let Some(end) = start.checked_add(length) else {
            return false;
        };
        start >= self.user_start && end <= self.user_end && start <= end
    }

    #[must_use]
    pub fn direct_map_address(self, physical: u64) -> Option<u64> {
        if physical >= MAX_DIRECT_MAP_BYTES {
            return None;
        }
        self.physical_direct_map_start.checked_add(physical)
    }

    #[must_use]
    pub fn direct_map_physical(self, virtual_address: u64) -> Option<u64> {
        if virtual_address < self.physical_direct_map_start
            || virtual_address > self.physical_direct_map_end
        {
            return None;
        }
        let physical = virtual_address.checked_sub(self.physical_direct_map_start)?;
        (physical < MAX_DIRECT_MAP_BYTES).then_some(physical)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuardedStack {
    pub guard_page: VirtualPage,
    pub stack_start: VirtualPage,
    pub stack_pages: usize,
    pub stack_top: u64,
}

impl GuardedStack {
    /// Creates a stack descriptor with one unmapped guard page below it.
    ///
    /// # Errors
    ///
    /// Returns [`PagingError::Unaligned`] for a non-page-aligned base.
    pub fn new(base: u64, stack_pages: usize) -> Result<Self, PagingError> {
        if !base.is_multiple_of(PAGE_SIZE) || stack_pages == 0 {
            return Err(PagingError::Unaligned);
        }
        if !is_canonical(base) {
            return Err(PagingError::NonCanonicalAddress);
        }
        let stack_start = base
            .checked_add(PAGE_SIZE)
            .ok_or(PagingError::AddressOverflow)?;
        let stack_bytes = u64::try_from(stack_pages)
            .ok()
            .and_then(|pages| pages.checked_mul(PAGE_SIZE))
            .ok_or(PagingError::AddressOverflow)?;
        let stack_top = stack_start
            .checked_add(stack_bytes)
            .ok_or(PagingError::AddressOverflow)?;
        if !is_canonical(stack_top.saturating_sub(1)) {
            return Err(PagingError::NonCanonicalAddress);
        }
        Ok(Self {
            guard_page: VirtualPage::containing(base),
            stack_start: VirtualPage::containing(stack_start),
            stack_pages,
            stack_top,
        })
    }
}

#[must_use]
pub const fn is_canonical(address: u64) -> bool {
    let upper = address >> 48;
    let sign = (address >> 47) & 1;
    (sign == 0 && upper == 0) || (sign == 1 && upper == 0xffff)
}

#[cfg(test)]
mod tests {
    use super::{
        GuardedStack, MAX_DIRECT_MAP_BYTES, PHYSICAL_DIRECT_MAP_START, PageFlags, PageTableIndices,
        PageTableManager, PagingError, VirtualMemoryLayout, VirtualPage, is_canonical,
    };
    use crate::memory::PhysicalFrame;

    #[test]
    fn mappings_enforce_write_xor_execute() {
        let mut manager = PageTableManager::new(0x1000);
        let page = VirtualPage::containing(0x400000);
        let frame = PhysicalFrame::from_start_address(0x200000).unwrap();
        let flags = PageFlags::WRITABLE;
        assert_eq!(
            manager.map(page, frame, flags),
            Err(PagingError::WriteExecuteViolation)
        );

        let safe = PageFlags::WRITABLE.union(PageFlags::NO_EXECUTE);
        manager.map(page, frame, safe).unwrap();
        assert!(manager.flags_for(page).unwrap().is_writable());
        assert!(!manager.flags_for(page).unwrap().is_executable());
        assert_eq!(manager.unmap(page), Ok(frame));
    }

    #[test]
    fn guarded_stack_reserves_first_page() {
        let stack = GuardedStack::new(0x800000, 4).unwrap();
        assert_eq!(stack.guard_page.start_address(), 0x800000);
        assert_eq!(stack.stack_start.start_address(), 0x801000);
        assert_eq!(stack.stack_top, 0x805000);
    }

    #[test]
    fn page_table_indices_cover_all_four_levels() {
        let indices = PageTableIndices::from_address(0xffff_8123_4567_8000);
        assert_eq!(indices.pml4, 258);
        assert_eq!(indices.pdpt, 141);
        assert_eq!(indices.page_directory, 43);
        assert_eq!(indices.page_table, 120);
    }

    #[test]
    fn direct_map_round_trip_is_stable() {
        let layout = VirtualMemoryLayout::sanjuos();
        let virtual_address = layout.direct_map_address(0x1234_5000).unwrap();
        assert_eq!(virtual_address, PHYSICAL_DIRECT_MAP_START + 0x1234_5000);
        assert_eq!(
            layout.direct_map_physical(virtual_address),
            Some(0x1234_5000)
        );
        assert_eq!(
            layout.direct_map_physical(PHYSICAL_DIRECT_MAP_START + MAX_DIRECT_MAP_BYTES),
            None
        );
        assert!(layout.is_frozen());
    }

    #[test]
    fn canonical_address_validation_rejects_the_hole() {
        assert!(is_canonical(0x0000_7fff_ffff_ffff));
        assert!(is_canonical(0xffff_8000_0000_0000));
        assert!(!is_canonical(0x0000_8000_0000_0000));
        assert!(!is_canonical(0xffff_7fff_ffff_ffff));
        assert!(!is_canonical(0x0001_0000_0000_0000));
    }
}
