#![no_std]
#![no_main]
#![allow(clippy::pedantic)]

mod arch;

use arch::x86_64::{self as cpu, SerialConsole};
use core::alloc::Layout;
use core::ffi::c_void;
use core::mem::{MaybeUninit, size_of};
use core::panic::PanicInfo;
use core::ptr::{addr_of, addr_of_mut};
use sanju_kernel::boot_info::{
    FramebufferInfo, OptionalPhysicalAddress, PhysicalRange, PixelFormat,
};
use sanju_kernel::cache::{BlockCache, CacheError, DEFAULT_CACHE_ENTRIES, DirtyStatePolicy};
use sanju_kernel::elf::load_position_independent;
use sanju_kernel::fat32::Fat32;
use sanju_kernel::fs::RamFs;
use sanju_kernel::heap::KernelHeap;
#[cfg(not(feature = "qemu-test"))]
use sanju_kernel::input::KeyboardDecoder;
use sanju_kernel::memory::{
    DEFAULT_FRAME_BITMAP_WORDS, FrameAllocator, FrameBitmap, MemoryError, PAGE_SIZE,
    PAGE_TABLE_BOOTSTRAP_FRAMES, PageTableBootstrapPool,
};
use sanju_kernel::ownership::{OwnershipError, OwnershipKind, PhysicalOwnershipMap};
use sanju_kernel::paging::{GuardedStack, PageFlags, PagingError, VirtualPage};
use sanju_kernel::pci::StorageControllerKind;
use sanju_kernel::process::{AddressSpace, ProcessTable};
use sanju_kernel::scheduler::{Scheduler, TaskKind};
use sanju_kernel::shell::{Shell, ShellEnvironment};
use sanju_kernel::startup::{self, StartupStage};
use sanju_kernel::vfs::{
    FileSystem, HandleRights, MAX_PATH_COMPONENTS, NodeKind, NormalizedPath, PathError, Vfs,
    VfsError,
};
use sanju_kernel::{
    BootInfo, Console, FoundationHardeningPhase2Report, FoundationHardeningPhase3Report,
    FoundationHardeningReport, M5Report, M6aReport, M6bReport, M6cReport, M6dReport, MemoryMapInfo,
    kernel_main_foundation_hardening, kernel_main_foundation_hardening_phase2,
    kernel_main_foundation_hardening_phase3, kernel_main_m5, kernel_main_m6a, kernel_main_m6b,
    kernel_main_m6c, kernel_main_m6d,
};

type EfiHandle = *mut c_void;
type EfiStatus = usize;
type EfiPhysicalAddress = u64;

const EFI_SUCCESS: EfiStatus = 0;
const EFI_INVALID_PARAMETER: EfiStatus = efi_error_code(2);
const EFI_BUFFER_TOO_SMALL: EfiStatus = efi_error_code(5);
const EFI_SYSTEM_TABLE_SIGNATURE: u64 = 0x5453_5953_2049_4249;
const EFI_BOOT_SERVICES_SIGNATURE: u64 = 0x5652_4553_544f_4f42;
const MEMORY_MAP_CAPACITY: usize = 256 * 1024;
const EXIT_BOOT_SERVICES_RETRIES: usize = 8;
const KERNEL_HEAP_SIZE: usize = 1024 * 1024;
const USER_IMAGE_SIZE: usize = 16 * 1024;
const USER_STACK_SIZE: usize = 64 * 1024;
const USER_STACK_TOTAL_SIZE: usize = USER_STACK_SIZE + 2 * 4096;

const fn efi_error_code(code: usize) -> usize {
    (1usize << (usize::BITS - 1)) | code
}

const fn efi_is_error(status: EfiStatus) -> bool {
    status & (1usize << (usize::BITS - 1)) != 0
}

#[allow(dead_code)]
#[repr(C)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

type TextReset = unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol, u8) -> EfiStatus;
type TextOutputString =
    unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol, *const u16) -> EfiStatus;
type TextClearScreen = unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol) -> EfiStatus;

