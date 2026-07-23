#![allow(clippy::module_name_repetitions)]

use core::alloc::Layout;
use core::ptr::NonNull;

use crate::MemoryMapInfo;

pub const PAGE_SIZE: u64 = 4096;
const EFI_CONVENTIONAL_MEMORY: u32 = 7;

/// A 4 KiB-aligned physical frame returned by the early frame allocator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalFrame {
    start_address: u64,
}

impl PhysicalFrame {
    /// Returns the physical start address of this frame.
    #[must_use]
    pub const fn start_address(self) -> u64 {
        self.start_address
    }
}

/// Errors raised while constructing the early memory managers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    InvalidMemoryMap,
    AddressOverflow,
    HeapAlreadyInitialized,
    InvalidHeapRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Descriptor {
    memory_type: u32,
    physical_start: u64,
    number_of_pages: u64,
}

/// Allocation-only physical frame allocator built from the retained UEFI map.
///
/// M2 intentionally allocates only `EfiConventionalMemory` frames. Loader and
/// boot-service ranges are not reclaimed yet because they can still contain
/// the active image, page tables, firmware data, or boot metadata.
pub struct FrameAllocator {
    map: MemoryMapInfo,
    descriptor_index: usize,
    frame_index: u64,
    total_usable_frames: u64,
    allocated_frames: u64,
}

impl FrameAllocator {
    /// Creates an allocator over a memory map captured immediately before
    /// `ExitBootServices`.
    ///
    /// # Safety
    ///
    /// `map.buffer_address` must remain readable for `map.map_size` bytes and
    /// each descriptor must use the declared UEFI descriptor stride. The boot
    /// layer guarantees this by retaining the map in static storage.
    pub unsafe fn from_memory_map(map: MemoryMapInfo) -> Result<Self, MemoryError> {
        if !map.is_structurally_valid() || map.descriptor_size < 40 {
            return Err(MemoryError::InvalidMemoryMap);
        }

        let mut total_usable_frames = 0_u64;
        for descriptor_index in 0..map.descriptor_count {
            // SAFETY: The caller guarantees the retained map range and the
            // structural checks above guarantee every descriptor offset lies
            // inside that range.
            let descriptor = unsafe { read_descriptor(map, descriptor_index)? };
            if descriptor.memory_type == EFI_CONVENTIONAL_MEMORY {
                total_usable_frames = total_usable_frames
                    .checked_add(descriptor.number_of_pages)
                    .ok_or(MemoryError::AddressOverflow)?;
            }
        }

        Ok(Self {
            map,
            descriptor_index: 0,
            frame_index: 0,
            total_usable_frames,
            allocated_frames: 0,
        })
    }

    /// Returns the number of conventional-memory frames visible at startup.
    #[must_use]
    pub const fn total_usable_frames(&self) -> u64 {
        self.total_usable_frames
    }

    /// Returns the number of frames handed out by this allocator.
    #[must_use]
    pub const fn allocated_frames(&self) -> u64 {
        self.allocated_frames
    }

    /// Allocates the next available 4 KiB physical frame.
    pub fn allocate_frame(&mut self) -> Option<PhysicalFrame> {
        while self.descriptor_index < self.map.descriptor_count {
            // SAFETY: `Self` can only be created through `from_memory_map`,
            // which validates and retains the memory-map contract.
            let descriptor =
                unsafe { read_descriptor(self.map, self.descriptor_index).ok()? };

            if descriptor.memory_type != EFI_CONVENTIONAL_MEMORY
                || self.frame_index >= descriptor.number_of_pages
            {
                self.descriptor_index += 1;
                self.frame_index = 0;
                continue;
            }

            let frame_offset = self.frame_index.checked_mul(PAGE_SIZE)?;
            let start_address = descriptor.physical_start.checked_add(frame_offset)?;
            self.frame_index += 1;
            self.allocated_frames += 1;
            return Some(PhysicalFrame { start_address });
        }

        None
    }
}

/// Simple allocation-only heap used during the earliest kernel phase.
///
/// Deallocation and reuse are deliberately deferred until the virtual-memory
/// subsystem introduces a production allocator.
#[derive(Debug)]
pub struct BumpAllocator {
    end: usize,
    next: usize,
    allocations: usize,
    initialized: bool,
}

