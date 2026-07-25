//! x86-64 hardware page-table ownership and CR3 transition support.

use core::arch::asm;
use core::ptr;

use sanju_kernel::boot_info::{BootInfo, MemoryMapInfo, PhysicalRange};
use sanju_kernel::memory::{
    FrameAllocator, MemoryError, PAGE_SIZE, PAGE_TABLE_BOOTSTRAP_FRAMES, PageTableBootstrapPool,
    PhysicalFrame,
};
use sanju_kernel::paging::{
    KERNEL_HEAP_START, KERNEL_STACK_START, MAX_DIRECT_MAP_BYTES, PAGE_SIZE_2M,
    PHYSICAL_DIRECT_MAP_START, PageFlags, PageTableIndices, PagingError, VirtualMemoryLayout,
    VirtualPage, is_canonical,
};

const ENTRIES_PER_TABLE: usize = 512;
const ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
const HUGE_ADDRESS_MASK: u64 = 0x000f_ffff_ffe0_0000;
const PAGE_OFFSET_MASK: u64 = PAGE_SIZE - 1;
const HUGE_PAGE_OFFSET_MASK: u64 = PAGE_SIZE_2M - 1;
const ENTRY_FLAG_MASK: u64 = !ADDRESS_MASK;
const IMAGE_DOS_SIGNATURE: u16 = 0x5a4d;
const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550;
const IMAGE_FILE_HEADER_SIZE: usize = 20;
const IMAGE_SECTION_HEADER_SIZE: usize = 40;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;
const MAX_IMAGE_SECTIONS: usize = 96;
const MAX_BOOT_PHYSICAL_BYTES: u64 = MAX_DIRECT_MAP_BYTES;
const MAX_INHERITED_TABLE_FRAMES: usize = 512;
const IA32_EFER: u32 = 0xc000_0080;
const EFER_NXE: u64 = 1 << 11;
const CR0_WRITE_PROTECT: u64 = 1 << 16;
const CR4_PAGE_GLOBAL_ENABLE: u64 = 1 << 7;
const CR4_FIVE_LEVEL_PAGING: u64 = 1 << 12;
const CPUID_EXTENDED_MAXIMUM: u32 = 0x8000_0000;
const CPUID_EXTENDED_FEATURES: u32 = 0x8000_0001;
const CPUID_NX_BIT: u32 = 1 << 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HardwarePagingReport {
    pub old_root: u64,
    pub new_root: u64,
    pub mapped_physical_bytes: u64,
    pub table_frames_used: usize,
    pub layout_frozen: bool,
    pub fresh_root_active: bool,
    pub direct_map_active: bool,
    pub mapper_active: bool,
    pub translation_test_passed: bool,
    pub map_unmap_test_passed: bool,
    pub protection_test_passed: bool,
    pub write_xor_execute_enforced: bool,
    pub guard_pages_active: bool,
    pub image_sections_verified: bool,
    pub transition_checkpoint_passed: bool,
}

impl HardwarePagingReport {
    #[must_use]
    pub const fn gate_passed(self) -> bool {
        self.old_root != 0
            && self.new_root != 0
            && self.old_root != self.new_root
            && self.mapped_physical_bytes > 0
            && self.table_frames_used > 0
            && self.layout_frozen
            && self.fresh_root_active
            && self.direct_map_active
            && self.mapper_active
            && self.translation_test_passed
            && self.map_unmap_test_passed
            && self.protection_test_passed
            && self.write_xor_execute_enforced
            && self.guard_pages_active
            && self.image_sections_verified
            && self.transition_checkpoint_passed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImageSection {
    virtual_address: u64,
    virtual_size: u64,
    characteristics: u32,
}

pub struct HardwarePageTable<'a> {
    root: PhysicalFrame,
    pool: &'a mut PageTableBootstrapPool<PAGE_TABLE_BOOTSTRAP_FRAMES>,
    allocated_tables: usize,
    mapped_physical_bytes: u64,
}

impl<'a> HardwarePageTable<'a> {
    /// Creates an empty, zeroed PML4 using the dedicated page-table pool.
    ///
    /// # Errors
    ///
    /// Returns [`PagingError::BootstrapPoolExhausted`] when the pool cannot
    /// provide the root frame.
    pub fn new(
        pool: &'a mut PageTableBootstrapPool<PAGE_TABLE_BOOTSTRAP_FRAMES>,
    ) -> Result<Self, PagingError> {
        let root = pool.allocate().ok_or(PagingError::BootstrapPoolExhausted)?;
        // SAFETY: The bootstrap pool owns the frame exclusively and the
        // inherited identity map keeps it writable before CR3 replacement.
        unsafe {
            zero_table(root)?;
        }
        Ok(Self {
            root,
            pool,
            allocated_tables: 1,
            mapped_physical_bytes: 0,
        })
    }

    #[must_use]
    pub const fn allocated_tables(&self) -> usize {
        self.allocated_tables
    }

    #[must_use]
    pub fn pool_remaining(&self) -> usize {
        self.pool.remaining()
    }

    /// Maps a single 4 KiB page.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid addresses, W^X policy violations, an
    /// existing mapping, a huge-page collision, or page-table exhaustion.
    pub fn map_page(
        &mut self,
        page: VirtualPage,
        frame: PhysicalFrame,
        flags: PageFlags,
    ) -> Result<(), PagingError> {
        validate_leaf_mapping(
            page.start_address(),
            frame.start_address(),
            flags,
            PAGE_SIZE,
        )?;
        let entry = self.leaf_entry(page.start_address(), flags.is_user(), true)?;
        // SAFETY: `leaf_entry` returns a valid PTE in an exclusively owned
        // hierarchy and the pointer remains stable for this mutation.
        let existing = unsafe { entry.read_volatile() };
        if existing & PageFlags::PRESENT.bits() != 0 {
            return Err(PagingError::AlreadyMapped);
        }
        // SAFETY: Same exclusive PTE ownership as above.
        unsafe {
            entry.write_volatile(
                frame.start_address()
                    | flags
                        .union(PageFlags::PRESENT)
                        .without(PageFlags::HUGE)
                        .bits(),
            );
        }
        invalidate_if_active(self.root.start_address(), page.start_address());
        Ok(())
    }

