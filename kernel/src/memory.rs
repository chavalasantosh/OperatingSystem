#![allow(clippy::module_name_repetitions)]

use core::alloc::Layout;
use core::ptr::NonNull;

use crate::MemoryMapInfo;
use crate::boot_info::PhysicalRange;
use crate::ownership::PhysicalOwnershipMap;

pub const PAGE_SIZE: u64 = 4096;
pub const DEFAULT_FRAME_BITMAP_WORDS: usize = 32_768;
pub const DEFAULT_FRAME_BITMAP_CAPACITY: u64 = 2_097_152;
pub const PAGE_TABLE_BOOTSTRAP_FRAMES: usize = 256;
const MAX_USABLE_REGIONS: usize = 128;
const EFI_BOOT_SERVICES_CODE: u32 = 3;
const EFI_BOOT_SERVICES_DATA: u32 = 4;
const EFI_CONVENTIONAL_MEMORY: u32 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalFrame {
    start_address: u64,
}

impl PhysicalFrame {
    pub const ZERO: Self = Self { start_address: 0 };

    #[must_use]
    pub const fn from_start_address(start_address: u64) -> Option<Self> {
        if start_address.is_multiple_of(PAGE_SIZE) {
            Some(Self { start_address })
        } else {
            None
        }
    }

    #[must_use]
    pub(crate) const fn from_start_address_unchecked(start_address: u64) -> Self {
        Self { start_address }
    }