impl BumpAllocator {
    /// Creates an uninitialized bootstrap heap.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            end: 0,
            next: 0,
            allocations: 0,
            initialized: false,
        }
    }

    /// Binds this allocator to a writable, exclusively owned byte range.
    ///
    /// # Safety
    ///
    /// The caller must guarantee exclusive access to `[heap_start, heap_start +
    /// heap_size)` for the allocator's lifetime and that the range is mapped as
    /// writable memory.
    pub unsafe fn initialize(
        &mut self,
        heap_start: usize,
        heap_size: usize,
    ) -> Result<(), MemoryError> {
        if self.initialized {
            return Err(MemoryError::HeapAlreadyInitialized);
        }
        let heap_end = heap_start
            .checked_add(heap_size)
            .ok_or(MemoryError::AddressOverflow)?;
        if heap_size == 0 || heap_end <= heap_start {
            return Err(MemoryError::InvalidHeapRange);
        }

        self.end = heap_end;
        self.next = heap_start;
        self.allocations = 0;
        self.initialized = true;
        Ok(())
    }

    /// Allocates one aligned block from the bootstrap heap.
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if !self.initialized || layout.size() == 0 {
            return None;
        }

        let aligned_start = align_up(self.next, layout.align())?;
        let allocation_end = aligned_start.checked_add(layout.size())?;
        if allocation_end > self.end {
            return None;
        }

        self.next = allocation_end;
        self.allocations += 1;
        NonNull::new(aligned_start as *mut u8)
    }

    /// Returns the number of successful allocations.
    #[must_use]
    pub const fn allocations(&self) -> usize {
        self.allocations
    }

    /// Returns the number of bytes not yet consumed by the heap.
    #[must_use]
    pub const fn remaining_bytes(&self) -> usize {
        self.end.saturating_sub(self.next)
    }
}

impl Default for BumpAllocator {
    fn default() -> Self {
        Self::new()
    }
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|candidate| candidate & !(alignment - 1))
}

#[allow(clippy::cast_ptr_alignment)]
unsafe fn read_descriptor(
    map: MemoryMapInfo,
    descriptor_index: usize,
) -> Result<Descriptor, MemoryError> {
    let byte_offset = descriptor_index
        .checked_mul(map.descriptor_size)
        .ok_or(MemoryError::AddressOverflow)?;
    let descriptor_end = byte_offset
        .checked_add(40)
        .ok_or(MemoryError::AddressOverflow)?;
    if descriptor_end > map.map_size {
        return Err(MemoryError::InvalidMemoryMap);
    }

    let descriptor_address = map
        .buffer_address
        .checked_add(byte_offset)
        .ok_or(MemoryError::AddressOverflow)?;
    let descriptor = descriptor_address as *const u8;

    // SAFETY: The caller provides the valid map range. UEFI permits descriptor
    // strides larger than the base structure, so fields are read independently
    // and unaligned from their fixed ABI offsets.
    let memory_type = unsafe { descriptor.cast::<u32>().read_unaligned() };
    // SAFETY: Offset 8 is the UEFI `PhysicalStart` field and is range-checked.
    let physical_start = unsafe { descriptor.add(8).cast::<u64>().read_unaligned() };
    // SAFETY: Offset 24 is the UEFI `NumberOfPages` field and is range-checked.
    let number_of_pages = unsafe { descriptor.add(24).cast::<u64>().read_unaligned() };

    Ok(Descriptor {
        memory_type,
        physical_start,
        number_of_pages,
    })
}

#[cfg(test)]
mod tests {
    use core::alloc::Layout;

    use super::{BumpAllocator, FrameAllocator, PAGE_SIZE};
    use crate::MemoryMapInfo;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestDescriptor {
        memory_type: u32,
        padding: u32,
        physical_start: u64,
        virtual_start: u64,
        number_of_pages: u64,
        attribute: u64,
    }

    #[test]
    fn frame_allocator_uses_only_conventional_memory() {
        let descriptors = [
            TestDescriptor {
                memory_type: 2,
                physical_start: 0x10_0000,
                number_of_pages: 3,
                ..TestDescriptor::default()
            },
            TestDescriptor {
                memory_type: 7,
                physical_start: 0x20_0000,
                number_of_pages: 2,
                ..TestDescriptor::default()
            },
        ];
        let map_size = core::mem::size_of_val(&descriptors);
        let map = MemoryMapInfo {
            buffer_address: descriptors.as_ptr().addr(),
            buffer_capacity: map_size,
            map_size,
            map_key: 1,
            descriptor_size: core::mem::size_of::<TestDescriptor>(),
            descriptor_version: 1,
            descriptor_count: descriptors.len(),
        };

        // SAFETY: `descriptors` remains alive and immutable for this test.
        let mut allocator = unsafe { FrameAllocator::from_memory_map(map) }.unwrap();
        assert_eq!(allocator.total_usable_frames(), 2);
        assert_eq!(
            allocator.allocate_frame().unwrap().start_address(),
            0x20_0000
        );
        assert_eq!(
            allocator.allocate_frame().unwrap().start_address(),
            0x20_0000 + PAGE_SIZE
        );
        assert!(allocator.allocate_frame().is_none());
    }

    #[test]
    fn bump_allocator_honors_alignment_and_capacity() {
        let mut storage = [0_u8; 128];
        let mut heap = BumpAllocator::new();
        // SAFETY: The local storage is exclusively owned for the test.
        unsafe { heap.initialize(storage.as_mut_ptr().addr(), storage.len()) }.unwrap();

        let first = heap.allocate(Layout::from_size_align(7, 8).unwrap()).unwrap();
        let second = heap
            .allocate(Layout::from_size_align(16, 16).unwrap())
            .unwrap();

        assert_eq!(first.as_ptr().addr() % 8, 0);
        assert_eq!(second.as_ptr().addr() % 16, 0);
        assert_eq!(heap.allocations(), 2);
        assert!(heap.remaining_bytes() < storage.len());
    }
}
