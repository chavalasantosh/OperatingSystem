#![allow(clippy::module_name_repetitions)]

//! Reusable fixed-capacity kernel heap.

use core::alloc::Layout;
use core::ptr::NonNull;

const MAX_REGIONS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapError {
    AlreadyInitialized,
    InvalidRange,
    AddressOverflow,
    UnknownAllocation,
    RegionTableFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Region {
    start: usize,
    size: usize,
    free: bool,
    occupied: bool,
}

impl Region {
    const fn empty() -> Self {
        Self {
            start: 0,
            size: 0,
            free: false,
            occupied: false,
        }
    }
}

/// First-fit allocator with splitting, deallocation, and adjacent-region merge.
pub struct KernelHeap {
    regions: [Region; MAX_REGIONS],
    initialized: bool,
    allocations: usize,
    frees: usize,
}

impl KernelHeap {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            regions: [Region::empty(); MAX_REGIONS],
            initialized: false,
            allocations: 0,
            frees: 0,
        }
    }

    /// Initializes the heap over one mapped writable region.
    ///
    /// # Safety
    ///
    /// The caller must exclusively own the supplied address range for the
    /// lifetime of the allocator.
    ///
    /// # Errors
    ///
    /// Returns an error for repeated initialization, empty ranges, or overflow.
    pub unsafe fn initialize(&mut self, start: usize, size: usize) -> Result<(), HeapError> {
        if self.initialized {
            return Err(HeapError::AlreadyInitialized);
        }
        let end = start.checked_add(size).ok_or(HeapError::AddressOverflow)?;
        if size == 0 || end <= start {
            return Err(HeapError::InvalidRange);
        }
        self.regions[0] = Region {
            start,
            size,
            free: true,
            occupied: true,
        };
        self.initialized = true;
        Ok(())
    }

    #[must_use]
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if !self.initialized || layout.size() == 0 {
            return None;
        }

        for index in 0..self.regions.len() {
            let region = self.regions[index];
            if !region.occupied || !region.free {
                continue;
            }
            let aligned = align_up(region.start, layout.align())?;
            let padding = aligned.checked_sub(region.start)?;
            let needed = padding.checked_add(layout.size())?;
            if needed > region.size {
                continue;
            }

            self.consume_region(index, aligned, layout.size(), padding)?;
            self.allocations = self.allocations.saturating_add(1);
            return NonNull::new(aligned as *mut u8);
        }
        None
    }

    /// Returns a previous allocation to the heap.
    ///
    /// # Errors
    ///
    /// Returns [`HeapError::UnknownAllocation`] if the pointer is not the start
    /// of an active allocation.
    pub fn deallocate(&mut self, pointer: NonNull<u8>) -> Result<(), HeapError> {
        let address = pointer.as_ptr().addr();
        let Some(index) = self
            .regions
            .iter()
            .position(|region| region.occupied && !region.free && region.start == address)
        else {
            return Err(HeapError::UnknownAllocation);
        };
        self.regions[index].free = true;
        self.frees = self.frees.saturating_add(1);
        self.merge_free_regions();
        Ok(())
    }

    #[must_use]
    pub const fn allocations(&self) -> usize {
        self.allocations
    }

    #[must_use]
    pub const fn frees(&self) -> usize {
        self.frees
    }

    #[must_use]
    pub fn free_bytes(&self) -> usize {
        self.regions
            .iter()
            .filter(|region| region.occupied && region.free)
            .map(|region| region.size)
            .sum()
    }

    fn consume_region(
        &mut self,
        index: usize,
        allocation_start: usize,
        allocation_size: usize,
        prefix_size: usize,
    ) -> Option<()> {
        let original = self.regions[index];
        let allocation_end = allocation_start.checked_add(allocation_size)?;
        let original_end = original.start.checked_add(original.size)?;
        let suffix_size = original_end.checked_sub(allocation_end)?;

        self.regions[index] = Region {
            start: allocation_start,
            size: allocation_size,
            free: false,
            occupied: true,
        };

        if prefix_size > 0 {
            self.insert_region(Region {
                start: original.start,
                size: prefix_size,
                free: true,
                occupied: true,
            })?;
        }
        if suffix_size > 0 {
            self.insert_region(Region {
                start: allocation_end,
                size: suffix_size,
                free: true,
                occupied: true,
            })?;
        }
        Some(())
    }

    fn insert_region(&mut self, region: Region) -> Option<()> {
        let slot = self.regions.iter_mut().find(|slot| !slot.occupied)?;
        *slot = region;
        Some(())
    }

    fn merge_free_regions(&mut self) {
        loop {
            let mut merged = false;
            'outer: for left in 0..self.regions.len() {
                if !self.regions[left].occupied || !self.regions[left].free {
                    continue;
                }
                let left_end = self.regions[left]
                    .start
                    .saturating_add(self.regions[left].size);
                for right in 0..self.regions.len() {
                    if left == right || !self.regions[right].occupied || !self.regions[right].free {
                        continue;
                    }
                    if left_end == self.regions[right].start {
                        self.regions[left].size = self.regions[left]
                            .size
                            .saturating_add(self.regions[right].size);
                        self.regions[right] = Region::empty();
                        merged = true;
                        break 'outer;
                    }
                }
            }
            if !merged {
                break;
            }
        }
    }
}

impl Default for KernelHeap {
    fn default() -> Self {
        Self::new()
    }
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    if !alignment.is_power_of_two() {
        return None;
    }
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|candidate| candidate & !(alignment - 1))
}

#[cfg(test)]
mod tests {
    use core::alloc::Layout;

    use super::KernelHeap;

    #[test]
    fn heap_reuses_freed_regions() {
        let mut storage = [0_u8; 4096];
        let mut heap = KernelHeap::new();
        unsafe { heap.initialize(storage.as_mut_ptr().addr(), storage.len()) }.unwrap();
        let layout = Layout::from_size_align(128, 16).unwrap();
        let first = heap.allocate(layout).unwrap();
        let _second = heap.allocate(layout).unwrap();
        heap.deallocate(first).unwrap();
        let reused = heap.allocate(layout).unwrap();
        assert_eq!(reused, first);
        assert_eq!(heap.allocations(), 3);
        assert_eq!(heap.frees(), 1);
    }
}