    /// Maps one 2 MiB huge page.
    ///
    /// # Errors
    ///
    /// Returns an error for alignment, W^X, duplicate mapping, hierarchy
    /// collision, or bootstrap-pool exhaustion.
    pub fn map_huge_2m(
        &mut self,
        virtual_address: u64,
        physical_address: u64,
        flags: PageFlags,
    ) -> Result<(), PagingError> {
        validate_leaf_mapping(virtual_address, physical_address, flags, PAGE_SIZE_2M)?;
        let indices = PageTableIndices::from_address(virtual_address);
        let pdpt = self.ensure_table(self.root, indices.pml4, flags.is_user())?;
        let directory = self.ensure_table(pdpt, indices.pdpt, flags.is_user())?;
        let entry = table_entry_pointer(directory, indices.page_directory)?;
        // SAFETY: The directory frame is exclusively owned by this manager.
        let existing = unsafe { entry.read_volatile() };
        if existing & PageFlags::PRESENT.bits() != 0 {
            return Err(PagingError::AlreadyMapped);
        }
        let leaf_flags = flags.union(PageFlags::PRESENT).union(PageFlags::HUGE);
        // SAFETY: `entry` is a valid PDE in the owned hierarchy.
        unsafe {
            entry.write_volatile(physical_address | leaf_flags.bits());
        }
        invalidate_if_active(self.root.start_address(), virtual_address);
        Ok(())
    }

    /// Converts an existing 2 MiB mapping into 512 4 KiB mappings.
    ///
    /// # Errors
    ///
    /// Returns an error if the page is absent, already split, corrupt, or the
    /// bootstrap pool cannot supply a page-table frame.
    pub fn split_huge_2m(&mut self, virtual_address: u64) -> Result<(), PagingError> {
        if !virtual_address.is_multiple_of(PAGE_SIZE_2M) || !is_canonical(virtual_address) {
            return Err(PagingError::Unaligned);
        }
        let indices = PageTableIndices::from_address(virtual_address);
        let pml4_entry = table_entry_value(self.root, indices.pml4)?;
        let pdpt = child_frame(pml4_entry)?;
        let pdpt_entry = table_entry_value(pdpt, indices.pdpt)?;
        let directory = child_frame(pdpt_entry)?;
        let directory_entry = table_entry_pointer(directory, indices.page_directory)?;
        // SAFETY: The directory frame is present in the hierarchy.
        let huge_entry = unsafe { directory_entry.read_volatile() };
        if huge_entry & PageFlags::PRESENT.bits() == 0 {
            return Err(PagingError::NotMapped);
        }
        if huge_entry & PageFlags::HUGE.bits() == 0 {
            return Ok(());
        }

        let table = self.allocate_table()?;
        let physical_base = huge_entry & HUGE_ADDRESS_MASK;
        let inherited_flags = PageFlags::from_bits(huge_entry & ENTRY_FLAG_MASK)
            .without(PageFlags::HUGE)
            .union(PageFlags::PRESENT);
        for index in 0..ENTRIES_PER_TABLE {
            let offset = u64::try_from(index)
                .map_err(|_| PagingError::AddressOverflow)?
                .checked_mul(PAGE_SIZE)
                .ok_or(PagingError::AddressOverflow)?;
            let entry = table_entry_pointer(table, index)?;
            // SAFETY: The new page table is zeroed and exclusively owned.
            unsafe {
                entry.write_volatile((physical_base + offset) | inherited_flags.bits());
            }
        }
        let parent_flags =
            PageFlags::PRESENT
                .union(PageFlags::WRITABLE)
                .union(if inherited_flags.is_user() {
                    PageFlags::USER
                } else {
                    PageFlags::empty()
                });
        // SAFETY: Replacing a huge PDE with the populated child table is the
        // architectural split operation. The active root is flushed below.
        unsafe {
            directory_entry.write_volatile(table.start_address() | parent_flags.bits());
        }
        flush_all_if_active(self.root.start_address());
        Ok(())
    }

    /// Changes permissions on an existing 4 KiB mapping.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid policy, absent mappings, or huge-page
    /// collisions.
    pub fn protect_page(&mut self, page: VirtualPage, flags: PageFlags) -> Result<(), PagingError> {
        flags.validate()?;
        let entry = self.leaf_entry(page.start_address(), flags.is_user(), false)?;
        // SAFETY: The pointer targets a PTE reached through the owned root.
        let value = unsafe { entry.read_volatile() };
        if value & PageFlags::PRESENT.bits() == 0 {
            return Err(PagingError::NotMapped);
        }
        let physical = value & ADDRESS_MASK;
        // SAFETY: The physical target is unchanged and only access flags are
        // replaced under the validated W^X policy.
        unsafe {
            entry.write_volatile(
                physical
                    | flags
                        .union(PageFlags::PRESENT)
                        .without(PageFlags::HUGE)
                        .bits(),
            );
        }
        invalidate_if_active(self.root.start_address(), page.start_address());
        Ok(())
    }

    /// Removes a 4 KiB mapping and returns its physical frame.
    ///
    /// # Errors
    ///
    /// Returns an error for absent mappings or huge-page collisions.
    pub fn unmap_page(&mut self, page: VirtualPage) -> Result<PhysicalFrame, PagingError> {
        let entry = self.leaf_entry(page.start_address(), false, false)?;
        // SAFETY: The pointer targets a PTE reached through the owned root.
        let value = unsafe { entry.read_volatile() };
        if value & PageFlags::PRESENT.bits() == 0 {
            return Err(PagingError::NotMapped);
        }
        let frame = PhysicalFrame::from_start_address(value & ADDRESS_MASK)
            .ok_or(PagingError::CorruptHierarchy)?;
        // SAFETY: Clearing the leaf removes only this mapping.
        unsafe {
            entry.write_volatile(0);
        }
        invalidate_if_active(self.root.start_address(), page.start_address());
        Ok(frame)
    }

    #[must_use]
    pub fn translate(&self, virtual_address: u64) -> Option<u64> {
        translate_from_root(self.root.start_address(), virtual_address)
    }