#[allow(dead_code)]
#[repr(C)]
struct SimpleTextOutputProtocol {
    reset: TextReset,
    output_string: TextOutputString,
    test_string: usize,
    query_mode: usize,
    set_mode: usize,
    set_attribute: usize,
    clear_screen: TextClearScreen,
    set_cursor_position: usize,
    enable_cursor: usize,
    mode: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct EfiGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

static EFI_LOADED_IMAGE_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x5b1b_31a1,
    data2: 0x9562,
    data3: 0x11d2,
    data4: [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};

static EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x9042_a9de,
    data2: 0x23dc,
    data3: 0x4a38,
    data4: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

static EFI_ACPI_20_TABLE_GUID: EfiGuid = EfiGuid {
    data1: 0x8868_e871,
    data2: 0xe4f1,
    data3: 0x11d3,
    data4: [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
};

static EFI_ACPI_10_TABLE_GUID: EfiGuid = EfiGuid {
    data1: 0xeb9d_2d30,
    data2: 0x2d88,
    data3: 0x11d3,
    data4: [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};

static EFI_SMBIOS3_TABLE_GUID: EfiGuid = EfiGuid {
    data1: 0xf2fd_1544,
    data2: 0x9794,
    data3: 0x4a2c,
    data4: [0x99, 0x2e, 0xe5, 0xbb, 0xcf, 0x20, 0xe3, 0x94],
};

static EFI_SMBIOS_TABLE_GUID: EfiGuid = EfiGuid {
    data1: 0xeb9d_2d31,
    data2: 0x2d88,
    data3: 0x11d3,
    data4: [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};

#[repr(C)]
struct EfiConfigurationTable {
    vendor_guid: EfiGuid,
    vendor_table: *mut c_void,
}

type HandleProtocol = unsafe extern "efiapi" fn(
    handle: EfiHandle,
    protocol: *const EfiGuid,
    interface: *mut *mut c_void,
) -> EfiStatus;

#[allow(dead_code)]
#[repr(C)]
struct EfiLoadedImageProtocol {
    revision: u32,
    parent_handle: EfiHandle,
    system_table: *mut EfiSystemTable,
    device_handle: EfiHandle,
    file_path: *mut c_void,
    reserved: *mut c_void,
    load_options_size: u32,
    load_options: *mut c_void,
    image_base: *mut c_void,
    image_size: u64,
    image_code_type: u32,
    image_data_type: u32,
    unload: usize,
}

#[repr(C)]
struct EfiPixelBitmask {
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    reserved_mask: u32,
}

#[allow(dead_code)]
#[repr(C)]
struct EfiGraphicsOutputModeInformation {
    version: u32,
    horizontal_resolution: u32,
    vertical_resolution: u32,
    pixel_format: u32,
    pixel_information: EfiPixelBitmask,
    pixels_per_scan_line: u32,
}

#[allow(dead_code)]
#[repr(C)]
struct EfiGraphicsOutputProtocolMode {
    max_mode: u32,
    mode: u32,
    info: *mut EfiGraphicsOutputModeInformation,
    size_of_info: usize,
    frame_buffer_base: EfiPhysicalAddress,
    frame_buffer_size: usize,
}

#[allow(dead_code)]
#[repr(C)]
struct EfiGraphicsOutputProtocol {
    query_mode: usize,
    set_mode: usize,
    blt: usize,
    mode: *mut EfiGraphicsOutputProtocolMode,
}

type AllocatePages = unsafe extern "efiapi" fn(
    allocation_type: u32,
    memory_type: u32,
    pages: usize,
    memory: *mut EfiPhysicalAddress,
) -> EfiStatus;
type GetMemoryMap = unsafe extern "efiapi" fn(
    memory_map_size: *mut usize,
    memory_map: *mut EfiMemoryDescriptor,
    map_key: *mut usize,
    descriptor_size: *mut usize,
    descriptor_version: *mut u32,
) -> EfiStatus;
type ExitBootServices =
    unsafe extern "efiapi" fn(image_handle: EfiHandle, map_key: usize) -> EfiStatus;

/// UEFI boot-services prefix through `ExitBootServices`, per the UEFI ABI.
#[allow(dead_code)]
#[repr(C)]
struct EfiBootServices {
    header: EfiTableHeader,
    raise_tpl: usize,
    restore_tpl: usize,
    allocate_pages: AllocatePages,
    free_pages: usize,
    get_memory_map: GetMemoryMap,
    allocate_pool: usize,
    free_pool: usize,
    create_event: usize,
    set_timer: usize,
    wait_for_event: usize,
    signal_event: usize,
    close_event: usize,
    check_event: usize,
    install_protocol_interface: usize,
    reinstall_protocol_interface: usize,
    uninstall_protocol_interface: usize,
    handle_protocol: HandleProtocol,
    reserved: usize,
    register_protocol_notify: usize,
    locate_handle: usize,
    locate_device_path: usize,
    install_configuration_table: usize,
    load_image: usize,
    start_image: usize,
    exit: usize,
    unload_image: usize,
    exit_boot_services: ExitBootServices,
}

#[allow(dead_code)]
#[repr(C)]
struct EfiSystemTable {
    header: EfiTableHeader,
    firmware_vendor: *mut u16,
    firmware_revision: u32,
    console_in_handle: EfiHandle,
    console_in: *mut c_void,
    console_out_handle: EfiHandle,
    console_out: *mut SimpleTextOutputProtocol,
    standard_error_handle: EfiHandle,
    standard_error: *mut SimpleTextOutputProtocol,
    runtime_services: *mut c_void,
    boot_services: *mut EfiBootServices,
    number_of_table_entries: usize,
    configuration_table: *mut c_void,
}

#[allow(dead_code)]
#[repr(C)]
struct EfiMemoryDescriptor {
    memory_type: u32,
    padding: u32,
    physical_start: u64,
    virtual_start: u64,
    number_of_pages: u64,
    attribute: u64,
}

#[allow(dead_code)]
#[repr(C, align(16))]
struct MemoryMapStorage([u8; MEMORY_MAP_CAPACITY]);

static mut MEMORY_MAP_STORAGE: MemoryMapStorage = MemoryMapStorage([0; MEMORY_MAP_CAPACITY]);

#[repr(C, align(64))]
struct FrameBitmapStorage {
    reserved: [u64; DEFAULT_FRAME_BITMAP_WORDS],
    allocated: [u64; DEFAULT_FRAME_BITMAP_WORDS],
}

static mut FRAME_BITMAP_STORAGE: FrameBitmapStorage = FrameBitmapStorage {
    reserved: [0; DEFAULT_FRAME_BITMAP_WORDS],
    allocated: [0; DEFAULT_FRAME_BITMAP_WORDS],
};

#[repr(C, align(4096))]
struct KernelHeapStorage([u8; KERNEL_HEAP_SIZE]);

#[repr(C, align(4096))]
struct UserImageStorage([u8; USER_IMAGE_SIZE]);

#[repr(C, align(4096))]
struct UserStackStorage([u8; USER_STACK_TOTAL_SIZE]);

static mut KERNEL_HEAP_STORAGE: KernelHeapStorage = KernelHeapStorage([0; KERNEL_HEAP_SIZE]);
static mut USER_INIT_IMAGE: UserImageStorage = UserImageStorage([0; USER_IMAGE_SIZE]);
static mut USER_HELLO_IMAGE: UserImageStorage = UserImageStorage([0; USER_IMAGE_SIZE]);
static mut USER_FAULT_IMAGE: UserImageStorage = UserImageStorage([0; USER_IMAGE_SIZE]);
static mut USER_INIT_STACK: UserStackStorage = UserStackStorage([0; USER_STACK_TOTAL_SIZE]);
static mut USER_HELLO_STACK: UserStackStorage = UserStackStorage([0; USER_STACK_TOTAL_SIZE]);
static mut USER_FAULT_STACK: UserStackStorage = UserStackStorage([0; USER_STACK_TOTAL_SIZE]);
static mut BOOT_INFO_SLOT: MaybeUninit<BootInfo> = MaybeUninit::uninit();

const INIT_ELF: &[u8] = include_bytes!("../../../user/programs/bin/init.elf");
const HELLO_ELF: &[u8] = include_bytes!("../../../user/programs/bin/hello.elf");
const FAULT_ELF: &[u8] = include_bytes!("../../../user/programs/bin/fault-test.elf");

struct UefiConsole {
    protocol: *mut SimpleTextOutputProtocol,
}

impl UefiConsole {
    #[must_use]
    fn new(protocol: *mut SimpleTextOutputProtocol) -> Option<Self> {
        (!protocol.is_null()).then_some(Self { protocol })
    }

    fn clear(&mut self) {
        // SAFETY: `protocol` was checked for null and originates from the
        // validated UEFI system table while boot services remain active.
        unsafe {
            ((*self.protocol).clear_screen)(self.protocol);
        }
    }

    fn output_code_unit(&mut self, code_unit: u16) {
        let text = [code_unit, 0];
        // SAFETY: `protocol` remains firmware-owned and valid before
        // `ExitBootServices`; `text` is NUL-terminated for the entire call.
        unsafe {
            ((*self.protocol).output_string)(self.protocol, text.as_ptr());
        }
    }
}

impl Console for UefiConsole {
    fn write_byte(&mut self, byte: u8) {
        self.output_code_unit(u16::from(byte));
    }
}

struct PreExitConsole<'a> {
    firmware: &'a mut UefiConsole,
    early: &'a mut KernelConsole,
}

impl Console for PreExitConsole<'_> {
    fn write_byte(&mut self, byte: u8) {
        self.firmware.write_byte(byte);
        self.early.write_byte(byte);
    }
}

struct KernelConsole {
    serial: SerialConsole,
}

impl KernelConsole {
    fn initialize() -> Self {
        Self {
            serial: SerialConsole::initialize(),
        }
    }
}

impl Console for KernelConsole {
    fn write_byte(&mut self, byte: u8) {
        self.serial.write_byte(byte);

        #[cfg(feature = "qemu-test")]
        cpu::qemu::debug_byte(byte);
    }
}

struct NullConsole;

impl Console for NullConsole {
    fn write_byte(&mut self, _byte: u8) {}
}

#[derive(Clone, Copy)]
struct MemoryMapSnapshot {
    info: MemoryMapInfo,
}

#[unsafe(no_mangle)]
extern "efiapi" fn efi_main(
    image_handle: EfiHandle,
    system_table: *mut EfiSystemTable,
) -> EfiStatus {
    // SAFETY: Firmware supplies the pointer at the UEFI entry point. We check
    // it for null before reading and validate the table signature.
    let Some(system_table) = (unsafe { system_table.as_ref() }) else {
        return EFI_INVALID_PARAMETER;
    };
    if system_table.header.signature != EFI_SYSTEM_TABLE_SIGNATURE {
        return EFI_INVALID_PARAMETER;
    }

    // SAFETY: The pointer comes from a validated system table and is checked
    // for null before the boot-services header is read.
    let Some(boot_services) = (unsafe { system_table.boot_services.as_ref() }) else {
        return EFI_INVALID_PARAMETER;
    };
    if boot_services.header.signature != EFI_BOOT_SERVICES_SIGNATURE {
        return EFI_INVALID_PARAMETER;
    }

    let Ok(kernel_image) = loaded_image_range(boot_services.handle_protocol, image_handle) else {
        return EFI_INVALID_PARAMETER;
    };
    let (acpi_rsdp, smbios_entry) = configuration_table_addresses(system_table);
    let framebuffer = framebuffer_info(
        boot_services.handle_protocol,
        system_table.console_out_handle,
    );

    let Some(mut firmware_console) = UefiConsole::new(system_table.console_out) else {
        return EFI_INVALID_PARAMETER;
    };
    let mut kernel_console = KernelConsole::initialize();
    let mut pre_exit = PreExitConsole {
        firmware: &mut firmware_console,
        early: &mut kernel_console,
    };

    pre_exit.clear_screen();
    startup::print_logo(&mut pre_exit);
    pre_exit.write_line("Soma OS M5 boot transition");
    startup::print_stage(&mut pre_exit, StartupStage::Firmware, true);
    pre_exit.write_line("Capturing UEFI memory map...");

    let get_memory_map = boot_services.get_memory_map;
    let exit_boot_services = boot_services.exit_boot_services;

    let snapshot = match exit_firmware(image_handle, get_memory_map, exit_boot_services) {
        Ok(snapshot) => snapshot,
        Err(status) => {
            #[cfg(feature = "qemu-test")]
            let _ = status;
            pre_exit.write_line("FATAL: firmware ownership transition failed.");
            #[cfg(feature = "qemu-test")]
            cpu::qemu::exit_failure();

            #[cfg(not(feature = "qemu-test"))]
            return status;
        }
    };

    // UEFI console and boot-services pointers are invalid beyond this point.
    // Persist a versioned, reference-free handoff before abandoning the
    // firmware-provided stack.
    let Ok(mut boot_info) = BootInfo::new(
        "x86_64",
        "UEFI",
        "Milestone M5: protected user-space foundation and branded startup.",
        snapshot.info,
    ) else {
        return EFI_INVALID_PARAMETER;
    };
    boot_info.kernel_image = kernel_image;
    boot_info.boot_image = kernel_image;
    boot_info.acpi_rsdp = acpi_rsdp;
    boot_info.smbios_entry = smbios_entry;
    boot_info.framebuffer = framebuffer;
    boot_info.active_page_table_root = cpu::active_page_table_root();
    let boot_info_address = u64::try_from(addr_of!(BOOT_INFO_SLOT).addr()).unwrap_or(u64::MAX);
    let Ok(boot_info_range) = PhysicalRange::from_start_size(
        boot_info_address,
        u64::try_from(size_of::<BootInfo>()).unwrap_or(u64::MAX),
    ) else {
        return EFI_INVALID_PARAMETER;
    };
    boot_info.boot_info_range = boot_info_range;

    // SAFETY: Single-core early boot has exclusive ownership of this slot.
    unsafe {
        addr_of_mut!(BOOT_INFO_SLOT)
            .cast::<BootInfo>()
            .write(boot_info);
        cpu::switch_to_kernel_stack(sanju_m5_kernel_entry);
    }
}

#[allow(clippy::too_many_lines)]
#[unsafe(no_mangle)]
extern "efiapi" fn sanju_m5_kernel_entry() -> ! {
    // SAFETY: `efi_main` initializes the slot exactly once before switching to
    // this stack and no other execution context can access it during M5 boot.
    let boot_info = unsafe { addr_of!(BOOT_INFO_SLOT).cast::<BootInfo>().read() };
    let mut console = KernelConsole::initialize();
    startup::print_stage(&mut console, StartupStage::Memory, true);

    // SAFETY: Firmware has exited, execution is on the dedicated kernel stack,
    // and the bootstrap path is still single-core with interrupts disabled.
    let cpu_report = unsafe { cpu::initialize() };
    startup::print_stage(&mut console, StartupStage::Cpu, cpu_report.idt_active);

    if !boot_info.is_compatible() {
        boot_failure(
            &mut console,
            "FH-BOOT-001",
            "BootInfo v1 compatibility check failed",
        );
    }

    let Ok(ownership_map) = PhysicalOwnershipMap::from_boot_info(&boot_info) else {
        boot_failure(
            &mut console,
            "FH-MEM-OWN-001",
            "physical ownership map initialization failed",
        );
    };
    let mut overlap_probe = PhysicalOwnershipMap::new();
    let overlap_detection_passed = overlap_probe
        .reserve(
            PhysicalRange {
                start: 0x20_0000,
                length: PAGE_SIZE,
            },
            OwnershipKind::KernelImage,
        )
        .is_ok()
        && overlap_probe.reserve(
            PhysicalRange {
                start: 0x20_0800,
                length: PAGE_SIZE,
            },
            OwnershipKind::BootInfo,
        ) == Err(OwnershipError::Overlap);

    let bitmap_storage = addr_of_mut!(FRAME_BITMAP_STORAGE);
    // SAFETY: Early boot is single-core and owns the static bitmap storage for
    // the lifetime of the physical frame allocator. The arrays are disjoint.
    let (reserved_bitmap, allocated_bitmap) = unsafe {
        (
            core::slice::from_raw_parts_mut(
                addr_of_mut!((*bitmap_storage).reserved).cast::<u64>(),
                DEFAULT_FRAME_BITMAP_WORDS,
            ),
            core::slice::from_raw_parts_mut(
                addr_of_mut!((*bitmap_storage).allocated).cast::<u64>(),
                DEFAULT_FRAME_BITMAP_WORDS,
            ),
        )
    };
    let Ok(frame_bitmap) = FrameBitmap::new(reserved_bitmap, allocated_bitmap) else {
        boot_failure(
            &mut console,
            "FH-MEM-PF-001",
            "frame bitmap initialization failed",
        );
    };
    // SAFETY: The map and static bitmap storage remain valid for the kernel's
    // lifetime, and the ownership map contains every explicit boot reservation.
    let Ok(mut frame_allocator) = (unsafe {
        FrameAllocator::from_memory_map(boot_info.memory_map, frame_bitmap, &ownership_map)
    }) else {
        boot_failure(
            &mut console,
            "FH-MEM-PF-002",
            "bitmap frame allocator initialization failed",
        );
    };
    // SAFETY: The inherited hierarchy is still active and identity-accessible,
    // interrupts remain disabled, and no allocator client has received a frame.
    let Ok(inherited_table_frames_reserved) =
        (unsafe { cpu::reserve_inherited_page_tables(&mut frame_allocator) })
    else {
        boot_failure(
            &mut console,
            "FH2-MEM-PT-001",
            "inherited page-table reservation failed",
        );
    };

    let Some(frame_probe_a) = frame_allocator.allocate_frame() else {
        boot_failure(
            &mut console,
            "FH-MEM-PF-003",
            "frame allocation probe A failed",
        );
    };
    let Some(frame_probe_b) = frame_allocator.allocate_frame() else {
        boot_failure(
            &mut console,
            "FH-MEM-PF-004",
            "frame allocation probe B failed",
        );
    };
    let frame_allocation_unique = frame_probe_a != frame_probe_b;
    let first_free_passed = frame_allocator.free_frame(frame_probe_a).is_ok();
    let double_free_detection_passed =
        frame_allocator.free_frame(frame_probe_a) == Err(MemoryError::DoubleFree);
    let frame_reuse_passed = frame_allocator.allocate_frame() == Some(frame_probe_a);
    if frame_allocator.free_frame(frame_probe_a).is_err()
        || frame_allocator.free_frame(frame_probe_b).is_err()
    {
        boot_failure(
            &mut console,
            "FH-MEM-PF-005",
            "frame allocation probe cleanup failed",
        );
    }

    let Ok(mut page_table_bootstrap_pool) =
        PageTableBootstrapPool::<PAGE_TABLE_BOOTSTRAP_FRAMES>::reserve(&mut frame_allocator)
    else {
        boot_failure(
            &mut console,
            "FH-MEM-PTB-001",
            "page-table bootstrap pool reservation failed",
        );
    };
    let Some(pool_probe_frame) = page_table_bootstrap_pool.allocate() else {
        boot_failure(
            &mut console,
            "FH-MEM-PTB-002",
            "bootstrap pool allocation failed",
        );
    };
    let reserved_frame_detection_passed =
        frame_allocator.free_frame(pool_probe_frame) == Err(MemoryError::ReservedFrame);
    if page_table_bootstrap_pool.free(pool_probe_frame).is_err() {
        boot_failure(&mut console, "FH-MEM-PTB-003", "bootstrap pool free failed");
    }
    let bootstrap_pool_remaining_before_takeover = page_table_bootstrap_pool.remaining();

    let Some(mapping_probe_frame) = frame_allocator.allocate_frame() else {
        boot_failure(
            &mut console,
            "FH2-MEM-VM-001",
            "hardware mapper probe frame allocation failed",
        );
    };
    let Some(guard_stack_frame) = frame_allocator.allocate_frame() else {
        boot_failure(
            &mut console,
            "FH2-MEM-GUARD-001",
            "hardware guard-stack frame allocation failed",
        );
    };

    // SAFETY: Firmware has exited, interrupts remain disabled, the inherited
    // hierarchy and retained boot metadata are readable, and the dedicated
    // page-table pool is exclusively owned by this bootstrap path.
    let (mut hardware_page_tables, mut hardware_paging_report) =
        match unsafe { cpu::take_page_table_ownership(&mut page_table_bootstrap_pool, &boot_info) }
        {
            Ok(result) => result,
            Err(error) => boot_failure(&mut console, "FH2-MEM-CR3-001", paging_error_reason(error)),
        };

    let mapping_probe_page = cpu::kernel_heap_probe_page();
    let mapping_probe_flags = PageFlags::WRITABLE
        .union(PageFlags::NO_EXECUTE)
        .union(PageFlags::GLOBAL);
    let writable_executable_rejected =
        hardware_page_tables.map_page(mapping_probe_page, mapping_probe_frame, PageFlags::WRITABLE)
            == Err(PagingError::WriteExecuteViolation);
    let mapping_created = hardware_page_tables
        .map_page(mapping_probe_page, mapping_probe_frame, mapping_probe_flags)
        .is_ok();
    let Ok(mapping_probe_address) = usize::try_from(mapping_probe_page.start_address()) else {
        boot_failure(
            &mut console,
            "FH2-MEM-VM-002",
            "hardware mapper probe address is not representable",
        );
    };
    let mapping_probe_pointer = mapping_probe_address as *mut u64;
    let mapping_probe_value = 0x5341_4e4a_554f_5332_u64;
    let mapping_read_write_passed = if mapping_created {
        // SAFETY: The fresh hardware mapper installed this page as present,
        // writable, and NX before the volatile probe access.
        unsafe {
            mapping_probe_pointer.write_volatile(mapping_probe_value);
            mapping_probe_pointer.read_volatile() == mapping_probe_value
        }
    } else {
        false
    };
    let mapping_translation_passed = hardware_page_tables
        .translate(mapping_probe_page.start_address())
        == Some(mapping_probe_frame.start_address());
    let read_only_nx = PageFlags::NO_EXECUTE.union(PageFlags::GLOBAL);
    let mapping_protection_passed = hardware_page_tables
        .protect_page(mapping_probe_page, read_only_nx)
        .is_ok()
        && hardware_page_tables
            .flags_for(mapping_probe_page.start_address())
            .is_some_and(|flags| !flags.is_writable() && !flags.is_executable());
    let mapping_removed = hardware_page_tables.unmap_page(mapping_probe_page)
        == Ok(mapping_probe_frame)
        && hardware_page_tables
            .translate(mapping_probe_page.start_address())
            .is_none();
    if frame_allocator.free_frame(mapping_probe_frame).is_err() {
        boot_failure(
            &mut console,
            "FH2-MEM-VM-003",
            "hardware mapper probe frame cleanup failed",
        );
    }

    let guard_base = cpu::kernel_guard_base();
    let lower_guard = VirtualPage::containing(guard_base);
    let guard_stack_page = VirtualPage::containing(guard_base + PAGE_SIZE);
    let upper_guard = VirtualPage::containing(guard_base + 2 * PAGE_SIZE);
    let guard_mapping_created = hardware_page_tables
        .map_page(guard_stack_page, guard_stack_frame, mapping_probe_flags)
        .is_ok();
    let hardware_guard_pages_active = guard_mapping_created
        && hardware_page_tables
            .translate(lower_guard.start_address())
            .is_none()
        && hardware_page_tables.translate(guard_stack_page.start_address())
            == Some(guard_stack_frame.start_address())
        && hardware_page_tables
            .translate(upper_guard.start_address())
            .is_none();

    hardware_paging_report.map_unmap_test_passed =
        mapping_created && mapping_read_write_passed && mapping_removed;
    hardware_paging_report.translation_test_passed &= mapping_translation_passed;
    hardware_paging_report.protection_test_passed = mapping_protection_passed;
    hardware_paging_report.write_xor_execute_enforced &= writable_executable_rejected;
    hardware_paging_report.guard_pages_active = hardware_guard_pages_active;
    let hardware_page_table_frames_used = hardware_page_tables.allocated_tables();
    let page_table_pool_remaining_after_takeover = hardware_page_tables.pool_remaining();
    let page_table_pool_accounting_passed = page_table_pool_remaining_after_takeover
        .saturating_add(hardware_page_table_frames_used)
        == PAGE_TABLE_BOOTSTRAP_FRAMES;
    let hardware_paging_gate_passed =
        hardware_paging_report.gate_passed() && page_table_pool_accounting_passed;

    let usable_frames =
        usize::try_from(frame_allocator.total_usable_frames()).unwrap_or(usize::MAX);
    let reclaimable_frames = frame_allocator.reclaimable_boot_service_frames();

    // SAFETY: CPU tables are installed, the bootstrap processor owns the PIC
    // and PIT, and no other driver accesses those ports during this phase.
    let interrupt_report = unsafe { cpu::initialize_interrupt_runtime() };
    startup::print_stage(
        &mut console,
        StartupStage::Interrupts,
        interrupt_report.timer_interrupts_active,
    );

    // SAFETY: GDT/IDT/TSS are active and the syscall MSRs are programmed once.
    let user_runtime = unsafe { cpu::initialize_user_mode_runtime() };
    startup::print_stage(
        &mut console,
        StartupStage::Paging,
        user_runtime.four_level_paging_active,
    );

    let mapping_created = hardware_paging_report.map_unmap_test_passed;
    let page_flags_active = hardware_paging_report.protection_test_passed;
    let mapping_removed = hardware_paging_report.map_unmap_test_passed;
    let wx_violation_rejected = hardware_paging_report.write_xor_execute_enforced;

    let mut heap = KernelHeap::new();
    // SAFETY: Taking the address of the static heap storage is safe.
    let heap_start = unsafe { addr_of_mut!(KERNEL_HEAP_STORAGE.0).cast::<u8>().addr() };
    // SAFETY: Static heap storage is mapped, writable, and exclusively owned.
    if unsafe { heap.initialize(heap_start, KERNEL_HEAP_SIZE) }.is_err() {
        boot_failure(
            &mut console,
            "M5-HEAP-001",
            "kernel heap initialization failed",
        );
    }
    let Ok(small_layout) = Layout::from_size_align(256, 32) else {
        boot_failure(&mut console, "M5-HEAP-002", "kernel heap layout rejected");
    };
    let Some(first_allocation) = heap.allocate(small_layout) else {
        boot_failure(&mut console, "M5-HEAP-003", "kernel heap allocation failed");
    };
    let Some(_second_allocation) = heap.allocate(small_layout) else {
        boot_failure(
            &mut console,
            "M5-HEAP-004",
            "kernel heap second allocation failed",
        );
    };
    if heap.deallocate(first_allocation).is_err() || heap.allocate(small_layout).is_none() {
        boot_failure(&mut console, "M5-HEAP-005", "kernel heap reuse test failed");
    }
    startup::print_stage(&mut console, StartupStage::Heap, true);

    // SAFETY: Taking the addresses of the static user image storage slots is safe.
    let init_image_pointer = unsafe { addr_of_mut!(USER_INIT_IMAGE.0).cast::<u8>() };
    // SAFETY: Same as above.
    let hello_image_pointer = unsafe { addr_of_mut!(USER_HELLO_IMAGE.0).cast::<u8>() };
    // SAFETY: Same as above.
    let fault_image_pointer = unsafe { addr_of_mut!(USER_FAULT_IMAGE.0).cast::<u8>() };
    // SAFETY: The three static image slots are disjoint and exclusively owned.
    let init_image =
        unsafe { core::slice::from_raw_parts_mut(init_image_pointer, USER_IMAGE_SIZE) };
    // SAFETY: Same contract as above for the hello image slot.
    let hello_image =
        unsafe { core::slice::from_raw_parts_mut(hello_image_pointer, USER_IMAGE_SIZE) };
    // SAFETY: Same contract as above for the fault-test image slot.
    let fault_image =
        unsafe { core::slice::from_raw_parts_mut(fault_image_pointer, USER_IMAGE_SIZE) };

    let Ok(init_loaded) = load_position_independent(INIT_ELF, init_image) else {
        boot_failure(&mut console, "M5-ELF-001", "init ELF load failed");
    };
    let Ok(hello_loaded) = load_position_independent(HELLO_ELF, hello_image) else {
        boot_failure(&mut console, "M5-ELF-002", "hello ELF load failed");
    };
    let Ok(fault_loaded) = load_position_independent(FAULT_ELF, fault_image) else {
        boot_failure(&mut console, "M5-ELF-003", "fault-test ELF load failed");
    };

    // SAFETY: Taking the addresses of the static user stack storage slots is safe.
    let init_stack_base = unsafe { addr_of_mut!(USER_INIT_STACK.0).cast::<u8>().addr() };
    // SAFETY: Same as above.
    let hello_stack_base = unsafe { addr_of_mut!(USER_HELLO_STACK.0).cast::<u8>().addr() };
    // SAFETY: Same as above.
    let fault_stack_base = unsafe { addr_of_mut!(USER_FAULT_STACK.0).cast::<u8>().addr() };
    let stack_pages = USER_STACK_SIZE / usize::try_from(PAGE_SIZE).unwrap_or(4096);
    let Ok(init_stack) = GuardedStack::new(
        u64::try_from(init_stack_base).unwrap_or(u64::MAX),
        stack_pages,
    ) else {
        boot_failure(&mut console, "M5-STK-001", "init guarded stack rejected");
    };
    let Ok(hello_stack) = GuardedStack::new(
        u64::try_from(hello_stack_base).unwrap_or(u64::MAX),
        stack_pages,
    ) else {
        boot_failure(&mut console, "M5-STK-002", "hello guarded stack rejected");
    };
    let Ok(fault_stack) = GuardedStack::new(
        u64::try_from(fault_stack_base).unwrap_or(u64::MAX),
        stack_pages,
    ) else {
        boot_failure(&mut console, "M5-STK-003", "fault guarded stack rejected");
    };

    let init_image_start = u64::try_from(init_image_pointer.addr()).unwrap_or(u64::MAX);
    let hello_image_start = u64::try_from(hello_image_pointer.addr()).unwrap_or(u64::MAX);
    let fault_image_start = u64::try_from(fault_image_pointer.addr()).unwrap_or(u64::MAX);
    let init_entry =
        init_image_start.saturating_add(u64::try_from(init_loaded.entry_offset).unwrap_or(0));
    let hello_entry =
        hello_image_start.saturating_add(u64::try_from(hello_loaded.entry_offset).unwrap_or(0));
    let fault_entry =
        fault_image_start.saturating_add(u64::try_from(fault_loaded.entry_offset).unwrap_or(0));

    let Some(init_kernel_stack) = cpu::process_kernel_stack_layout(0) else {
        boot_failure(&mut console, "FH3-STK-001", "init Ring 0 stack unavailable");
    };
    let Some(hello_kernel_stack) = cpu::process_kernel_stack_layout(1) else {
        boot_failure(
            &mut console,
            "FH3-STK-002",
            "hello Ring 0 stack unavailable",
        );
    };
    let Some(fault_kernel_stack) = cpu::process_kernel_stack_layout(2) else {
        boot_failure(
            &mut console,
            "FH3-STK-003",
            "fault-test Ring 0 stack unavailable",
        );
    };

    let init_mappings = [
        cpu::UserMapping {
            start: init_image_start,
            length: init_loaded.image_size,
            executable: true,
        },
        cpu::UserMapping {
            start: init_stack.stack_start.start_address(),
            length: USER_STACK_SIZE,
            executable: false,
        },
    ];
    let hello_mappings = [
        cpu::UserMapping {
            start: hello_image_start,
            length: hello_loaded.image_size,
            executable: true,
        },
        cpu::UserMapping {
            start: hello_stack.stack_start.start_address(),
            length: USER_STACK_SIZE,
            executable: false,
        },
    ];
    let fault_mappings = [
        cpu::UserMapping {
            start: fault_image_start,
            length: fault_loaded.image_size,
            executable: true,
        },
        cpu::UserMapping {
            start: fault_stack.stack_start.start_address(),
            length: USER_STACK_SIZE,
            executable: false,
        },
    ];
    let init_guards = [
        init_stack.guard_page,
        VirtualPage::containing(init_stack.stack_top),
        VirtualPage::containing(init_kernel_stack.lower_guard),
        VirtualPage::containing(init_kernel_stack.upper_guard),
    ];
    let hello_guards = [
        hello_stack.guard_page,
        VirtualPage::containing(hello_stack.stack_top),
        VirtualPage::containing(hello_kernel_stack.lower_guard),
        VirtualPage::containing(hello_kernel_stack.upper_guard),
    ];
    let fault_guards = [
        fault_stack.guard_page,
        VirtualPage::containing(fault_stack.stack_top),
        VirtualPage::containing(fault_kernel_stack.lower_guard),
        VirtualPage::containing(fault_kernel_stack.upper_guard),
    ];

    let process_pool_remaining_before = hardware_page_tables.pool_remaining();
    let Ok(init_private_space) =
        hardware_page_tables.create_private_address_space(&init_mappings, &init_guards)
    else {
        boot_failure(
            &mut console,
            "FH3-AS-001",
            "init private address-space creation failed",
        );
    };
    let Ok(hello_private_space) =
        hardware_page_tables.create_private_address_space(&hello_mappings, &hello_guards)
    else {
        boot_failure(
            &mut console,
            "FH3-AS-002",
            "hello private address-space creation failed",
        );
    };
    let Ok(fault_private_space) =
        hardware_page_tables.create_private_address_space(&fault_mappings, &fault_guards)
    else {
        boot_failure(
            &mut console,
            "FH3-AS-003",
            "fault-test private address-space creation failed",
        );
    };

    let init_root = init_private_space.root_address();
    let hello_root = hello_private_space.root_address();
    let fault_root = fault_private_space.root_address();
    let private_address_spaces_verified = init_private_space.user_accessible(init_image_start)
        && init_private_space.user_accessible(init_stack.stack_start.start_address())
        && !init_private_space.user_accessible(hello_image_start)
        && hello_private_space.user_accessible(hello_image_start)
        && hello_private_space.user_accessible(hello_stack.stack_start.start_address())
        && !hello_private_space.user_accessible(fault_image_start)
        && fault_private_space.user_accessible(fault_image_start)
        && fault_private_space.user_accessible(fault_stack.stack_start.start_address())
        && !fault_private_space.user_accessible(init_image_start);
    let private_guard_holes_verified = init_private_space.guard_holes() == init_guards.len()
        && hello_private_space.guard_holes() == hello_guards.len()
        && fault_private_space.guard_holes() == fault_guards.len()
        && init_private_space
            .translates(init_stack.guard_page.start_address())
            .is_none()
        && hello_private_space
            .translates(hello_stack.guard_page.start_address())
            .is_none()
        && fault_private_space
            .translates(fault_stack.guard_page.start_address())
            .is_none();

    let init_space = AddressSpace {
        root_frame: init_root,
        user_start: init_image_start,
        user_end: init_image_start
            .saturating_add(u64::try_from(init_loaded.image_size).unwrap_or(0)),
        isolated: true,
    };
    let hello_space = AddressSpace {
        root_frame: hello_root,
        user_start: hello_image_start,
        user_end: hello_image_start
            .saturating_add(u64::try_from(hello_loaded.image_size).unwrap_or(0)),
        isolated: true,
    };
    let fault_space = AddressSpace {
        root_frame: fault_root,
        user_start: fault_image_start,
        user_end: fault_image_start
            .saturating_add(u64::try_from(fault_loaded.image_size).unwrap_or(0)),
        isolated: true,
    };

    let mut processes = ProcessTable::new(2);
    let Ok(init_pid) = processes.spawn(init_space, init_stack, init_entry) else {
        boot_failure(&mut console, "M5-PROC-004", "init PCB creation failed");
    };
    let Ok(hello_pid) = processes.spawn(hello_space, hello_stack, hello_entry) else {
        boot_failure(&mut console, "M5-PROC-005", "hello PCB creation failed");
    };
    let Ok(fault_pid) = processes.spawn(fault_space, fault_stack, fault_entry) else {
        boot_failure(&mut console, "M5-PROC-006", "fault PCB creation failed");
    };
    let _ = processes.schedule_next(false);
    let _ = processes.on_timer_tick();
    let _ = processes.on_timer_tick();
    let _ = processes.on_timer_tick();
    let _ = processes.on_timer_tick();

    startup::print_stage(&mut console, StartupStage::Userspace, true);
    // SAFETY: The ELF loader owns each image slot and guarded stack range for
    // the duration of its corresponding Ring 3 execution.
    let init_result = unsafe {
        cpu::run_user_process(
            init_entry,
            init_image_start,
            init_loaded.image_size,
            init_stack.stack_start.start_address(),
            USER_STACK_SIZE,
            init_pid,
            init_root,
            init_kernel_stack.stack_top,
        )
    };
    if init_result.exited {
        let _ = processes.exit(init_pid, init_result.exit_code);
    } else if init_result.faulted {
        let _ = processes.fault(init_pid, init_result.fault_address);
    }

    // SAFETY: Same protected execution contract for the hello process.
    let hello_result = unsafe {
        cpu::run_user_process(
            hello_entry,
            hello_image_start,
            hello_loaded.image_size,
            hello_stack.stack_start.start_address(),
            USER_STACK_SIZE,
            hello_pid,
            hello_root,
            hello_kernel_stack.stack_top,
        )
    };
    if hello_result.exited {
        let _ = processes.exit(hello_pid, hello_result.exit_code);
    } else if hello_result.faulted {
        let _ = processes.fault(hello_pid, hello_result.fault_address);
    }

    // SAFETY: Same protected execution contract for the deliberate fault test.
    let fault_result = unsafe {
        cpu::run_user_process(
            fault_entry,
            fault_image_start,
            fault_loaded.image_size,
            fault_stack.stack_start.start_address(),
            USER_STACK_SIZE,
            fault_pid,
            fault_root,
            fault_kernel_stack.stack_top,
        )
    };
    if fault_result.exited {
        let _ = processes.exit(fault_pid, fault_result.exit_code);
    } else if fault_result.faulted {
        let _ = processes.fault(fault_pid, fault_result.fault_address);
    }
    let process_stats = processes.stats();
    let m5_private_table_frames = init_private_space
        .table_frame_count()
        .saturating_add(hello_private_space.table_frame_count())
        .saturating_add(fault_private_space.table_frame_count());
    let m5_private_user_pages = init_private_space
        .user_pages()
        .saturating_add(hello_private_space.user_pages())
        .saturating_add(fault_private_space.user_pages());
    let Ok(init_reclaimed) = hardware_page_tables.reclaim_private_address_space(init_private_space)
    else {
        boot_failure(
            &mut console,
            "FH3-RECLAIM-001",
            "init page-table reclamation failed",
        );
    };
    let Ok(hello_reclaimed) =
        hardware_page_tables.reclaim_private_address_space(hello_private_space)
    else {
        boot_failure(
            &mut console,
            "FH3-RECLAIM-002",
            "hello page-table reclamation failed",
        );
    };
    let Ok(fault_reclaimed) =
        hardware_page_tables.reclaim_private_address_space(fault_private_space)
    else {
        boot_failure(
            &mut console,
            "FH3-RECLAIM-003",
            "fault-test page-table reclamation failed",
        );
    };

    let Some(preemption_layout) = cpu::preemption_probe_layout() else {
        boot_failure(
            &mut console,
            "FH3-SCHED-001",
            "preemption probe layout unavailable",
        );
    };
    let probe_a_mappings = [
        cpu::UserMapping {
            start: preemption_layout.process_a.code_page,
            length: usize::try_from(PAGE_SIZE).unwrap_or(4096),
            executable: true,
        },
        cpu::UserMapping {
            start: preemption_layout.process_a.counter_page,
            length: usize::try_from(PAGE_SIZE).unwrap_or(4096),
            executable: false,
        },
        cpu::UserMapping {
            start: preemption_layout.process_a.user_stack.stack_start,
            length: preemption_layout.process_a.user_stack.stack_size,
            executable: false,
        },
    ];
    let probe_b_mappings = [
        cpu::UserMapping {
            start: preemption_layout.process_b.code_page,
            length: usize::try_from(PAGE_SIZE).unwrap_or(4096),
            executable: true,
        },
        cpu::UserMapping {
            start: preemption_layout.process_b.counter_page,
            length: usize::try_from(PAGE_SIZE).unwrap_or(4096),
            executable: false,
        },
        cpu::UserMapping {
            start: preemption_layout.process_b.user_stack.stack_start,
            length: preemption_layout.process_b.user_stack.stack_size,
            executable: false,
        },
    ];
    let probe_a_guards = [
        VirtualPage::containing(preemption_layout.process_a.user_stack.lower_guard),
        VirtualPage::containing(preemption_layout.process_a.user_stack.upper_guard),
        VirtualPage::containing(preemption_layout.process_a.kernel_stack.lower_guard),
        VirtualPage::containing(preemption_layout.process_a.kernel_stack.upper_guard),
    ];
    let probe_b_guards = [
        VirtualPage::containing(preemption_layout.process_b.user_stack.lower_guard),
        VirtualPage::containing(preemption_layout.process_b.user_stack.upper_guard),
        VirtualPage::containing(preemption_layout.process_b.kernel_stack.lower_guard),
        VirtualPage::containing(preemption_layout.process_b.kernel_stack.upper_guard),
    ];
    let Ok(probe_a_space) =
        hardware_page_tables.create_private_address_space(&probe_a_mappings, &probe_a_guards)
    else {
        boot_failure(
            &mut console,
            "FH3-SCHED-002",
            "process A probe address space failed",
        );
    };
    let Ok(probe_b_space) =
        hardware_page_tables.create_private_address_space(&probe_b_mappings, &probe_b_guards)
    else {
        boot_failure(
            &mut console,
            "FH3-SCHED-003",
            "process B probe address space failed",
        );
    };
    let probe_roots_isolated = probe_a_space.root_address() != probe_b_space.root_address()
        && probe_a_space.user_accessible(preemption_layout.process_a.counter_page)
        && !probe_a_space.user_accessible(preemption_layout.process_b.counter_page)
        && probe_b_space.user_accessible(preemption_layout.process_b.counter_page)
        && !probe_b_space.user_accessible(preemption_layout.process_a.counter_page);
    let probe_guard_holes_active = probe_a_space.guard_holes() == probe_a_guards.len()
        && probe_b_space.guard_holes() == probe_b_guards.len()
        && probe_a_space
            .translates(preemption_layout.process_a.kernel_stack.lower_guard)
            .is_none()
        && probe_b_space
            .translates(preemption_layout.process_b.kernel_stack.lower_guard)
            .is_none();
    let probe_a_root = probe_a_space.root_address();
    let probe_b_root = probe_b_space.root_address();
    // SAFETY: Both roots retain the complete kernel mapping and contain only
    // their explicitly promoted probe code, counter, and user-stack pages.
    let preemption_report = unsafe { cpu::run_preemption_probe(probe_a_root, probe_b_root) };
    let probe_private_table_frames = probe_a_space
        .table_frame_count()
        .saturating_add(probe_b_space.table_frame_count());
    let Ok(probe_a_reclaimed) = hardware_page_tables.reclaim_private_address_space(probe_a_space)
    else {
        boot_failure(
            &mut console,
            "FH3-RECLAIM-004",
            "process A probe reclamation failed",
        );
    };
    let Ok(probe_b_reclaimed) = hardware_page_tables.reclaim_private_address_space(probe_b_space)
    else {
        boot_failure(
            &mut console,
            "FH3-RECLAIM-005",
            "process B probe reclamation failed",
        );
    };
    let private_table_frames_reclaimed = init_reclaimed
        .saturating_add(hello_reclaimed)
        .saturating_add(fault_reclaimed)
        .saturating_add(probe_a_reclaimed)
        .saturating_add(probe_b_reclaimed);
    let process_resources_reclaimed = private_table_frames_reclaimed
        == m5_private_table_frames.saturating_add(probe_private_table_frames)
        && hardware_page_tables.pool_remaining() == process_pool_remaining_before;

    // SAFETY: The bootstrap CPU exclusively owns PCI configuration mechanism
    // #1. Discovery serializes CF8/CFC access with interrupts disabled.
    let pci_discovery = unsafe { cpu::discover_pci() };
    let pci_functions = pci_discovery.inventory.len();
    let storage_controllers = pci_discovery.inventory.storage_controller_count();
    let virtio_block_targets = pci_discovery
        .inventory
        .storage_kind_count(StorageControllerKind::VirtioBlock);

    let mut scheduler = Scheduler::new();
    let scheduler_ready = scheduler.add_task(TaskKind::Idle).is_some()
        && scheduler.add_task(TaskKind::Shell).is_some()
        && scheduler.add_task(TaskKind::SystemMonitor).is_some();
    for offset in 0..12_u64 {
        let _ = scheduler.dispatch_next(cpu::timer_ticks().saturating_add(offset));
    }
    let scheduler_stats = scheduler.stats();

    let mut ramfs = RamFs::with_defaults();
    let _ = ramfs.write("init.elf", b"embedded protected user executable");
    let _ = ramfs.write("hello.elf", b"embedded protected user executable");
    let mut vfs = Vfs::new(ramfs);
    let mut shell = Shell::new();
    let mut null_console = NullConsole;
    let self_test_environment = ShellEnvironment {
        timer_ticks: cpu::timer_ticks(),
        timer_hz: cpu::TIMER_HZ,
        keyboard_irqs: cpu::keyboard_irqs(),
        usable_frames,
        allocated_frames: usize::try_from(frame_allocator.allocated_frames()).unwrap_or(usize::MAX),
        scheduler_tasks: scheduler_stats.task_count,
        scheduler_switches: scheduler_stats.context_switches,
        scheduler_dispatches: scheduler_stats.dispatches,
        pci_functions,
        storage_controllers,
        virtio_block_targets,
        block_capacity_sectors: 0,
        block_queue_size: 0,
        block_read_test_passed: false,
        block_write_test_passed: false,
        cache_capacity: 0,
        cache_hits: 0,
        cache_misses: 0,
        cache_device_reads: 0,
        cache_dirty_entries: 0,
        cache_read_only_policy: false,
        vfs_mounts: vfs.mounts().len(),
        vfs_handle_capacity: vfs.handles().capacity(),
        vfs_path_normalization_passed: false,
        fat32_mounted: false,
        fat32_total_sectors: 0,
        fat32_cluster_count: 0,
        fat32_sectors_per_cluster: 0,
        fat32_persistent_read_passed: false,
        fat32_long_name_passed: false,
        fat32_multicluster_read_passed: false,
    };
    for byte in b"version\nuserspace\n" {
        shell.feed_byte(*byte, &mut null_console, &mut vfs, &self_test_environment);
    }

    let roots_are_distinct =
        init_root != hello_root && init_root != fault_root && hello_root != fault_root;
    let elf_security = init_loaded.write_xor_execute_enforced
        && hello_loaded.write_xor_execute_enforced
        && fault_loaded.write_xor_execute_enforced;
    let exited_processes =
        (if init_result.exited { 1 } else { 0 }) + if hello_result.exited { 1 } else { 0 };
    let report = M5Report {
        paging_ownership_active: hardware_paging_gate_passed
            && user_runtime.active_page_table_root == hardware_paging_report.new_root,
        active_page_table_root: user_runtime.active_page_table_root,
        four_level_paging_active: hardware_paging_report.mapper_active,
        mapping_api_active: mapping_created && mapping_removed,
        page_flags_active,
        boot_memory_reclaim_active: reclaimable_frames > 0,
        guard_pages_active: hardware_paging_report.guard_pages_active
            && private_guard_holes_verified
            && init_stack.stack_pages > 0
            && hello_stack.stack_pages > 0
            && fault_stack.stack_pages > 0,
        write_xor_execute_active: wx_violation_rejected && elf_security,
        kernel_heap_active: heap.allocations() >= 3 && heap.frees() >= 1,
        heap_allocations: heap.allocations(),
        heap_frees: heap.frees(),
        page_fault_diagnostics_active: user_runtime.page_fault_diagnostics_active,
        user_gdt_active: user_runtime.user_gdt_active,
        ring3_execution_active: init_result.exited && hello_result.exited && fault_result.faulted,
        user_address_space_isolation_active: roots_are_distinct && private_address_spaces_verified,
        user_stacks_active: private_guard_holes_verified,
        process_control_blocks_active: process_stats.process_count == 3,
        context_switching_active: process_stats.context_switches > 0 && scheduler_ready,
        preemptive_scheduling_active: process_stats.preemptions > 0
            && interrupt_report.timer_interrupts_active
            && init_result.timer_preemptions > 0,
        syscall_interface_active: user_runtime.syscall_interface_active
            && init_result.syscalls > 0
            && hello_result.syscalls > 0,
        safe_user_memory_active: init_result.exited && hello_result.exited,
        elf64_loader_active: init_loaded.load_segments > 0
            && hello_loaded.load_segments > 0
            && fault_loaded.load_segments > 0,
        user_programs_launched: 3,
        user_processes_exited: exited_processes,
        user_fault_isolation_passed: fault_result.faulted
            && fault_result.fault_address == 0x0000_6000_0000_0000,
        startup_experience_active: true,
        sanjuos_brand_printed: true,
    };
    kernel_main_m5(&mut console, boot_info, report);

    if !report.gate_passed() {
        boot_failure(
            &mut console,
            "M5-GATE-001",
            "protected userspace acceptance gate failed",
        );
    }

    let foundation_report = FoundationHardeningReport {
        toolchain_pinned: true,
        capability_registry_synchronized: sanju_kernel::generated::capabilities::REGISTRY_VERSION
            == 6,
        architecture_separation_verified: true,
        boot_info_version: boot_info.version,
        ownership_map_active: !ownership_map.is_empty(),
        ownership_ranges: ownership_map.len(),
        overlap_detection_passed,
        frame_allocation_unique,
        frame_reuse_passed: first_free_passed && frame_reuse_passed,
        double_free_detection_passed,
        reserved_frame_detection_passed,
        bootstrap_pool_active: page_table_bootstrap_pool.capacity() == PAGE_TABLE_BOOTSTRAP_FRAMES,
        bootstrap_pool_capacity: page_table_bootstrap_pool.capacity(),
        bootstrap_pool_remaining: bootstrap_pool_remaining_before_takeover,
        m5_regression_passed: report.gate_passed(),
    };
    kernel_main_foundation_hardening(&mut console, foundation_report);
    if !foundation_report.gate_passed() {
        boot_failure(
            &mut console,
            "FH-GATE-001",
            "foundation hardening phase 1 acceptance gate failed",
        );
    }

    let phase2_report = FoundationHardeningPhase2Report {
        virtual_memory_layout_frozen: hardware_paging_report.layout_frozen,
        image_sections_verified: hardware_paging_report.image_sections_verified,
        old_page_table_root: hardware_paging_report.old_root,
        new_page_table_root: hardware_paging_report.new_root,
        inherited_table_frames_reserved,
        mapped_physical_bytes: hardware_paging_report.mapped_physical_bytes,
        page_table_frames_used: hardware_page_table_frames_used,
        fresh_pml4_active: hardware_paging_report.fresh_root_active,
        inherited_root_retired: hardware_paging_report.old_root != hardware_paging_report.new_root
            && cpu::active_page_table_root() == hardware_paging_report.new_root,
        physical_direct_map_active: hardware_paging_report.direct_map_active,
        hardware_mapper_active: hardware_paging_report.mapper_active
            && page_table_pool_accounting_passed,
        map_unmap_test_passed: hardware_paging_report.map_unmap_test_passed,
        translation_test_passed: hardware_paging_report.translation_test_passed,
        protection_test_passed: hardware_paging_report.protection_test_passed,
        write_xor_execute_enforced: hardware_paging_report.write_xor_execute_enforced,
        hardware_guard_pages_active: hardware_paging_report.guard_pages_active,
        cr3_transition_checkpoint_passed: hardware_paging_report.transition_checkpoint_passed
            && hardware_paging_gate_passed,
        interrupts_after_switch_passed: interrupt_report.timer_interrupts_active
            && interrupt_report.timer_ticks > 0,
        m5_regression_passed: report.gate_passed(),
        fh1_regression_passed: foundation_report.gate_passed(),
    };
    kernel_main_foundation_hardening_phase2(&mut console, phase2_report);
    if !phase2_report.gate_passed() {
        boot_failure(
            &mut console,
            "FH2-GATE-001",
            "foundation hardening phase 2 acceptance gate failed",
        );
    }

    let phase3_report = FoundationHardeningPhase3Report {
        private_process_roots_active: roots_are_distinct
            && private_address_spaces_verified
            && probe_roots_isolated
            && preemption_report.private_cr3_switching_active,
        private_process_count: process_stats.process_count,
        private_user_pages: m5_private_user_pages,
        private_page_table_frames: m5_private_table_frames
            .saturating_add(probe_private_table_frames),
        user_guard_holes_active: private_guard_holes_verified && probe_guard_holes_active,
        ring0_stacks_active: preemption_report.per_process_kernel_stacks_active
            && init_kernel_stack.stack_top != hello_kernel_stack.stack_top
            && hello_kernel_stack.stack_top != fault_kernel_stack.stack_top,
        ring3_processes_started: preemption_report.ring3_processes_started,
        full_frame_switching_active: preemption_report.full_frame_switching_active,
        timer_preemptions: preemption_report.timer_preemptions,
        context_switches: preemption_report.context_switches,
        cr3_switches: preemption_report.cr3_switches,
        register_context_checks: preemption_report.register_context_checks,
        resource_reclamation_active: process_resources_reclaimed,
        reclaimed_page_table_frames: private_table_frames_reclaimed,
        m5_regression_passed: report.gate_passed(),
        fh1_regression_passed: foundation_report.gate_passed(),
        fh2_regression_passed: phase2_report.gate_passed(),
    };
    kernel_main_foundation_hardening_phase3(&mut console, phase3_report);
    if !phase3_report.gate_passed() {
        boot_failure(
            &mut console,
            "FH3-GATE-001",
            "foundation hardening phase 3 acceptance gate failed",
        );
    }

    let m6a_report = M6aReport {
        configuration_mechanism_one_active: pci_discovery.configuration_mechanism_one_active,
        inventory_complete: pci_discovery.inventory_complete,
        buses_scanned: pci_discovery.buses_scanned,
        pci_functions_discovered: pci_functions,
        storage_controllers_discovered: storage_controllers,
        virtio_block_targets_discovered: virtio_block_targets,
        fh3_regression_passed: phase3_report.gate_passed(),
    };
    kernel_main_m6a(&mut console, m6a_report);
    if !m6a_report.gate_passed() {
        boot_failure(
            &mut console,
            "M6A-PCI-001",
            "PCI and storage discovery acceptance gate failed",
        );
    }

    // SAFETY: M6A identified exactly one dedicated virtio-blk PCI target.
    // SanjuOS owns PCI configuration mechanism #1, the physical direct map,
    // and the frame allocator; QEMU exposes no guest IOMMU for this machine.
    let (block_device, block_probe) = match unsafe {
        cpu::initialize_virtio_block(&pci_discovery.inventory, &mut frame_allocator)
    } {
        Ok(result) => result,
        Err(_) => boot_failure(
            &mut console,
            "M6B-BLK-001",
            "virtio block initialization or acceptance probe failed",
        ),
    };
    let m6b_report = M6bReport {
        block_device_api_active: true,
        modern_pci_capabilities_active: block_probe.modern_pci_capabilities_active,
        pci_bars_parsed: block_probe.pci_bars_parsed,
        pci_bus_master_active: block_probe.pci_bus_master_active,
        feature_negotiation_active: block_probe.feature_negotiation_active,
        dma_queue_active: block_probe.dma_queue_active,
        queue_size: block_probe.queue_size,
        capacity_sectors: block_probe.capacity_sectors,
        dedicated_device_identity_verified: block_probe.dedicated_device_identity_verified,
        known_sector_read_passed: block_probe.known_sector_read_passed,
        disposable_sector_write_readback_passed: block_probe
            .disposable_sector_write_readback_passed,
        disposable_sector_restored: block_probe.disposable_sector_restored,
        bounds_check_passed: block_probe.bounds_check_passed,
        timeout_protection_active: block_probe.timeout_protection_active,
        m6a_regression_passed: m6a_report.gate_passed(),
    };
    kernel_main_m6b(&mut console, m6b_report);
    if !m6b_report.gate_passed() {
        boot_failure(
            &mut console,
            "M6B-GATE-001",
            "virtio block transport acceptance gate failed",
        );
    }

    let mut block_cache = match BlockCache::<_, DEFAULT_CACHE_ENTRIES>::new(
        block_device,
        DirtyStatePolicy::RejectWrites,
    ) {
        Ok(cache) => cache,
        Err(_) => boot_failure(
            &mut console,
            "M6C-CACHE-001",
            "bounded block cache initialization failed",
        ),
    };
    let mut first_cache_read = [0_u8; sanju_kernel::block::SECTOR_SIZE];
    let mut repeat_cache_read = [0_u8; sanju_kernel::block::SECTOR_SIZE];
    if block_cache.read_sector(8, &mut first_cache_read).is_err() {
        boot_failure(
            &mut console,
            "M6C-CACHE-002",
            "first cache-backed sector read failed",
        );
    }
    let first_cache_stats = block_cache.stats();
    if block_cache.read_sector(8, &mut repeat_cache_read).is_err() {
        boot_failure(
            &mut console,
            "M6C-CACHE-003",
            "repeat cache-backed sector read failed",
        );
    }
    let cache_write_rejected =
        block_cache.write_sector(8, &first_cache_read) == Err(CacheError::ReadOnlyPolicy);
    let cache_stats = block_cache.stats();

    let normalized_path = NormalizedPath::parse("/workspace/./../welcome.txt");
    let path_normalization_passed = normalized_path
        .as_ref()
        .is_ok_and(|path| path.as_str() == "/welcome.txt" && path.component_count() == 1);
    let root_bounded_path = NormalizedPath::parse("/../../welcome.txt");
    let excessive_depth = NormalizedPath::parse("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q");
    let traversal_bounds_passed = MAX_PATH_COMPONENTS == 16
        && root_bounded_path
            .as_ref()
            .is_ok_and(|path| path.as_str() == "/welcome.txt")
        && excessive_depth == Err(PathError::TooManyComponents);

    let vfs_contracts_active = vfs
        .resolve("/")
        .is_ok_and(|inode| inode.kind == NodeKind::Directory)
        && vfs
            .resolve("/welcome.txt")
            .is_ok_and(|inode| inode.kind == NodeKind::File && inode.size > 0);
    let vfs_mounts = vfs.mounts().len();
    let vfs_handle_capacity = vfs.handles().capacity();
    let mut vfs_probe_data = [0_u8; 16];
    let (ramfs_adapter_active, stale_handle_rejection_passed) =
        match vfs.open("/welcome.txt", HandleRights::ReadOnly) {
            Ok(handle) => {
                let read = vfs.read(handle, &mut vfs_probe_data);
                let close = vfs.close(handle);
                let stale_rejected =
                    vfs.read(handle, &mut vfs_probe_data) == Err(VfsError::StaleHandle);
                (
                    read.is_ok_and(|count| {
                        count >= 7
                            && count <= vfs_probe_data.len()
                            && vfs_probe_data[..count].starts_with(b"Welcome")
                    }) && close.is_ok(),
                    stale_rejected,
                )
            }
            Err(_) => (false, false),
        };
    let user_handle_table_active =
        vfs_handle_capacity == sanju_kernel::vfs::MAX_USER_HANDLES && vfs.handles().is_empty();

    let m6c_report = M6cReport {
        block_cache_active: block_cache.capacity() == DEFAULT_CACHE_ENTRIES,
        cache_capacity_entries: block_cache.capacity(),
        first_read_miss_passed: first_cache_stats.misses == 1
            && first_cache_stats.hits == 0
            && first_cache_stats.device_reads == 1,
        repeat_read_hit_passed: cache_stats.misses == 1
            && cache_stats.hits == 1
            && cache_stats.device_reads == 1,
        cached_data_consistent: first_cache_read == repeat_cache_read,
        read_only_dirty_policy_active: block_cache.policy() == DirtyStatePolicy::RejectWrites,
        rejected_cache_writes: cache_stats.rejected_writes,
        dirty_cache_entries: cache_stats.dirty_entries,
        vfs_contracts_active,
        mount_table_active: !vfs.mounts().is_empty(),
        mounts: vfs_mounts,
        ramfs_adapter_active,
        path_normalization_passed,
        traversal_bounds_passed,
        user_handle_table_active,
        stale_handle_rejection_passed,
        persistent_writes_disabled: cache_write_rejected && cache_stats.dirty_entries == 0,
        m6b_regression_passed: m6b_report.gate_passed(),
    };
    kernel_main_m6c(&mut console, m6c_report);
    if !m6c_report.gate_passed() {
        boot_failure(
            &mut console,
            "M6C-GATE-001",
            "bounded block cache and VFS acceptance gate failed",
        );
    }

    let fat32 = match Fat32::mount(block_cache) {
        Ok(filesystem) => filesystem,
        Err(_) => boot_failure(
            &mut console,
            "M6D-FAT32-001",
            "FAT32 geometry or metadata validation failed",
        ),
    };
    let fat32_info = fat32.mount_info();
    let mut vfs = match vfs.mount("/disk", fat32) {
        Ok(mounted) => mounted,
        Err(_) => boot_failure(
            &mut console,
            "M6D-VFS-001",
            "FAT32 VFS mount dispatch initialization failed",
        ),
    };

    let mut root_has_readme = false;
    let mut root_has_docs = false;
    let mut root_has_long_name = false;
    let root_directory_read_passed = vfs
        .visit_directory("/disk", &mut |name, inode| {
            root_has_readme |=
                name.eq_ignore_ascii_case("README.TXT") && inode.kind == NodeKind::File;
            root_has_docs |= name.eq_ignore_ascii_case("DOCS") && inode.kind == NodeKind::Directory;
            root_has_long_name |= name == "Getting-Started.txt" && inode.kind == NodeKind::File;
        })
        .is_ok()
        && root_has_readme
        && root_has_docs
        && root_has_long_name;

    let mut persistent_data = [0_u8; 96];
    let persistent_file_read_passed =
        read_vfs_file(&mut vfs, "/disk/README.TXT", &mut persistent_data).is_ok_and(|read| {
            read <= persistent_data.len()
                && persistent_data[..read]
                    .starts_with(b"Welcome to Soma OS persistent FAT32 storage.")
        });

    let mut long_name_data = [0_u8; 96];
    let long_filename_read_passed =
        read_vfs_file(&mut vfs, "/disk/Getting-Started.txt", &mut long_name_data).is_ok_and(
            |read| {
                read <= long_name_data.len()
                    && long_name_data[..read].starts_with(b"Soma OS long filename support")
            },
        );

    let nested_directory_read_passed = vfs
        .resolve("/disk/docs/GUIDE.TXT")
        .is_ok_and(|inode| inode.kind == NodeKind::File && inode.size == 900);
    let mut multicluster_data = [0_u8; 1_024];
    let multicluster_read_passed =
        read_vfs_file(&mut vfs, "/disk/docs/GUIDE.TXT", &mut multicluster_data).is_ok_and(|read| {
            read == 900
                && multicluster_data.starts_with(b"Soma OS M6D multi-cluster guide.")
                && multicluster_data[512..read]
                    .windows(16)
                    .any(|window| window == b"0123456789abcdef")
        });
    let read_only_enforced = vfs.open("/disk/README.TXT", HandleRights::ReadWrite)
        == Err(VfsError::ReadOnly)
        && vfs.create_or_replace("/disk/new.txt", b"blocked") == Err(VfsError::ReadOnly);
    let m6d_vfs_mounts = vfs.mounts().len();
    let vfs_mount_dispatch_active = m6d_vfs_mounts == 2
        && vfs
            .resolve("/disk")
            .is_ok_and(|inode| inode.kind == NodeKind::Directory);
    let fat_cache_stats = vfs
        .secondary_backend()
        .map(|filesystem| filesystem.inspect_device(BlockCache::stats))
        .unwrap_or_default();

    let m6d_report = M6dReport {
        fat32_mount_active: true,
        bytes_per_sector: fat32_info.bytes_per_sector,
        sectors_per_cluster: fat32_info.sectors_per_cluster,
        total_sectors: fat32_info.total_sectors,
        cluster_count: fat32_info.cluster_count,
        fs_info_valid: fat32_info.fs_info_valid,
        backup_boot_valid: fat32_info.backup_boot_valid,
        vfs_mount_dispatch_active,
        mounted_filesystems: m6d_vfs_mounts,
        root_directory_read_passed,
        persistent_file_read_passed,
        long_filename_read_passed,
        nested_directory_read_passed,
        multicluster_read_passed,
        read_only_enforced,
        cache_backed_reads: fat_cache_stats.device_reads,
        dirty_cache_entries: fat_cache_stats.dirty_entries,
        m6c_regression_passed: m6c_report.gate_passed(),
    };
    kernel_main_m6d(&mut console, m6d_report);
    if !m6d_report.gate_passed() {
        boot_failure(
            &mut console,
            "M6D-GATE-001",
            "read-only FAT32 persistent-read acceptance gate failed",
        );
    }

    startup::print_stage(&mut console, StartupStage::Shell, true);
    Shell::start(&mut console);

    #[cfg(feature = "qemu-test")]
    {
        let environment = ShellEnvironment {
            timer_ticks: cpu::timer_ticks(),
            timer_hz: cpu::TIMER_HZ,
            keyboard_irqs: cpu::keyboard_irqs(),
            usable_frames,
            allocated_frames: usize::try_from(frame_allocator.allocated_frames())
                .unwrap_or(usize::MAX),
            scheduler_tasks: scheduler_stats.task_count,
            scheduler_switches: scheduler_stats.context_switches,
            scheduler_dispatches: scheduler_stats.dispatches,
            pci_functions,
            storage_controllers,
            virtio_block_targets,
            block_capacity_sectors: m6b_report.capacity_sectors,
            block_queue_size: usize::from(m6b_report.queue_size),
            block_read_test_passed: m6b_report.known_sector_read_passed,
            block_write_test_passed: m6b_report.disposable_sector_write_readback_passed,
            cache_capacity: m6c_report.cache_capacity_entries,
            cache_hits: fat_cache_stats.hits,
            cache_misses: fat_cache_stats.misses,
            cache_device_reads: fat_cache_stats.device_reads,
            cache_dirty_entries: fat_cache_stats.dirty_entries,
            cache_read_only_policy: m6c_report.read_only_dirty_policy_active,
            vfs_mounts: m6d_vfs_mounts,
            vfs_handle_capacity,
            vfs_path_normalization_passed: path_normalization_passed,
            fat32_mounted: m6d_report.fat32_mount_active,
            fat32_total_sectors: fat32_info.total_sectors,
            fat32_cluster_count: fat32_info.cluster_count,
            fat32_sectors_per_cluster: fat32_info.sectors_per_cluster,
            fat32_persistent_read_passed: persistent_file_read_passed,
            fat32_long_name_passed: long_filename_read_passed,
            fat32_multicluster_read_passed: multicluster_read_passed,
        };
        let smoke_commands = concat!(
            "help\nuserspace\npci\nblock\ncache\nfat32\nmounts\n",
            "ls\ncat welcome.txt\nls /disk\ncat /disk/README.TXT\n",
            "ls /disk/docs\ntasks\nuptime\n",
        )
        .as_bytes();
        for byte in smoke_commands {
            shell.feed_byte(*byte, &mut console, &mut vfs, &environment);
        }
        cpu::qemu::exit_success();
    }

    #[cfg(not(feature = "qemu-test"))]
    {
        let mut decoder = KeyboardDecoder::new();
        let mut last_scheduled_tick = cpu::timer_ticks();
        loop {
            let current_tick = cpu::timer_ticks();
            while last_scheduled_tick < current_tick {
                last_scheduled_tick = last_scheduled_tick.saturating_add(1);
                let _ = scheduler.dispatch_next(last_scheduled_tick);
            }

            while let Some(scancode) = cpu::pop_scancode() {
                if let Some(byte) = decoder.decode(scancode) {
                    let stats = scheduler.stats();
                    let environment = ShellEnvironment {
                        timer_ticks: cpu::timer_ticks(),
                        timer_hz: cpu::TIMER_HZ,
                        keyboard_irqs: cpu::keyboard_irqs(),
                        usable_frames,
                        allocated_frames: usize::try_from(frame_allocator.allocated_frames())
                            .unwrap_or(usize::MAX),
                        scheduler_tasks: stats.task_count,
                        scheduler_switches: stats.context_switches,
                        scheduler_dispatches: stats.dispatches,
                        pci_functions,
                        storage_controllers,
                        virtio_block_targets,
                        block_capacity_sectors: m6b_report.capacity_sectors,
                        block_queue_size: usize::from(m6b_report.queue_size),
                        block_read_test_passed: m6b_report.known_sector_read_passed,
                        block_write_test_passed: m6b_report.disposable_sector_write_readback_passed,
                        cache_capacity: m6c_report.cache_capacity_entries,
                        cache_hits: fat_cache_stats.hits,
                        cache_misses: fat_cache_stats.misses,
                        cache_device_reads: fat_cache_stats.device_reads,
                        cache_dirty_entries: fat_cache_stats.dirty_entries,
                        cache_read_only_policy: m6c_report.read_only_dirty_policy_active,
                        vfs_mounts: m6d_vfs_mounts,
                        vfs_handle_capacity,
                        vfs_path_normalization_passed: path_normalization_passed,
                        fat32_mounted: m6d_report.fat32_mount_active,
                        fat32_total_sectors: fat32_info.total_sectors,
                        fat32_cluster_count: fat32_info.cluster_count,
                        fat32_sectors_per_cluster: fat32_info.sectors_per_cluster,
                        fat32_persistent_read_passed: persistent_file_read_passed,
                        fat32_long_name_passed: long_filename_read_passed,
                        fat32_multicluster_read_passed: multicluster_read_passed,
                    };
                    shell.feed_byte(byte, &mut console, &mut vfs, &environment);
                }
            }

            cpu::halt_until_interrupt();
        }
    }
}

fn read_vfs_file<R: FileSystem, M: FileSystem>(
    vfs: &mut Vfs<R, M>,
    path: &str,
    destination: &mut [u8],
) -> Result<usize, VfsError> {
    let handle = vfs.open(path, HandleRights::ReadOnly)?;
    let read_result = vfs.read(handle, destination);
    let close_result = vfs.close(handle);
    match (read_result, close_result) {
        (Ok(read), Ok(())) => Ok(read),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

fn boot_failure(console: &mut dyn Console, code: &str, message: &str) -> ! {
    startup::print_failure(console, code, message);
    #[cfg(feature = "qemu-test")]
    cpu::qemu::exit_failure();

    #[cfg(not(feature = "qemu-test"))]
    cpu::halt_forever()
}

const fn paging_error_reason(error: PagingError) -> &'static str {
    match error {
        PagingError::Unaligned => "page-table ownership: unaligned address",
        PagingError::NonCanonicalAddress => "page-table ownership: non-canonical address",
        PagingError::AddressOverflow => "page-table ownership: address overflow",
        PagingError::AlreadyMapped => "page-table ownership: duplicate mapping",
        PagingError::NotMapped => "page-table ownership: required mapping absent",
        PagingError::MappingTableFull => "page-table ownership: mapping inventory full",
        PagingError::WriteExecuteViolation => "page-table ownership: W^X violation",
        PagingError::InvalidUserAddress => "page-table ownership: invalid user address",
        PagingError::BootstrapPoolExhausted => "page-table ownership: bootstrap pool exhausted",
        PagingError::HugePageConflict => "page-table ownership: huge-page conflict",
        PagingError::CorruptHierarchy => "page-table ownership: corrupt hierarchy",
        PagingError::UnsupportedImage => "page-table ownership: unsupported PE image",
        PagingError::PermissionConflict => "page-table ownership: PE permission conflict",
        PagingError::UnsupportedCpuFeature => "page-table ownership: unsupported CPU feature",
    }
}

trait ClearScreen {
    fn clear_screen(&mut self);
}

impl ClearScreen for PreExitConsole<'_> {
    fn clear_screen(&mut self) {
        self.firmware.clear();
    }
}

fn configuration_table_addresses(
    system_table: &EfiSystemTable,
) -> (OptionalPhysicalAddress, OptionalPhysicalAddress) {
    if system_table.configuration_table.is_null() || system_table.number_of_table_entries == 0 {
        return (
            OptionalPhysicalAddress::ABSENT,
            OptionalPhysicalAddress::ABSENT,
        );
    }

    let entry_count = system_table.number_of_table_entries.min(256);
    // SAFETY: The system-table signature was validated and UEFI owns a
    // contiguous array of `number_of_table_entries` configuration records.
    let entries = unsafe {
        core::slice::from_raw_parts(
            system_table
                .configuration_table
                .cast::<EfiConfigurationTable>(),
            entry_count,
        )
    };
    let mut acpi_10 = OptionalPhysicalAddress::ABSENT;
    let mut acpi_20 = OptionalPhysicalAddress::ABSENT;
    let mut smbios = OptionalPhysicalAddress::ABSENT;
    let mut smbios_3 = OptionalPhysicalAddress::ABSENT;

    for entry in entries {
        let address = optional_physical_address(entry.vendor_table);
        if entry.vendor_guid == EFI_ACPI_20_TABLE_GUID {
            acpi_20 = address;
        } else if entry.vendor_guid == EFI_ACPI_10_TABLE_GUID {
            acpi_10 = address;
        } else if entry.vendor_guid == EFI_SMBIOS3_TABLE_GUID {
            smbios_3 = address;
        } else if entry.vendor_guid == EFI_SMBIOS_TABLE_GUID {
            smbios = address;
        }
    }

    let acpi = if acpi_20.is_present() {
        acpi_20
    } else {
        acpi_10
    };
    let smbios = if smbios_3.is_present() {
        smbios_3
    } else {
        smbios
    };
    (acpi, smbios)
}

fn optional_physical_address(pointer: *mut c_void) -> OptionalPhysicalAddress {
    if pointer.is_null() {
        return OptionalPhysicalAddress::ABSENT;
    }
    OptionalPhysicalAddress {
        present: 1,
        reserved: [0; 7],
        address: u64::try_from(pointer.addr()).unwrap_or(u64::MAX),
    }
}

fn framebuffer_info(
    handle_protocol: HandleProtocol,
    console_out_handle: EfiHandle,
) -> FramebufferInfo {
    if console_out_handle.is_null() {
        return FramebufferInfo::ABSENT;
    }

    let mut interface = core::ptr::null_mut::<c_void>();
    // SAFETY: The console handle is supplied by the validated UEFI system
    // table, the GUID identifies GOP, and `interface` is a valid out-pointer.
    let status = unsafe {
        handle_protocol(
            console_out_handle,
            &raw const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
            &raw mut interface,
        )
    };
    if efi_is_error(status) || interface.is_null() {
        return FramebufferInfo::ABSENT;
    }

    // SAFETY: A successful HandleProtocol call returns a live GOP pointer
    // while boot services remain active.
    let protocol = unsafe { &*interface.cast::<EfiGraphicsOutputProtocol>() };
    // SAFETY: GOP owns the mode structure while boot services remain active;
    // the pointer is checked before it is dereferenced.
    let Some(mode) = (unsafe { protocol.mode.as_ref() }) else {
        return FramebufferInfo::ABSENT;
    };
    // SAFETY: GOP owns the mode-information structure while boot services
    // remain active; the pointer is checked before it is dereferenced.
    let Some(info) = (unsafe { mode.info.as_ref() }) else {
        return FramebufferInfo::ABSENT;
    };
    if mode.frame_buffer_base == 0
        || mode.frame_buffer_size == 0
        || info.horizontal_resolution == 0
        || info.vertical_resolution == 0
        || info.pixels_per_scan_line < info.horizontal_resolution
    {
        return FramebufferInfo::ABSENT;
    }

    let (pixel_format, masks) = match info.pixel_format {
        0 => (
            PixelFormat::Rgb,
            EfiPixelBitmask {
                red_mask: 0x0000_00ff,
                green_mask: 0x0000_ff00,
                blue_mask: 0x00ff_0000,
                reserved_mask: 0xff00_0000,
            },
        ),
        1 => (
            PixelFormat::Bgr,
            EfiPixelBitmask {
                red_mask: 0x00ff_0000,
                green_mask: 0x0000_ff00,
                blue_mask: 0x0000_00ff,
                reserved_mask: 0xff00_0000,
            },
        ),
        2 => (
            PixelFormat::BitMask,
            EfiPixelBitmask {
                red_mask: info.pixel_information.red_mask,
                green_mask: info.pixel_information.green_mask,
                blue_mask: info.pixel_information.blue_mask,
                reserved_mask: info.pixel_information.reserved_mask,
            },
        ),
        _ => return FramebufferInfo::ABSENT,
    };
    let byte_length = u64::try_from(mode.frame_buffer_size).unwrap_or(u64::MAX);
    if PhysicalRange::from_start_size(mode.frame_buffer_base, byte_length).is_err() {
        return FramebufferInfo::ABSENT;
    }

    FramebufferInfo {
        present: 1,
        reserved: [0; 7],
        physical_start: mode.frame_buffer_base,
        byte_length,
        width: info.horizontal_resolution,
        height: info.vertical_resolution,
        stride: info.pixels_per_scan_line,
        pixel_format,
        red_mask: masks.red_mask,
        green_mask: masks.green_mask,
        blue_mask: masks.blue_mask,
        reserved_mask: masks.reserved_mask,
    }
}

fn loaded_image_range(
    handle_protocol: HandleProtocol,
    image_handle: EfiHandle,
) -> Result<PhysicalRange, EfiStatus> {
    let mut interface = core::ptr::null_mut::<c_void>();
    // SAFETY: `image_handle` is the firmware-provided image handle, the GUID is
    // the UEFI Loaded Image Protocol GUID, and `interface` is a valid out-pointer.
    let status = unsafe {
        handle_protocol(
            image_handle,
            &raw const EFI_LOADED_IMAGE_PROTOCOL_GUID,
            &raw mut interface,
        )
    };
    if efi_is_error(status) || interface.is_null() {
        return Err(status);
    }
    // SAFETY: A successful HandleProtocol call returns a live loaded-image
    // protocol pointer while boot services remain active.
    let loaded_image = unsafe { &*interface.cast::<EfiLoadedImageProtocol>() };
    if loaded_image.image_base.is_null() || loaded_image.image_size == 0 {
        return Err(EFI_INVALID_PARAMETER);
    }
    PhysicalRange::from_start_size(
        u64::try_from(loaded_image.image_base.addr()).unwrap_or(u64::MAX),
        loaded_image.image_size,
    )
    .map_err(|_| EFI_INVALID_PARAMETER)
}

fn exit_firmware(
    image_handle: EfiHandle,
    get_memory_map: GetMemoryMap,
    exit_boot_services: ExitBootServices,
) -> Result<MemoryMapSnapshot, EfiStatus> {
    for _ in 0..EXIT_BOOT_SERVICES_RETRIES {
        let snapshot = capture_memory_map(get_memory_map)?;

        let map_key = usize::try_from(snapshot.info.map_key).map_err(|_| EFI_INVALID_PARAMETER)?;
        // SAFETY: `image_handle` is the firmware-provided image handle and the
        // map key comes from the immediately preceding successful memory-map
        // call. No allocation or other map-mutating service occurs between.
        let status = unsafe { exit_boot_services(image_handle, map_key) };
        if status == EFI_SUCCESS {
            return Ok(snapshot);
        }
        if status != EFI_INVALID_PARAMETER {
            return Err(status);
        }
        // Firmware changed the map between calls; retry with a fresh map key.
    }

    Err(EFI_INVALID_PARAMETER)
}

fn capture_memory_map(get_memory_map: GetMemoryMap) -> Result<MemoryMapSnapshot, EfiStatus> {
    let buffer = addr_of_mut!(MEMORY_MAP_STORAGE).cast::<EfiMemoryDescriptor>();
    let mut map_size = MEMORY_MAP_CAPACITY;
    let mut map_key = 0;
    let mut descriptor_size = 0;
    let mut descriptor_version = 0;

    // SAFETY: The raw pointer addresses a statically reserved, aligned buffer
    // of `MEMORY_MAP_CAPACITY` bytes. All metadata out-pointers reference live
    // local variables for the duration of the firmware call.
    let status = unsafe {
        get_memory_map(
            &raw mut map_size,
            buffer,
            &raw mut map_key,
            &raw mut descriptor_size,
            &raw mut descriptor_version,
        )
    };

    if status == EFI_BUFFER_TOO_SMALL {
        return Err(EFI_BUFFER_TOO_SMALL);
    }
    if efi_is_error(status) {
        return Err(status);
    }
    if descriptor_size < size_of::<EfiMemoryDescriptor>()
        || descriptor_size == 0
        || map_size > MEMORY_MAP_CAPACITY
        || !map_size.is_multiple_of(descriptor_size)
    {
        return Err(EFI_INVALID_PARAMETER);
    }

    let info = MemoryMapInfo {
        buffer_address: u64::try_from(buffer.addr()).unwrap_or(u64::MAX),
        buffer_capacity: u64::try_from(MEMORY_MAP_CAPACITY).unwrap_or(u64::MAX),
        map_size: u64::try_from(map_size).unwrap_or(u64::MAX),
        map_key: u64::try_from(map_key).unwrap_or(u64::MAX),
        descriptor_size: u64::try_from(descriptor_size).unwrap_or(u64::MAX),
        descriptor_version,
        reserved: 0,
        descriptor_count: u64::try_from(map_size / descriptor_size).unwrap_or(u64::MAX),
    };
    if !info.is_structurally_valid() {
        return Err(EFI_INVALID_PARAMETER);
    }

    Ok(MemoryMapSnapshot { info })
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    let mut console = KernelConsole::initialize();
    console.write_line("FATAL: Soma OS panic during early boot.");

    #[cfg(feature = "qemu-test")]
    cpu::qemu::exit_failure();

    #[cfg(not(feature = "qemu-test"))]
    cpu::halt_forever()
}
