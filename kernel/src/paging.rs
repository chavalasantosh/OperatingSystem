#![allow(clippy::module_name_repetitions)]

//! x86-64 virtual-memory policy and page-table bookkeeping.

use crate::memory::{PAGE_SIZE, PhysicalFrame};

pub const USER_SPACE_START: u64 = 0x0000_0000_0040_0000;
pub const USER_SPACE_END: u64 = 0x0000_7fff_ffff_f000;
pub const KERNEL_SPACE_START: u64 = 0xffff_8000_0000_0000;
pub const KERNEL_HEAP_START: u64 = 0xffff_9000_0000_0000;
pub const KERNEL_STACK_START: u64 = 0xffff_a000_0000_0000;
pub const DEVICE_SPACE_START: u64 = 0xffff_b000_0000_0000;
pub const MAX_MAPPINGS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageFlags(u64);

impl PageFlags {
    pub const PRESENT: Self = Self(1 << 0);
    pub const WRITABLE: Self = Self(1 << 1);
    pub const USER: Self = Self(1 << 2);
    pub const GLOBAL: Self = Self(1 << 8);
    pub const NO_EXECUTE: Self = Self(1 << 63);

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
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
    pub const fn is_writable(self) -> bool {
        self.contains(Self::WRITABLE)
    }

    #[must_use]
    pub const fn is_executable(self) -> bool {
        !self.contains(Self::NO_EXECUTE)
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
    pub const fn start_address(self) -> u64 {
        self.start_address
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PagingError {
    Unaligned,
    AlreadyMapped,
    NotMapped,
    MappingTableFull,
    WriteExecuteViolation,
    InvalidUserAddress,
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

/// Fixed-capacity page-table ownership model used while the architecture layer
/// grows the hardware page-table implementation.
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
        if flags.is_writable() && flags.is_executable() {
            return Err(PagingError::WriteExecuteViolation);
        }
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
    pub kernel_start: u64,
    pub heap_start: u64,
    pub stack_start: u64,
    pub device_start: u64,
}

impl VirtualMemoryLayout {
    #[must_use]
    pub const fn sanjuos() -> Self {
        Self {
            user_start: USER_SPACE_START,
            user_end: USER_SPACE_END,
            kernel_start: KERNEL_SPACE_START,
            heap_start: KERNEL_HEAP_START,
            stack_start: KERNEL_STACK_START,
            device_start: DEVICE_SPACE_START,
        }
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
        let stack_start = base.checked_add(PAGE_SIZE).ok_or(PagingError::Unaligned)?;
        let stack_bytes = u64::try_from(stack_pages)
            .ok()
            .and_then(|pages| pages.checked_mul(PAGE_SIZE))
            .ok_or(PagingError::Unaligned)?;
        let stack_top = stack_start
            .checked_add(stack_bytes)
            .ok_or(PagingError::Unaligned)?;
        Ok(Self {
            guard_page: VirtualPage::containing(base),
            stack_start: VirtualPage::containing(stack_start),
            stack_pages,
            stack_top,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{GuardedStack, PageFlags, PageTableManager, PagingError, VirtualPage};
    use crate::memory::PhysicalFrame;

    #[test]
    fn mappings_enforce_write_xor_execute() {
        let mut manager = PageTableManager::new(0x1000);
        let page = VirtualPage::containing(0x400000);
        let frame = PhysicalFrame::from_start_address(0x200000).unwrap();
        let flags = PageFlags::WRITABLE;
        assert_eq!(manager.map(page, frame, flags), Err(PagingError::WriteExecuteViolation));

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
}