    #[must_use]
    pub fn flags_for(&self, virtual_address: u64) -> Option<PageFlags> {
        flags_from_root(self.root.start_address(), virtual_address)
    }

    fn leaf_entry(
        &mut self,
        virtual_address: u64,
        user: bool,
        create: bool,
    ) -> Result<*mut u64, PagingError> {
        if !virtual_address.is_multiple_of(PAGE_SIZE) {
            return Err(PagingError::Unaligned);
        }
        if !is_canonical(virtual_address) {
            return Err(PagingError::NonCanonicalAddress);
        }
        let indices = PageTableIndices::from_address(virtual_address);
        let pdpt = if create {
            self.ensure_table(self.root, indices.pml4, user)?
        } else {
            self.existing_table(self.root, indices.pml4, user)?
        };
        let directory = if create {
            self.ensure_table(pdpt, indices.pdpt, user)?
        } else {
            self.existing_table(pdpt, indices.pdpt, user)?
        };
        let directory_entry = table_entry_value(directory, indices.page_directory)?;
        if directory_entry & PageFlags::HUGE.bits() != 0 {
            return Err(PagingError::HugePageConflict);
        }
        let page_table = if create {
            self.ensure_table(directory, indices.page_directory, user)?
        } else {
            self.existing_table(directory, indices.page_directory, user)?
        };
        table_entry_pointer(page_table, indices.page_table)
    }

    fn existing_table(
        &mut self,
        parent: PhysicalFrame,
        index: usize,
        user: bool,
    ) -> Result<PhysicalFrame, PagingError> {
        let entry = table_entry_pointer(parent, index)?;
        // SAFETY: The parent is part of the hierarchy exclusively managed by
        // this object.
        let mut value = unsafe { entry.read_volatile() };
        if value & PageFlags::PRESENT.bits() == 0 {
            return Err(PagingError::NotMapped);
        }
        if value & PageFlags::HUGE.bits() != 0 {
            return Err(PagingError::HugePageConflict);
        }
        if user && value & PageFlags::USER.bits() == 0 {
            value |= PageFlags::USER.bits();
            // SAFETY: Promoting the parent is required for a user leaf and
            // preserves the child frame and every other permission bit.
            unsafe {
                entry.write_volatile(value);
            }
        }
        child_frame(value)
    }

    fn ensure_table(
        &mut self,
        parent: PhysicalFrame,
        index: usize,
        user: bool,
    ) -> Result<PhysicalFrame, PagingError> {
        let entry = table_entry_pointer(parent, index)?;
        // SAFETY: The parent frame is an owned page-table frame.
        let mut value = unsafe { entry.read_volatile() };
        if value & PageFlags::PRESENT.bits() != 0 {
            if value & PageFlags::HUGE.bits() != 0 {
                return Err(PagingError::HugePageConflict);
            }
            if user && value & PageFlags::USER.bits() == 0 {
                value |= PageFlags::USER.bits();
                // SAFETY: Promoting an intermediate entry to user-visible is
                // required for a user leaf and preserves its child address.
                unsafe {
                    entry.write_volatile(value);
                }
            }
            return child_frame(value);
        }
        let child = self.allocate_table()?;
        let flags = PageFlags::PRESENT
            .union(PageFlags::WRITABLE)
            .union(if user {
                PageFlags::USER
            } else {
                PageFlags::empty()
            });
        // SAFETY: `entry` is absent and the newly allocated child is zeroed.
        unsafe {
            entry.write_volatile(child.start_address() | flags.bits());
        }
        Ok(child)
    }

    fn allocate_table(&mut self) -> Result<PhysicalFrame, PagingError> {
        let frame = self
            .pool
            .allocate()
            .ok_or(PagingError::BootstrapPoolExhausted)?;
        // SAFETY: The pool transfers exclusive ownership of the frame to this
        // page-table hierarchy.
        unsafe {
            zero_table(frame)?;
        }
        self.allocated_tables = self.allocated_tables.saturating_add(1);
        Ok(frame)
    }
}

/// Builds and activates a SanjuOS-owned page-table hierarchy.
///
/// The transition retains a bounded identity window because the current EFI
/// stub kernel executes at its load address. A higher-half physical direct map
/// is installed in parallel. Kernel PE sections receive page-granular W^X
/// permissions before CR3 is replaced.
///
/// # Errors
///
/// Returns an error when the processor lacks execute-disable support, boot
/// metadata is invalid, the bounded physical window cannot contain required
/// memory, PE section permissions are unsafe, or the bootstrap pool is
/// exhausted.
///
/// # Safety
///
/// The caller must run on the bootstrap processor after `ExitBootServices`
/// with interrupts disabled. The retained memory map, boot image, and page-table
/// bootstrap pool must remain valid for the lifetime of the new hierarchy.
pub unsafe fn take_page_table_ownership<'a>(
    pool: &'a mut PageTableBootstrapPool<PAGE_TABLE_BOOTSTRAP_FRAMES>,
    boot_info: &BootInfo,
) -> Result<(HardwarePageTable<'a>, HardwarePagingReport), PagingError> {
    if read_cr4() & CR4_FIVE_LEVEL_PAGING != 0 || !enable_execute_disable() {
        return Err(PagingError::UnsupportedCpuFeature);
    }
    enable_kernel_write_protect();
    let old_root = read_cr3();
    let layout = VirtualMemoryLayout::sanjuos();
    let mapping_limit = required_physical_limit(boot_info)?;
    let mut manager = HardwarePageTable::new(pool)?;
    let data_flags = PageFlags::WRITABLE
        .union(PageFlags::NO_EXECUTE)
        .union(PageFlags::GLOBAL);

    let mut physical = 0_u64;
    while physical < mapping_limit {
        manager.map_huge_2m(physical, physical, data_flags)?;
        let direct = PHYSICAL_DIRECT_MAP_START
            .checked_add(physical)
            .ok_or(PagingError::AddressOverflow)?;
        manager.map_huge_2m(direct, physical, data_flags)?;
        physical = physical
            .checked_add(PAGE_SIZE_2M)
            .ok_or(PagingError::AddressOverflow)?;
    }
    manager.mapped_physical_bytes = mapping_limit;

    let image_sections_verified = manager.harden_loaded_image(boot_info.kernel_image)?;
    let write_xor_execute_enforced = manager.verify_image_wx(boot_info.kernel_image)?;
    let translation_test_passed = manager
        .translate(boot_info.kernel_image.start)
        .is_some_and(|translated| translated == boot_info.kernel_image.start)
        && layout
            .direct_map_address(boot_info.kernel_image.start)
            .and_then(|address| manager.translate(address))
            == Some(boot_info.kernel_image.start);

    let new_root = manager.root.start_address();
    // SAFETY: The hierarchy maps the executing image, active stack, descriptor
    // tables, allocator storage, framebuffer window, and its own frames.
    unsafe {
        switch_cr3_and_flush_global(new_root);
    }
    let checkpoint_rip = current_instruction_pointer();
    let checkpoint_rsp = current_stack_pointer();
    let transition_checkpoint_passed = read_cr3() == new_root
        && manager.translate(checkpoint_rip) == Some(checkpoint_rip)
        && manager.translate(checkpoint_rsp) == Some(checkpoint_rsp);
    let direct_map_active = layout
        .direct_map_address(boot_info.kernel_image.start)
        .and_then(|address| manager.translate(address))
        == Some(boot_info.kernel_image.start);

    let report = HardwarePagingReport {
        old_root,
        new_root,
        mapped_physical_bytes: mapping_limit,
        table_frames_used: manager.allocated_tables(),
        layout_frozen: layout.is_frozen(),
        fresh_root_active: old_root != new_root && read_cr3() == new_root,
        direct_map_active,
        mapper_active: true,
        translation_test_passed,
        map_unmap_test_passed: false,
        protection_test_passed: false,
        write_xor_execute_enforced,
        guard_pages_active: false,
        image_sections_verified,
        transition_checkpoint_passed,
    };
    Ok((manager, report))
}

