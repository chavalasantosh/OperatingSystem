#![no_std]
#![no_main]

mod serial;

use core::ffi::c_void;
use core::mem::size_of;
use core::panic::PanicInfo;
use core::ptr::addr_of_mut;
use sanju_kernel::{BootInfo, Console, MemoryMapInfo, kernel_main};
use serial::SerialConsole;

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
    handle_protocol: usize,
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
        qemu::debug_byte(byte);
    }
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

    let Some(mut firmware_console) = UefiConsole::new(system_table.console_out) else {
        return EFI_INVALID_PARAMETER;
    };
    let mut kernel_console = KernelConsole::initialize();
    let mut pre_exit = PreExitConsole {
        firmware: &mut firmware_console,
        early: &mut kernel_console,
    };

    pre_exit.clear_screen();
    pre_exit.write_line("SanjuOS M1 boot transition");
    pre_exit.write_line("Capturing UEFI memory map...");

    let get_memory_map = boot_services.get_memory_map;
    let exit_boot_services = boot_services.exit_boot_services;

    let snapshot = match exit_firmware(image_handle, get_memory_map, exit_boot_services) {
        Ok(snapshot) => snapshot,
        Err(status) => {
            pre_exit.write_line("FATAL: firmware ownership transition failed.");
            #[cfg(feature = "qemu-test")]
            qemu::exit_failure();

            #[cfg(not(feature = "qemu-test"))]
            return status;
        }
    };

    // UEFI console and boot-services pointers are invalid beyond this point.
    // Only the serial/debug console and captured memory map may be used.
    let boot_info = BootInfo::new(
        "x86_64",
        "UEFI",
        "Milestone M1: firmware exit and kernel ownership.",
        snapshot.info,
    );
    kernel_main(&mut kernel_console, boot_info);

    #[cfg(feature = "qemu-test")]
    qemu::exit_success();

    #[cfg(not(feature = "qemu-test"))]
    halt_forever()
}

trait ClearScreen {
    fn clear_screen(&mut self);
}

impl ClearScreen for PreExitConsole<'_> {
    fn clear_screen(&mut self) {
        self.firmware.clear();
    }
}

fn exit_firmware(
    image_handle: EfiHandle,
    get_memory_map: GetMemoryMap,
    exit_boot_services: ExitBootServices,
) -> Result<MemoryMapSnapshot, EfiStatus> {
    for _ in 0..EXIT_BOOT_SERVICES_RETRIES {
        let snapshot = capture_memory_map(get_memory_map)?;

        // SAFETY: `image_handle` is the firmware-provided image handle and the
        // map key comes from the immediately preceding successful memory-map
        // call. No allocation or other map-mutating service occurs between.
        let status = unsafe { exit_boot_services(image_handle, snapshot.info.map_key) };
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
        buffer_address: buffer.addr(),
        buffer_capacity: MEMORY_MAP_CAPACITY,
        map_size,
        map_key,
        descriptor_size,
        descriptor_version,
        descriptor_count: map_size / descriptor_size,
    };
    if !info.is_structurally_valid() {
        return Err(EFI_INVALID_PARAMETER);
    }

    Ok(MemoryMapSnapshot { info })
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    let mut console = KernelConsole::initialize();
    console.write_line("FATAL: SanjuOS panic during early boot.");

    #[cfg(feature = "qemu-test")]
    qemu::exit_failure();

    #[cfg(not(feature = "qemu-test"))]
    halt_forever()
}

#[allow(dead_code)]
fn halt_forever() -> ! {
    loop {
        // SAFETY: Halting is the intended terminal state after M1 completes or
        // encounters a non-recoverable condition. Interrupts are disabled to
        // avoid entering firmware-installed handlers after boot-services exit.
        unsafe {
            core::arch::asm!("cli", "hlt", options(nomem, nostack));
        }
    }
}

#[cfg(feature = "qemu-test")]
mod qemu {
    use core::arch::asm;

    const DEBUG_PORT: u16 = 0x00e9;
    const EXIT_PORT: u16 = 0x00f4;
    const EXIT_SUCCESS: u32 = 0x10;
    const EXIT_FAILURE: u32 = 0x11;

    pub fn debug_byte(byte: u8) {
        // SAFETY: Enabled only for the QEMU test machine, where port 0xE9 is
        // explicitly configured as the debug console.
        unsafe {
            asm!(
                "out dx, al",
                in("dx") DEBUG_PORT,
                in("al") byte,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    pub fn exit_success() -> ! {
        exit(EXIT_SUCCESS)
    }

    pub fn exit_failure() -> ! {
        exit(EXIT_FAILURE)
    }

    fn exit(code: u32) -> ! {
        // SAFETY: The smoke-test QEMU machine configures `isa-debug-exit` at
        // port 0xF4. This module is omitted from physical-hardware builds.
        unsafe {
            asm!(
                "out dx, eax",
                in("dx") EXIT_PORT,
                in("eax") code,
                options(nomem, nostack, preserves_flags)
            );
        }

        loop {
            core::hint::spin_loop();
        }
    }
}