    #[must_use]
    pub const fn start_address(self) -> u64 {
        self.start_address
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalFrameRange {
    pub start: PhysicalFrame,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameStatistics {
    pub total_usable: u64,
    pub allocated: u64,
    pub reserved: u64,
    pub free: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    InvalidMemoryMap,
    AddressOverflow,
    HeapAlreadyInitialized,
    InvalidHeapRange,
    BitmapSizeMismatch,
    BitmapCapacityExceeded,
    TooManyUsableRegions,
    FrameOutOfRange,
    FrameNotAligned,
    DoubleFree,
    ReservedFrame,
    AlreadyReserved,
    BootstrapPoolExhausted,
    BootstrapPoolForeignFrame,
    BootstrapPoolDoubleFree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Descriptor {
    memory_type: u32,
    physical_start: u64,
    number_of_pages: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UsableRegion {
    physical_start: u64,
    frame_count: u64,
    bitmap_start: u64,
}

impl UsableRegion {
    const EMPTY: Self = Self {
        physical_start: 0,
        frame_count: 0,
        bitmap_start: 0,
    };
}

pub struct FrameBitmap<'a> {
    reserved: &'a mut [u64],
    allocated: &'a mut [u64],
}

impl<'a> FrameBitmap<'a> {
    pub fn new(reserved: &'a mut [u64], allocated: &'a mut [u64]) -> Result<Self, MemoryError> {
        if reserved.is_empty() || reserved.len() != allocated.len() {
            return Err(MemoryError::BitmapSizeMismatch);
        }
        Ok(Self {
            reserved,
            allocated,
        })
    }

    #[must_use]
    pub fn capacity_frames(&self) -> u64 {
        u64::try_from(self.reserved.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(64)
    }

    fn clear(&mut self) {
        self.reserved.fill(0);
        self.allocated.fill(0);
    }
}

/// Bitmap-backed physical-frame allocator over UEFI conventional memory.
///
/// Reserved and allocated frames are tracked independently so invalid frees are
/// detected rather than silently corrupting kernel-owned memory.
pub struct FrameAllocator<'a> {
    map: MemoryMapInfo,
    bitmap: FrameBitmap<'a>,
    regions: [UsableRegion; MAX_USABLE_REGIONS],
    region_count: usize,
    total_usable_frames: u64,
    allocated_frames: u64,
    reserved_frames: u64,
    next_hint: u64,
}

impl<'a> FrameAllocator<'a> {
    /// # Safety
    ///
    /// The retained memory-map bytes must remain readable for the allocator's
    /// lifetime. Both bitmap slices must be exclusively owned for the same
    /// lifetime and large enough for every conventional-memory frame.
    pub unsafe fn from_memory_map(
        map: MemoryMapInfo,
        mut bitmap: FrameBitmap<'a>,
        ownership: &PhysicalOwnershipMap,
    ) -> Result<Self, MemoryError> {
        if !map.is_structurally_valid() || map.descriptor_size < 40 {
            return Err(MemoryError::InvalidMemoryMap);
        }

        bitmap.clear();
        let descriptor_count = map
            .descriptor_count_usize()
            .ok_or(MemoryError::InvalidMemoryMap)?;
        let mut regions = [UsableRegion::EMPTY; MAX_USABLE_REGIONS];
        let mut region_count = 0_usize;
        let mut total_usable_frames = 0_u64;

        for descriptor_index in 0..descriptor_count {
            // SAFETY: The caller guarantees the retained map and the structural
            // validation above bounds every descriptor access.
            let descriptor = unsafe { read_descriptor(map, descriptor_index)? };
            if descriptor.memory_type != EFI_CONVENTIONAL_MEMORY
                || descriptor.number_of_pages == 0
            {
                continue;
            }
            if !descriptor.physical_start.is_multiple_of(PAGE_SIZE) {
                return Err(MemoryError::InvalidMemoryMap);
            }
            if region_count == regions.len() {
                return Err(MemoryError::TooManyUsableRegions);
            }
            let next_total = total_usable_frames
                .checked_add(descriptor.number_of_pages)
                .ok_or(MemoryError::AddressOverflow)?;
            if next_total > bitmap.capacity_frames() {
                return Err(MemoryError::BitmapCapacityExceeded);
            }
            descriptor
                .physical_start
                .checked_add(
                    descriptor
                        .number_of_pages
                        .checked_mul(PAGE_SIZE)
                        .ok_or(MemoryError::AddressOverflow)?,
                )
                .ok_or(MemoryError::AddressOverflow)?;

            regions[region_count] = UsableRegion {
                physical_start: descriptor.physical_start,
                frame_count: descriptor.number_of_pages,
                bitmap_start: total_usable_frames,
            };
            region_count += 1;
            total_usable_frames = next_total;
        }

        let mut allocator = Self {
            map,
            bitmap,
            regions,
            region_count,
            total_usable_frames,
            allocated_frames: 0,
            reserved_frames: 0,
            next_hint: 0,
        };
        for entry in ownership.entries() {
            allocator.reserve_range(entry.range)?;
        }
        Ok(allocator)
    }

    #[must_use]
    pub const fn total_usable_frames(&self) -> u64 {
        self.total_usable_frames
    }

    #[must_use]
    pub const fn allocated_frames(&self) -> u64 {
        self.allocated_frames
    }

    #[must_use]
    pub const fn reserved_frames(&self) -> u64 {
        self.reserved_frames
    }

    #[must_use]
    pub fn statistics(&self) -> FrameStatistics {
        FrameStatistics {
            total_usable: self.total_usable_frames,
            allocated: self.allocated_frames,
            reserved: self.reserved_frames,
            free: self
                .total_usable_frames
                .saturating_sub(self.allocated_frames)
                .saturating_sub(self.reserved_frames),
        }
    }

    #[must_use]
    pub fn reclaimable_boot_service_frames(&self) -> u64 {
        let Some(descriptor_count) = self.map.descriptor_count_usize() else {
            return 0;
        };
        let mut frames = 0_u64;
        for descriptor_index in 0..descriptor_count {
            // SAFETY: `Self` exists only after memory-map validation.
            let Ok(descriptor) = (unsafe { read_descriptor(self.map, descriptor_index) }) else {
                continue;
            };
            if matches!(
                descriptor.memory_type,
                EFI_BOOT_SERVICES_CODE | EFI_BOOT_SERVICES_DATA
            ) {
                frames = frames.saturating_add(descriptor.number_of_pages);
            }
        }
        frames
    }

    pub fn allocate_frame(&mut self) -> Option<PhysicalFrame> {
        if self.total_usable_frames == 0 {
            return None;
        }
        for offset in 0..self.total_usable_frames {
            let bit = (self.next_hint + offset) % self.total_usable_frames;
            if !self.bit_is_set(bit, true) && !self.bit_is_set(bit, false) {
                self.set_bit(bit, false, true);
                self.allocated_frames = self.allocated_frames.saturating_add(1);
                self.next_hint = (bit + 1) % self.total_usable_frames;
                return self.frame_for_bit(bit);
            }
        }
        None
    }

    pub fn allocate_contiguous(
        &mut self,
        count: usize,
        alignment_frames: usize,
    ) -> Option<PhysicalFrameRange> {
        if count == 0 || alignment_frames == 0 || !alignment_frames.is_power_of_two() {
            return None;
        }
        let count_u64 = u64::try_from(count).ok()?;
        let alignment_u64 = u64::try_from(alignment_frames).ok()?;

        for region_index in 0..self.region_count {
            let region = self.regions[region_index];
            if count_u64 > region.frame_count {
                continue;
            }
            let mut local = 0_u64;
            while local + count_u64 <= region.frame_count {
                let frame_number = region.physical_start / PAGE_SIZE + local;
                if !frame_number.is_multiple_of(alignment_u64) {
                    local += 1;
                    continue;
                }
                let first_bit = region.bitmap_start + local;
                let available = (0..count_u64).all(|delta| {
                    !self.bit_is_set(first_bit + delta, true)
                        && !self.bit_is_set(first_bit + delta, false)
                });
                if available {
                    for delta in 0..count_u64 {
                        self.set_bit(first_bit + delta, false, true);
                    }
                    self.allocated_frames = self.allocated_frames.saturating_add(count_u64);
                    self.next_hint = (first_bit + count_u64) % self.total_usable_frames;
                    return Some(PhysicalFrameRange {
                        start: PhysicalFrame {
                            start_address: region.physical_start + local * PAGE_SIZE,
                        },
                        count,
                    });
                }
                local += 1;
            }
        }
        None
    }

    pub fn free_frame(&mut self, frame: PhysicalFrame) -> Result<(), MemoryError> {
        let bit = self.bit_for_frame(frame)?;
        if self.bit_is_set(bit, true) {
            return Err(MemoryError::ReservedFrame);
        }
        if !self.bit_is_set(bit, false) {
            return Err(MemoryError::DoubleFree);
        }
        self.set_bit(bit, false, false);
        self.allocated_frames = self.allocated_frames.saturating_sub(1);
        self.next_hint = self.next_hint.min(bit);
        Ok(())
    }

    pub fn reserve_range(&mut self, range: PhysicalRange) -> Result<(), MemoryError> {
        if range.is_empty() {
            return Ok(());
        }
        let end = range
            .end_exclusive()
            .ok_or(MemoryError::AddressOverflow)?;
        let aligned_start = range.start - (range.start % PAGE_SIZE);
        let aligned_end = align_up_u64(end, PAGE_SIZE).ok_or(MemoryError::AddressOverflow)?;
        let mut address = aligned_start;
        while address < aligned_end {
            if let Ok(bit) = self.bit_for_address(address) {
                if self.bit_is_set(bit, false) {
                    return Err(MemoryError::AlreadyReserved);
                }
                if !self.bit_is_set(bit, true) {
                    self.set_bit(bit, true, true);
                    self.reserved_frames = self.reserved_frames.saturating_add(1);
                }
            }
            address = address
                .checked_add(PAGE_SIZE)
                .ok_or(MemoryError::AddressOverflow)?;
        }
        Ok(())
    }

    pub fn promote_allocated_to_reserved(
        &mut self,
        frame: PhysicalFrame,
    ) -> Result<(), MemoryError> {
        let bit = self.bit_for_frame(frame)?;
        if self.bit_is_set(bit, true) {
            return Err(MemoryError::AlreadyReserved);
        }
        if !self.bit_is_set(bit, false) {
            return Err(MemoryError::DoubleFree);
        }
        self.set_bit(bit, false, false);
        self.set_bit(bit, true, true);
        self.allocated_frames = self.allocated_frames.saturating_sub(1);
        self.reserved_frames = self.reserved_frames.saturating_add(1);
        Ok(())
    }

    fn bit_for_frame(&self, frame: PhysicalFrame) -> Result<u64, MemoryError> {
        if !frame.start_address.is_multiple_of(PAGE_SIZE) {
            return Err(MemoryError::FrameNotAligned);
        }
        self.bit_for_address(frame.start_address)
    }

    fn bit_for_address(&self, address: u64) -> Result<u64, MemoryError> {
        for region in &self.regions[..self.region_count] {
            let length = region
                .frame_count
                .checked_mul(PAGE_SIZE)
                .ok_or(MemoryError::AddressOverflow)?;
            let end = region
                .physical_start
                .checked_add(length)
                .ok_or(MemoryError::AddressOverflow)?;
            if address >= region.physical_start && address < end {
                let local = (address - region.physical_start) / PAGE_SIZE;
                return Ok(region.bitmap_start + local);
            }
        }
        Err(MemoryError::FrameOutOfRange)
    }

    fn frame_for_bit(&self, bit: u64) -> Option<PhysicalFrame> {
        for region in &self.regions[..self.region_count] {
            let end_bit = region.bitmap_start.checked_add(region.frame_count)?;
            if bit >= region.bitmap_start && bit < end_bit {
                let local = bit - region.bitmap_start;
                let offset = local.checked_mul(PAGE_SIZE)?;
                let start_address = region.physical_start.checked_add(offset)?;
                return Some(PhysicalFrame { start_address });
            }
        }
        None
    }

    fn bit_is_set(&self, bit: u64, reserved: bool) -> bool {
        let Ok(word) = usize::try_from(bit / 64) else {
            return true;
        };
        let mask = 1_u64 << (bit % 64);
        let bitmap: &[u64] = if reserved {
            &*self.bitmap.reserved
        } else {
            &*self.bitmap.allocated
        };
        bitmap.get(word).is_none_or(|value| *value & mask != 0)
    }

    fn set_bit(&mut self, bit: u64, reserved: bool, value: bool) {
        let word = usize::try_from(bit / 64).expect("validated bitmap index");
        let mask = 1_u64 << (bit % 64);
        let bitmap: &mut [u64] = if reserved {
            &mut *self.bitmap.reserved
        } else {
            &mut *self.bitmap.allocated
        };
        if value {
            bitmap[word] |= mask;
        } else {
            bitmap[word] &= !mask;
        }
    }
}

pub struct PageTableBootstrapPool<const N: usize> {
    frames: [PhysicalFrame; N],
    in_use: [bool; N],
}

impl<const N: usize> PageTableBootstrapPool<N> {
    pub fn reserve(allocator: &mut FrameAllocator<'_>) -> Result<Self, MemoryError> {
        let mut pool = Self {
            frames: [PhysicalFrame::ZERO; N],
            in_use: [false; N],
        };
        for index in 0..N {
            let frame = allocator
                .allocate_frame()
                .ok_or(MemoryError::BootstrapPoolExhausted)?;
            allocator.promote_allocated_to_reserved(frame)?;
            pool.frames[index] = frame;
        }
        Ok(pool)
    }

    pub fn allocate(&mut self) -> Option<PhysicalFrame> {
        for (index, used) in self.in_use.iter_mut().enumerate() {
            if !*used {
                *used = true;
                return Some(self.frames[index]);
            }
        }
        None
    }

    pub fn free(&mut self, frame: PhysicalFrame) -> Result<(), MemoryError> {
        let Some(index) = self.frames.iter().position(|candidate| *candidate == frame) else {
            return Err(MemoryError::BootstrapPoolForeignFrame);
        };
        if !self.in_use[index] {
            return Err(MemoryError::BootstrapPoolDoubleFree);
        }
        self.in_use[index] = false;
        Ok(())
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.in_use.iter().filter(|used| !**used).count()
    }

    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.frames.len()
    }
}

#[derive(Debug)]
pub struct BumpAllocator {
    end: usize,
    next: usize,
    allocations: usize,
    initialized: bool,
}

impl BumpAllocator {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            end: 0,
            next: 0,
            allocations: 0,
            initialized: false,
        }
    }

    /// # Safety
    ///
    /// The caller must guarantee exclusive access to the writable heap range.
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

    #[must_use]
    pub const fn allocations(&self) -> usize {
        self.allocations
    }

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

fn align_up_u64(value: u64, alignment: u64) -> Option<u64> {
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
    let descriptor_size = map
        .descriptor_size_usize()
        .ok_or(MemoryError::InvalidMemoryMap)?;
    let map_size = map.map_size_usize().ok_or(MemoryError::InvalidMemoryMap)?;
    let buffer_address = map
        .buffer_address_usize()
        .ok_or(MemoryError::InvalidMemoryMap)?;
    let byte_offset = descriptor_index
        .checked_mul(descriptor_size)
        .ok_or(MemoryError::AddressOverflow)?;
    let descriptor_end = byte_offset
        .checked_add(40)
        .ok_or(MemoryError::AddressOverflow)?;
    if descriptor_end > map_size {
        return Err(MemoryError::InvalidMemoryMap);
    }
    let descriptor_address = buffer_address
        .checked_add(byte_offset)
        .ok_or(MemoryError::AddressOverflow)?;
    let descriptor = descriptor_address as *const u8;
    // SAFETY: The caller provides the valid map range and fixed UEFI offsets.
    let memory_type = unsafe { descriptor.cast::<u32>().read_unaligned() };
    // SAFETY: Offset 8 is the UEFI `PhysicalStart` field.
    let physical_start = unsafe { descriptor.add(8).cast::<u64>().read_unaligned() };
    // SAFETY: Offset 24 is the UEFI `NumberOfPages` field.
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

    use super::{
        BumpAllocator, FrameAllocator, FrameBitmap, MemoryError, PAGE_SIZE,
        PAGE_TABLE_BOOTSTRAP_FRAMES, PageTableBootstrapPool,
    };
    use crate::MemoryMapInfo;
    use crate::boot_info::PhysicalRange;
    use crate::ownership::{OwnershipKind, PhysicalOwnershipMap};

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

    fn test_map(descriptors: &[TestDescriptor]) -> MemoryMapInfo {
        let map_size = core::mem::size_of_val(descriptors);
        MemoryMapInfo {
            buffer_address: u64::try_from(descriptors.as_ptr().addr()).unwrap(),
            buffer_capacity: u64::try_from(map_size).unwrap(),
            map_size: u64::try_from(map_size).unwrap(),
            map_key: 1,
            descriptor_size: u64::try_from(core::mem::size_of::<TestDescriptor>()).unwrap(),
            descriptor_version: 1,
            reserved: 0,
            descriptor_count: u64::try_from(descriptors.len()).unwrap(),
        }
    }

    fn conventional_descriptor(start: u64, pages: u64) -> TestDescriptor {
        TestDescriptor {
            memory_type: 7,
            physical_start: start,
            number_of_pages: pages,
            ..TestDescriptor::default()
        }
    }

    #[test]
    fn allocates_unique_frames() {
        let descriptors = [conventional_descriptor(0x20_0000, 8)];
        let mut reserved = [0_u64; 1];
        let mut allocated = [0_u64; 1];
        let bitmap = FrameBitmap::new(&mut reserved, &mut allocated).unwrap();
        // SAFETY: The descriptors and bitmap storage outlive the allocator.
        let mut allocator = unsafe {
            FrameAllocator::from_memory_map(
                test_map(&descriptors),
                bitmap,
                &PhysicalOwnershipMap::new(),
            )
        }
        .unwrap();
        assert_ne!(
            allocator.allocate_frame().unwrap(),
            allocator.allocate_frame().unwrap()
        );
    }

    #[test]
    fn reuses_freed_frames() {
        let descriptors = [conventional_descriptor(0x21_0000, 8)];
        let mut reserved = [0_u64; 1];
        let mut allocated = [0_u64; 1];
        let bitmap = FrameBitmap::new(&mut reserved, &mut allocated).unwrap();
        // SAFETY: Test storage remains alive.
        let mut allocator = unsafe {
            FrameAllocator::from_memory_map(
                test_map(&descriptors),
                bitmap,
                &PhysicalOwnershipMap::new(),
            )
        }
        .unwrap();
        let first = allocator.allocate_frame().unwrap();
        allocator.free_frame(first).unwrap();
        assert_eq!(allocator.allocate_frame(), Some(first));
    }

    #[test]
    fn rejects_double_free() {
        let descriptors = [conventional_descriptor(0x30_0000, 8)];
        let mut reserved = [0_u64; 1];
        let mut allocated = [0_u64; 1];
        let bitmap = FrameBitmap::new(&mut reserved, &mut allocated).unwrap();
        // SAFETY: Test storage remains alive.
        let mut allocator = unsafe {
            FrameAllocator::from_memory_map(
                test_map(&descriptors),
                bitmap,
                &PhysicalOwnershipMap::new(),
            )
        }
        .unwrap();
        let frame = allocator.allocate_frame().unwrap();
        allocator.free_frame(frame).unwrap();
        assert_eq!(allocator.free_frame(frame), Err(MemoryError::DoubleFree));
    }

    #[test]
    fn rejects_reserved_frame_free() {
        let descriptors = [conventional_descriptor(0x31_0000, 8)];
        let mut ownership = PhysicalOwnershipMap::new();
        ownership
            .reserve(
                PhysicalRange {
                    start: 0x31_0000,
                    length: PAGE_SIZE,
                },
                OwnershipKind::KernelImage,
            )
            .unwrap();
        let mut reserved = [0_u64; 1];
        let mut allocated = [0_u64; 1];
        let bitmap = FrameBitmap::new(&mut reserved, &mut allocated).unwrap();
        // SAFETY: Test storage remains alive.
        let mut allocator =
            unsafe { FrameAllocator::from_memory_map(test_map(&descriptors), bitmap, &ownership) }
                .unwrap();
        let reserved_frame = super::PhysicalFrame::from_start_address(0x31_0000).unwrap();
        assert_eq!(
            allocator.free_frame(reserved_frame),
            Err(MemoryError::ReservedFrame)
        );
    }

    #[test]
    fn reserves_unaligned_ranges_correctly() {
        let descriptors = [conventional_descriptor(0x32_0000, 4)];
        let mut ownership = PhysicalOwnershipMap::new();
        ownership
            .reserve(
                PhysicalRange {
                    start: 0x32_0064,
                    length: PAGE_SIZE,
                },
                OwnershipKind::BootInfo,
            )
            .unwrap();
        let mut reserved = [0_u64; 1];
        let mut allocated = [0_u64; 1];
        let bitmap = FrameBitmap::new(&mut reserved, &mut allocated).unwrap();
        // SAFETY: Test storage remains alive.
        let mut allocator =
            unsafe { FrameAllocator::from_memory_map(test_map(&descriptors), bitmap, &ownership) }
                .unwrap();
        assert_eq!(allocator.reserved_frames(), 2);
        assert_eq!(
            allocator.allocate_frame().unwrap().start_address(),
            0x32_2000
        );
    }

    #[test]
    fn handles_allocator_exhaustion() {
        let descriptors = [conventional_descriptor(0x40_0000, 2)];
        let mut reserved = [0_u64; 1];
        let mut allocated = [0_u64; 1];
        let bitmap = FrameBitmap::new(&mut reserved, &mut allocated).unwrap();
        // SAFETY: Test storage remains alive.
        let mut allocator = unsafe {
            FrameAllocator::from_memory_map(
                test_map(&descriptors),
                bitmap,
                &PhysicalOwnershipMap::new(),
            )
        }
        .unwrap();
        assert!(allocator.allocate_frame().is_some());
        assert!(allocator.allocate_frame().is_some());
        assert!(allocator.allocate_frame().is_none());
    }

    #[test]
    fn bootstrap_pool_does_not_use_heap() {
        let pages = u64::try_from(PAGE_TABLE_BOOTSTRAP_FRAMES + 8).unwrap();
        let descriptors = [conventional_descriptor(0x50_0000, pages)];
        let words = (PAGE_TABLE_BOOTSTRAP_FRAMES + 63) / 64 + 1;
        let mut reserved = vec![0_u64; words];
        let mut allocated = vec![0_u64; words];
        let bitmap = FrameBitmap::new(&mut reserved, &mut allocated).unwrap();
        // SAFETY: Test storage remains alive.
        let mut allocator = unsafe {
            FrameAllocator::from_memory_map(
                test_map(&descriptors),
                bitmap,
                &PhysicalOwnershipMap::new(),
            )
        }
        .unwrap();
        let mut pool =
            PageTableBootstrapPool::<PAGE_TABLE_BOOTSTRAP_FRAMES>::reserve(&mut allocator).unwrap();
        let frame = pool.allocate().unwrap();
        assert_eq!(allocator.free_frame(frame), Err(MemoryError::ReservedFrame));
        pool.free(frame).unwrap();
        assert_eq!(pool.remaining(), PAGE_TABLE_BOOTSTRAP_FRAMES);
    }

    #[test]
    fn bump_allocator_honors_alignment_and_capacity() {
        let mut storage = [0_u8; 128];
        let mut heap = BumpAllocator::new();
        // SAFETY: The local storage is exclusively owned for the test.
        unsafe { heap.initialize(storage.as_mut_ptr().addr(), storage.len()) }.unwrap();
        let first = heap
            .allocate(Layout::from_size_align(7, 8).unwrap())
            .unwrap();
        let second = heap
            .allocate(Layout::from_size_align(16, 16).unwrap())
            .unwrap();
        assert_eq!(first.as_ptr().addr() % 8, 0);
        assert_eq!(second.as_ptr().addr() % 16, 0);
        assert_eq!(heap.allocations(), 2);
        assert!(heap.remaining_bytes() < storage.len());
    }
}