impl HardwarePageTable<'_> {
    fn harden_loaded_image(&mut self, image: PhysicalRange) -> Result<bool, PagingError> {
        if image.is_empty() || !image.start.is_multiple_of(PAGE_SIZE) {
            return Err(PagingError::UnsupportedImage);
        }
        let end = image.end_exclusive().ok_or(PagingError::AddressOverflow)?;
        let split_start = align_down(image.start, PAGE_SIZE_2M);
        let split_end = align_up(end, PAGE_SIZE_2M)?;
        let mut chunk = split_start;
        while chunk < split_end {
            self.split_huge_2m(chunk)?;
            let direct = PHYSICAL_DIRECT_MAP_START
                .checked_add(chunk)
                .ok_or(PagingError::AddressOverflow)?;
            self.split_huge_2m(direct)?;
            chunk = chunk
                .checked_add(PAGE_SIZE_2M)
                .ok_or(PagingError::AddressOverflow)?;
        }

        let read_only_nx = PageFlags::NO_EXECUTE.union(PageFlags::GLOBAL);
        let mut page = image.start;
        while page < end {
            self.protect_page(VirtualPage::containing(page), read_only_nx)?;
            let direct = PHYSICAL_DIRECT_MAP_START
                .checked_add(page)
                .ok_or(PagingError::AddressOverflow)?;
            self.protect_page(VirtualPage::containing(direct), read_only_nx)?;
            page = page
                .checked_add(PAGE_SIZE)
                .ok_or(PagingError::AddressOverflow)?;
        }

        let sections = parse_image_sections(image)?;
        let mut saw_executable = false;
        let mut saw_writable = false;
        for section in sections.iter().flatten() {
            if section.virtual_size == 0 {
                continue;
            }
            let executable = section.characteristics & IMAGE_SCN_MEM_EXECUTE != 0;
            let writable = section.characteristics & IMAGE_SCN_MEM_WRITE != 0;
            let readable = section.characteristics & IMAGE_SCN_MEM_READ != 0;
            if executable && writable {
                return Err(PagingError::PermissionConflict);
            }
            if !readable && !executable && !writable {
                continue;
            }
            saw_executable |= executable;
            saw_writable |= writable;
            let section_start = image
                .start
                .checked_add(section.virtual_address)
                .ok_or(PagingError::AddressOverflow)?;
            let section_end = section_start
                .checked_add(section.virtual_size)
                .ok_or(PagingError::AddressOverflow)?;
            if section_start < image.start || section_end > end {
                return Err(PagingError::UnsupportedImage);
            }
            let flags = if writable {
                PageFlags::WRITABLE
                    .union(PageFlags::NO_EXECUTE)
                    .union(PageFlags::GLOBAL)
            } else if executable {
                PageFlags::GLOBAL
            } else {
                PageFlags::NO_EXECUTE.union(PageFlags::GLOBAL)
            };
            let mut section_page = align_down(section_start, PAGE_SIZE);
            let section_page_end = align_up(section_end, PAGE_SIZE)?;
            while section_page < section_page_end {
                self.protect_page(VirtualPage::containing(section_page), flags)?;
                let direct = PHYSICAL_DIRECT_MAP_START
                    .checked_add(section_page)
                    .ok_or(PagingError::AddressOverflow)?;
                self.protect_page(VirtualPage::containing(direct), flags)?;
                section_page = section_page
                    .checked_add(PAGE_SIZE)
                    .ok_or(PagingError::AddressOverflow)?;
            }
        }
        Ok(saw_executable && saw_writable)
    }

    fn verify_image_wx(&self, image: PhysicalRange) -> Result<bool, PagingError> {
        let end = image.end_exclusive().ok_or(PagingError::AddressOverflow)?;
        let mut saw_executable = false;
        let mut saw_writable = false;
        let mut page = image.start;
        while page < end {
            let identity_flags = self.flags_for(page).ok_or(PagingError::NotMapped)?;
            let direct_address = PHYSICAL_DIRECT_MAP_START
                .checked_add(page)
                .ok_or(PagingError::AddressOverflow)?;
            let direct_flags = self
                .flags_for(direct_address)
                .ok_or(PagingError::NotMapped)?;
            identity_flags.validate()?;
            direct_flags.validate()?;
            if identity_flags.is_writable() != direct_flags.is_writable()
                || identity_flags.is_executable() != direct_flags.is_executable()
            {
                return Ok(false);
            }
            saw_executable |= identity_flags.is_executable();
            saw_writable |= identity_flags.is_writable();
            page = page
                .checked_add(PAGE_SIZE)
                .ok_or(PagingError::AddressOverflow)?;
        }
        Ok(saw_executable && saw_writable)
    }
}

/// Reserves every page-table frame reachable from the inherited CR3 before the
/// general physical allocator can reuse it. Huge-page leaves are not table
/// frames and are therefore not added to the reservation queue.
///
/// # Errors
///
/// Returns an error for a corrupt or unexpectedly large inherited hierarchy,
/// an invalid physical frame, or a reservation failure.
///
/// # Safety
///
/// Must run while the inherited hierarchy is still active and identity
/// accessible. No concurrent page-table mutation may occur.
pub unsafe fn reserve_inherited_page_tables(
    allocator: &mut FrameAllocator<'_>,
) -> Result<usize, PagingError> {
    if read_cr4() & CR4_FIVE_LEVEL_PAGING != 0 {
        return Err(PagingError::UnsupportedCpuFeature);
    }
    let root = read_cr3();
    let root_frame =
        PhysicalFrame::from_start_address(root).ok_or(PagingError::CorruptHierarchy)?;
    let mut frames = [PhysicalFrame::ZERO; MAX_INHERITED_TABLE_FRAMES];
    let mut levels = [0_u8; MAX_INHERITED_TABLE_FRAMES];
    let mut head = 0_usize;
    let mut tail = 1_usize;
    frames[0] = root_frame;

    while head < tail {
        let frame = frames[head];
        let level = levels[head];
        head += 1;
        match allocator.reserve_range(PhysicalRange {
            start: frame.start_address(),
            length: PAGE_SIZE,
        }) {
            Ok(()) | Err(MemoryError::AlreadyReserved) => {}
            Err(_) => return Err(PagingError::CorruptHierarchy),
        }
        if level == 3 {
            continue;
        }
        for index in 0..ENTRIES_PER_TABLE {
            let value = table_entry_value(frame, index)?;
            if value & PageFlags::PRESENT.bits() == 0 || value & PageFlags::HUGE.bits() != 0 {
                continue;
            }
            let child = child_frame(value)?;
            if frames[..tail].contains(&child) {
                continue;
            }
            if tail == frames.len() {
                return Err(PagingError::MappingTableFull);
            }
            frames[tail] = child;
            levels[tail] = level.saturating_add(1);
            tail += 1;
        }
    }
    Ok(tail)
}

#[must_use]
pub fn active_page_table_root() -> u64 {
    read_cr3()
}

/// Promotes an existing range to Ring 3 visibility under the active SanjuOS
/// hierarchy. Executable user pages are made read-only; user stacks are made
/// writable and NX.
///
/// # Safety
///
/// The range must belong exclusively to the current process and remain mapped
/// for the entire Ring 3 execution interval.
pub unsafe fn mark_user_range(start: u64, length: usize, executable: bool) -> bool {
    if length == 0 {
        return false;
    }
    let Ok(length) = u64::try_from(length) else {
        return false;
    };
    let Some(end) = start.checked_add(length.saturating_sub(1)) else {
        return false;
    };
    let mut page = align_down(start, PAGE_SIZE);
    let last = align_down(end, PAGE_SIZE);
    loop {
        // SAFETY: The active root is identity-accessible and the caller owns
        // the supplied user range.
        if !unsafe { mark_active_user_page(page, executable) } {
            return false;
        }
        if page == last {
            return true;
        }
        let Some(next) = page.checked_add(PAGE_SIZE) else {
            return false;
        };
        page = next;
    }
}

#[allow(clippy::cast_ptr_alignment)]
unsafe fn mark_active_user_page(address: u64, executable: bool) -> bool {
    let indices = PageTableIndices::from_address(address);
    let mut table = match PhysicalFrame::from_start_address(read_cr3()) {
        Some(frame) => frame,
        None => return false,
    };
    for (level, index) in [
        indices.pml4,
        indices.pdpt,
        indices.page_directory,
        indices.page_table,
    ]
    .into_iter()
    .enumerate()
    {
        let Ok(pointer) = table_entry_pointer(table, index) else {
            return false;
        };
        // SAFETY: The page-table frame is present under the active root.
        let mut value = unsafe { pointer.read_volatile() };
        if value & PageFlags::PRESENT.bits() == 0 {
            return false;
        }
        value |= PageFlags::USER.bits();
        let huge_leaf = level >= 1 && value & PageFlags::HUGE.bits() != 0;
        if huge_leaf {
            return false;
        }
        let is_leaf = level == 3;
        if is_leaf {
            if executable {
                value &= !PageFlags::NO_EXECUTE.bits();
                value &= !PageFlags::WRITABLE.bits();
            } else {
                value |= PageFlags::NO_EXECUTE.bits();
                value |= PageFlags::WRITABLE.bits();
            }
        }
        // SAFETY: Only permission bits are changed while preserving the target.
        unsafe {
            pointer.write_volatile(value);
        }
        if is_leaf {
            invalidate_page(address);
            let Some(physical_page) = translate_from_root(read_cr3(), address)
                .map(|physical| align_down(physical, PAGE_SIZE))
            else {
                return false;
            };
            // Keep the kernel-only direct alias non-executable and never
            // writable when the user identity alias is executable. This
            // enforces W^X across aliases of the same physical page.
            if !update_direct_alias_permissions(physical_page, executable) {
                return false;
            }
            return true;
        }
        let Some(child) = PhysicalFrame::from_start_address(value & ADDRESS_MASK) else {
            return false;
        };
        table = child;
    }
    false
}

fn update_direct_alias_permissions(physical_page: u64, executable_alias: bool) -> bool {
    let Some(direct_address) = PHYSICAL_DIRECT_MAP_START.checked_add(physical_page) else {
        return false;
    };
    let indices = PageTableIndices::from_address(direct_address);
    let mut table = match PhysicalFrame::from_start_address(read_cr3()) {
        Some(frame) => frame,
        None => return false,
    };
    for (level, index) in [
        indices.pml4,
        indices.pdpt,
        indices.page_directory,
        indices.page_table,
    ]
    .into_iter()
    .enumerate()
    {
        let Ok(pointer) = table_entry_pointer(table, index) else {
            return false;
        };
        // SAFETY: The direct-map hierarchy belongs to the active SanjuOS root.
        let mut value = unsafe { pointer.read_volatile() };
        if value & PageFlags::PRESENT.bits() == 0 {
            return false;
        }
        let huge_leaf = level >= 1 && value & PageFlags::HUGE.bits() != 0;
        if huge_leaf {
            return false;
        }
        let is_leaf = level == 3;
        if is_leaf {
            value &= !PageFlags::USER.bits();
            value |= PageFlags::NO_EXECUTE.bits();
            if executable_alias {
                value &= !PageFlags::WRITABLE.bits();
            } else {
                value |= PageFlags::WRITABLE.bits();
            }
            // SAFETY: The target address is preserved; only direct-alias
            // permissions are narrowed or restored.
            unsafe {
                pointer.write_volatile(value);
            }
            invalidate_page(direct_address);
            return true;
        }
        let Some(child) = PhysicalFrame::from_start_address(value & ADDRESS_MASK) else {
            return false;
        };
        table = child;
    }
    false
}

fn translate_from_root(root: u64, virtual_address: u64) -> Option<u64> {
    if root == 0 || !is_canonical(virtual_address) {
        return None;
    }
    let indices = PageTableIndices::from_address(virtual_address);
    let root_frame = PhysicalFrame::from_start_address(root)?;
    let pml4 = table_entry_value(root_frame, indices.pml4).ok()?;
    let pdpt_frame = child_frame(pml4).ok()?;
    let pdpt = table_entry_value(pdpt_frame, indices.pdpt).ok()?;
    if pdpt & PageFlags::HUGE.bits() != 0 {
        let physical = pdpt & 0x000f_ffff_c000_0000;
        return physical.checked_add(virtual_address & (PAGE_SIZE_1G_LOCAL - 1));
    }
    let directory_frame = child_frame(pdpt).ok()?;
    let directory = table_entry_value(directory_frame, indices.page_directory).ok()?;
    if directory & PageFlags::HUGE.bits() != 0 {
        let physical = directory & HUGE_ADDRESS_MASK;
        return physical.checked_add(virtual_address & HUGE_PAGE_OFFSET_MASK);
    }
    let table_frame = child_frame(directory).ok()?;
    let page = table_entry_value(table_frame, indices.page_table).ok()?;
    if page & PageFlags::PRESENT.bits() == 0 {
        return None;
    }
    (page & ADDRESS_MASK).checked_add(virtual_address & PAGE_OFFSET_MASK)
}

fn flags_from_root(root: u64, virtual_address: u64) -> Option<PageFlags> {
    if root == 0 || !is_canonical(virtual_address) {
        return None;
    }
    let indices = PageTableIndices::from_address(virtual_address);
    let root_frame = PhysicalFrame::from_start_address(root)?;
    let pml4 = table_entry_value(root_frame, indices.pml4).ok()?;
    let pdpt_frame = child_frame(pml4).ok()?;
    let pdpt = table_entry_value(pdpt_frame, indices.pdpt).ok()?;
    if pdpt & PageFlags::HUGE.bits() != 0 {
        return Some(PageFlags::from_bits(pdpt & ENTRY_FLAG_MASK));
    }
    let directory_frame = child_frame(pdpt).ok()?;
    let directory = table_entry_value(directory_frame, indices.page_directory).ok()?;
    if directory & PageFlags::HUGE.bits() != 0 {
        return Some(PageFlags::from_bits(directory & ENTRY_FLAG_MASK));
    }
    let table_frame = child_frame(directory).ok()?;
    let page = table_entry_value(table_frame, indices.page_table).ok()?;
    if page & PageFlags::PRESENT.bits() == 0 {
        None
    } else {
        Some(PageFlags::from_bits(page & ENTRY_FLAG_MASK))
    }
}

const PAGE_SIZE_1G_LOCAL: u64 = 1024 * 1024 * 1024;

fn required_physical_limit(boot_info: &BootInfo) -> Result<u64, PagingError> {
    let mut highest = highest_memory_map_end(boot_info.memory_map)?;
    for range in [
        boot_info.kernel_image,
        boot_info.boot_image,
        boot_info.boot_info_range,
        boot_info.framebuffer.physical_range(),
        PhysicalRange {
            start: boot_info.active_page_table_root,
            length: PAGE_SIZE,
        },
    ] {
        if let Some(end) = range.end_exclusive() {
            highest = highest.max(end);
        }
    }
    for address in [boot_info.acpi_rsdp, boot_info.smbios_entry] {
        if address.is_present() {
            highest = highest.max(
                address
                    .address
                    .checked_add(PAGE_SIZE)
                    .ok_or(PagingError::AddressOverflow)?,
            );
        }
    }
    if highest == 0 || highest > MAX_BOOT_PHYSICAL_BYTES {
        return Err(PagingError::AddressOverflow);
    }
    align_up(highest, PAGE_SIZE_2M)
}

fn highest_memory_map_end(map: MemoryMapInfo) -> Result<u64, PagingError> {
    if !map.is_structurally_valid() || map.descriptor_size < 40 {
        return Err(PagingError::CorruptHierarchy);
    }
    let count = map
        .descriptor_count_usize()
        .ok_or(PagingError::AddressOverflow)?;
    let descriptor_size = map
        .descriptor_size_usize()
        .ok_or(PagingError::AddressOverflow)?;
    let base = map
        .buffer_address_usize()
        .ok_or(PagingError::AddressOverflow)?;
    let map_size = map.map_size_usize().ok_or(PagingError::AddressOverflow)?;
    let mut highest = 0_u64;
    for index in 0..count {
        let offset = index
            .checked_mul(descriptor_size)
            .ok_or(PagingError::AddressOverflow)?;
        if offset.checked_add(40).ok_or(PagingError::AddressOverflow)? > map_size {
            return Err(PagingError::CorruptHierarchy);
        }
        let descriptor = base
            .checked_add(offset)
            .ok_or(PagingError::AddressOverflow)? as *const u8;
        // SAFETY: The retained UEFI memory-map bounds were validated above.
        let memory_type = unsafe { descriptor.cast::<u32>().read_unaligned() };
        if !matches!(memory_type, 1..=7 | 9 | 10 | 14) {
            continue;
        }
        // SAFETY: Offset 8 is UEFI `PhysicalStart`.
        let start = unsafe { descriptor.add(8).cast::<u64>().read_unaligned() };
        // SAFETY: Offset 24 is UEFI `NumberOfPages`.
        let pages = unsafe { descriptor.add(24).cast::<u64>().read_unaligned() };
        let length = pages
            .checked_mul(PAGE_SIZE)
            .ok_or(PagingError::AddressOverflow)?;
        highest = highest.max(
            start
                .checked_add(length)
                .ok_or(PagingError::AddressOverflow)?,
        );
    }
    Ok(highest)
}

fn parse_image_sections(
    image: PhysicalRange,
) -> Result<[Option<ImageSection>; MAX_IMAGE_SECTIONS], PagingError> {
    let image_base = usize::try_from(image.start).map_err(|_| PagingError::AddressOverflow)?;
    let image_size = usize::try_from(image.length).map_err(|_| PagingError::AddressOverflow)?;
    if image_size < 0x100 {
        return Err(PagingError::UnsupportedImage);
    }
    let base = image_base as *const u8;
    // SAFETY: The loaded-image range is retained and readable after firmware exit.
    let dos = unsafe { base.cast::<u16>().read_unaligned() };
    if dos != IMAGE_DOS_SIGNATURE {
        return Err(PagingError::UnsupportedImage);
    }
    // SAFETY: The DOS header is at least 0x40 bytes for a valid PE image.
    let pe_offset = usize::try_from(unsafe { base.add(0x3c).cast::<u32>().read_unaligned() })
        .map_err(|_| PagingError::AddressOverflow)?;
    let signature_end = pe_offset
        .checked_add(4 + IMAGE_FILE_HEADER_SIZE)
        .ok_or(PagingError::AddressOverflow)?;
    if signature_end > image_size {
        return Err(PagingError::UnsupportedImage);
    }
    // SAFETY: The PE signature lies within the loaded-image bounds.
    let signature = unsafe { base.add(pe_offset).cast::<u32>().read_unaligned() };
    if signature != IMAGE_NT_SIGNATURE {
        return Err(PagingError::UnsupportedImage);
    }
    // SAFETY: The COFF fields are within `signature_end` checked above.
    let section_count =
        usize::from(unsafe { base.add(pe_offset + 6).cast::<u16>().read_unaligned() });
    // SAFETY: Same validated COFF header.
    let optional_header_size =
        usize::from(unsafe { base.add(pe_offset + 20).cast::<u16>().read_unaligned() });
    if section_count == 0 || section_count > MAX_IMAGE_SECTIONS {
        return Err(PagingError::UnsupportedImage);
    }
    let section_table = pe_offset
        .checked_add(4 + IMAGE_FILE_HEADER_SIZE)
        .and_then(|value| value.checked_add(optional_header_size))
        .ok_or(PagingError::AddressOverflow)?;
    let section_bytes = section_count
        .checked_mul(IMAGE_SECTION_HEADER_SIZE)
        .ok_or(PagingError::AddressOverflow)?;
    if section_table
        .checked_add(section_bytes)
        .ok_or(PagingError::AddressOverflow)?
        > image_size
    {
        return Err(PagingError::UnsupportedImage);
    }

    let mut sections = [None; MAX_IMAGE_SECTIONS];
    for (index, slot) in sections.iter_mut().take(section_count).enumerate() {
        let offset = section_table + index * IMAGE_SECTION_HEADER_SIZE;
        // SAFETY: Every section header is within the validated section table.
        let virtual_size =
            u64::from(unsafe { base.add(offset + 8).cast::<u32>().read_unaligned() });
        // SAFETY: Same validated section-header bounds.
        let virtual_address =
            u64::from(unsafe { base.add(offset + 12).cast::<u32>().read_unaligned() });
        // SAFETY: Same validated section-header bounds.
        let raw_size = u64::from(unsafe { base.add(offset + 16).cast::<u32>().read_unaligned() });
        // SAFETY: Same validated section-header bounds.
        let characteristics = unsafe { base.add(offset + 36).cast::<u32>().read_unaligned() };
        *slot = Some(ImageSection {
            virtual_address,
            virtual_size: virtual_size.max(raw_size),
            characteristics,
        });
    }
    Ok(sections)
}

fn validate_leaf_mapping(
    virtual_address: u64,
    physical_address: u64,
    flags: PageFlags,
    page_size: u64,
) -> Result<(), PagingError> {
    if !is_canonical(virtual_address) {
        return Err(PagingError::NonCanonicalAddress);
    }
    if !virtual_address.is_multiple_of(page_size) || !physical_address.is_multiple_of(page_size) {
        return Err(PagingError::Unaligned);
    }
    flags.validate()
}

fn child_frame(entry: u64) -> Result<PhysicalFrame, PagingError> {
    if entry & PageFlags::PRESENT.bits() == 0 {
        return Err(PagingError::NotMapped);
    }
    PhysicalFrame::from_start_address(entry & ADDRESS_MASK).ok_or(PagingError::CorruptHierarchy)
}

fn table_entry_value(frame: PhysicalFrame, index: usize) -> Result<u64, PagingError> {
    let pointer = table_entry_pointer(frame, index)?;
    // SAFETY: `pointer` is within the identity-accessible page-table frame.
    Ok(unsafe { pointer.read_volatile() })
}

fn table_entry_pointer(frame: PhysicalFrame, index: usize) -> Result<*mut u64, PagingError> {
    if index >= ENTRIES_PER_TABLE {
        return Err(PagingError::CorruptHierarchy);
    }
    let address =
        usize::try_from(frame.start_address()).map_err(|_| PagingError::AddressOverflow)?;
    let table = address as *mut u64;
    // SAFETY: `index` is checked against the 512-entry page-table size.
    Ok(unsafe { table.add(index) })
}

unsafe fn zero_table(frame: PhysicalFrame) -> Result<(), PagingError> {
    let address =
        usize::try_from(frame.start_address()).map_err(|_| PagingError::AddressOverflow)?;
    let table = address as *mut u64;
    // SAFETY: The caller exclusively owns a writable 4 KiB page-table frame.
    unsafe {
        ptr::write_bytes(table, 0, ENTRIES_PER_TABLE);
    }
    Ok(())
}

fn enable_execute_disable() -> bool {
    // SAFETY: CPUID is available in x86-64 mode and does not alter privileged
    // execution state.
    let maximum = unsafe { core::arch::x86_64::__cpuid(CPUID_EXTENDED_MAXIMUM) }.eax;
    if maximum < CPUID_EXTENDED_FEATURES {
        return false;
    }
    // SAFETY: Same CPUID contract as above.
    let features = unsafe { core::arch::x86_64::__cpuid(CPUID_EXTENDED_FEATURES) };
    if features.edx & CPUID_NX_BIT == 0 {
        return false;
    }
    let efer = read_msr(IA32_EFER);
    // SAFETY: NX is advertised by CPUID and setting EFER.NXE is architecturally
    // valid before installing page entries that use the execute-disable bit.
    unsafe {
        write_msr(IA32_EFER, efer | EFER_NXE);
    }
    read_msr(IA32_EFER) & EFER_NXE != 0
}

fn enable_kernel_write_protect() {
    let value = read_cr0();
    if value & CR0_WRITE_PROTECT == 0 {
        // SAFETY: Setting CR0.WP strengthens supervisor write protection and
        // preserves every other control-register bit.
        unsafe {
            write_cr0(value | CR0_WRITE_PROTECT);
        }
    }
}

fn read_msr(index: u32) -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY: The selected architectural MSR is readable at Ring 0.
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") index,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

unsafe fn write_msr(index: u32, value: u64) {
    let low = u32::try_from(value & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    let high = u32::try_from(value >> 32).unwrap_or(u32::MAX);
    // SAFETY: The caller validates the architectural MSR and value.
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") index,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

fn read_cr0() -> u64 {
    let value: u64;
    // SAFETY: Reading CR0 is side-effect free at Ring 0.
    unsafe {
        asm!(
            "mov {value}, cr0",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

unsafe fn write_cr0(value: u64) {
    // SAFETY: The caller preserves required architectural control bits.
    unsafe {
        asm!(
            "mov cr0, {value}",
            value = in(reg) value,
            options(nostack, preserves_flags)
        );
    }
}

fn read_cr4() -> u64 {
    let value: u64;
    // SAFETY: Reading CR4 is side-effect free at Ring 0.
    unsafe {
        asm!(
            "mov {value}, cr4",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

unsafe fn write_cr4(value: u64) {
    // SAFETY: The caller preserves every required CR4 feature bit.
    unsafe {
        asm!(
            "mov cr4, {value}",
            value = in(reg) value,
            options(nostack, preserves_flags)
        );
    }
}

unsafe fn switch_cr3_and_flush_global(root: u64) {
    let cr4 = read_cr4();
    let global_pages_enabled = cr4 & CR4_PAGE_GLOBAL_ENABLE != 0;
    if global_pages_enabled {
        // SAFETY: Temporarily clearing PGE invalidates global translations;
        // the original CR4 value is restored immediately after the root switch.
        unsafe {
            write_cr4(cr4 & !CR4_PAGE_GLOBAL_ENABLE);
        }
    }
    // SAFETY: The caller supplied a complete, aligned PML4 root.
    unsafe {
        write_cr3(root);
    }
    if global_pages_enabled {
        // SAFETY: Restores the exact pre-transition CR4 feature set.
        unsafe {
            write_cr4(cr4);
        }
    }
}

fn read_cr3() -> u64 {
    let value: u64;
    // SAFETY: Reading CR3 is side-effect free at Ring 0.
    unsafe {
        asm!(
            "mov {value}, cr3",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value & ADDRESS_MASK
}

unsafe fn write_cr3(root: u64) {
    // SAFETY: The caller guarantees `root` points to a complete PML4 that maps
    // the executing instruction, stack, and architecture data structures.
    unsafe {
        asm!(
            "mov cr3, {root}",
            root = in(reg) root,
            options(nostack, preserves_flags)
        );
    }
}

fn current_instruction_pointer() -> u64 {
    let value: u64;
    // SAFETY: LEA only materializes the address of the local label.
    unsafe {
        asm!(
            "lea {value}, [rip + 2f]",
            "2:",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

fn current_stack_pointer() -> u64 {
    let value: u64;
    // SAFETY: Copying RSP to a general register has no side effects.
    unsafe {
        asm!(
            "mov {value}, rsp",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

fn invalidate_if_active(root: u64, virtual_address: u64) {
    if read_cr3() == root {
        invalidate_page(virtual_address);
    }
}

fn invalidate_page(virtual_address: u64) {
    // SAFETY: INVLPG invalidates only the current address-space translation for
    // the supplied canonical address.
    unsafe {
        asm!(
            "invlpg [{address}]",
            address = in(reg) virtual_address,
            options(nostack, preserves_flags)
        );
    }
}

fn flush_all_if_active(root: u64) {
    if read_cr3() == root {
        // SAFETY: Reloading the currently active CR3 flushes non-global TLB
        // entries and preserves the same valid hierarchy.
        unsafe {
            write_cr3(root);
        }
    }
}

const fn align_down(value: u64, alignment: u64) -> u64 {
    value & !(alignment - 1)
}

fn align_up(value: u64, alignment: u64) -> Result<u64, PagingError> {
    value
        .checked_add(alignment - 1)
        .map(|candidate| candidate & !(alignment - 1))
        .ok_or(PagingError::AddressOverflow)
}

#[must_use]
pub const fn kernel_heap_probe_page() -> VirtualPage {
    VirtualPage::containing(KERNEL_HEAP_START)
}

#[must_use]
pub const fn kernel_guard_base() -> u64 {
    KERNEL_STACK_START
}
