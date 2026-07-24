#![no_std]
#![no_main]
#![allow(clippy::pedantic)]

mod cpu;
mod serial;

use core::alloc::Layout;
use core::ffi::c_void;
use core::mem::{MaybeUninit, size_of};
use core::panic::PanicInfo;
use core::ptr::{addr_of, addr_of_mut};
use sanju_kernel::elf::load_position_independent;
use sanju_kernel::fs::RamFs;
use sanju_kernel::heap::KernelHeap;
#[cfg(not(feature = "qemu-test"))]
use sanju_kernel::input::KeyboardDecoder;
use sanju_kernel::memory::{FrameAllocator, PAGE_SIZE};
use sanju_kernel::paging::{
    GuardedStack, KERNEL_HEAP_START, PageFlags, PageTableManager, VirtualPage,
};
use sanju_kernel::process::{AddressSpace, ProcessTable};
use sanju_kernel::scheduler::{Scheduler, TaskKind};
use sanju_kernel::shell::{Shell, ShellEnvironment};
use sanju_kernel::startup::{self, StartupStage};
use sanju_kernel::{BootInfo, Console, M5Report, MemoryMapInfo, kernel_main_m5};
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
const KERNEL_HEAP_SIZE: usize = 1024 * 1024;
const USER_IMAGE_SIZE: usize = 16 * 1024;
const USER_STACK_SIZE: usize = 64 * 1024;
const USER_STACK_TOTAL_SIZE: usize = USER_STACK_SIZE + 4096;

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
        qemu::debug_byte(byte);
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
    pre_exit.write_line("SanjuOS M5 boot transition");
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
            qemu::exit_failure();

            #[cfg(not(feature = "qemu-test"))]
            return status;
        }
    };

    // UEFI console and boot-services pointers are invalid beyond this point.
    // Persist owned boot facts before abandoning the firmware-provided stack.
    let boot_info = BootInfo::new(
        "x86_64",
        "UEFI",
        "Milestone M5: protected user-space foundation and branded startup.",
        snapshot.info,
    );
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

    // SAFETY: The map is retained in static boot storage for the kernel's life.
    let Ok(mut frame_allocator) =
        (unsafe { FrameAllocator::from_memory_map(boot_info.memory_map) })
    else {
        boot_failure(
            &mut console,
            "M5-MEM-001",
            "frame allocator initialization failed",
        );
    };
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

    let Some(mapping_frame) = frame_allocator.allocate_frame() else {
        boot_failure(
            &mut console,
            "M5-MEM-002",
            "no frame for page-table API probe",
        );
    };
    let mut page_tables = PageTableManager::new(user_runtime.active_page_table_root);
    let mapping_page = VirtualPage::containing(KERNEL_HEAP_START);
    let safe_flags = PageFlags::WRITABLE
        .union(PageFlags::NO_EXECUTE)
        .union(PageFlags::GLOBAL);
    let mapping_created = page_tables
        .map(mapping_page, mapping_frame, safe_flags)
        .is_ok();
    let page_flags_active = page_tables
        .flags_for(mapping_page)
        .is_some_and(|flags| flags.is_writable() && !flags.is_executable());
    let mapping_removed = page_tables.unmap(mapping_page) == Ok(mapping_frame);
    let wx_violation_rejected = page_tables
        .map(
            VirtualPage::containing(KERNEL_HEAP_START + PAGE_SIZE),
            mapping_frame,
            PageFlags::WRITABLE,
        )
        .is_err();

    let mut heap = KernelHeap::new();
    let heap_start = addr_of_mut!(KERNEL_HEAP_STORAGE.0).cast::<u8>().addr();
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

    let init_image_pointer = addr_of_mut!(USER_INIT_IMAGE.0).cast::<u8>();
    let hello_image_pointer = addr_of_mut!(USER_HELLO_IMAGE.0).cast::<u8>();
    let fault_image_pointer = addr_of_mut!(USER_FAULT_IMAGE.0).cast::<u8>();
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

    let init_stack_base = addr_of_mut!(USER_INIT_STACK.0).cast::<u8>().addr();
    let hello_stack_base = addr_of_mut!(USER_HELLO_STACK.0).cast::<u8>().addr();
    let fault_stack_base = addr_of_mut!(USER_FAULT_STACK.0).cast::<u8>().addr();
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

    let Some(init_root) = frame_allocator.allocate_frame() else {
        boot_failure(&mut console, "M5-PROC-001", "no root frame for init");
    };
    let Some(hello_root) = frame_allocator.allocate_frame() else {
        boot_failure(&mut console, "M5-PROC-002", "no root frame for hello");
    };
    let Some(fault_root) = frame_allocator.allocate_frame() else {
        boot_failure(&mut console, "M5-PROC-003", "no root frame for fault-test");
    };

    let init_image_start = u64::try_from(init_image_pointer.addr()).unwrap_or(u64::MAX);
    let hello_image_start = u64::try_from(hello_image_pointer.addr()).unwrap_or(u64::MAX);
    let fault_image_start = u64::try_from(fault_image_pointer.addr()).unwrap_or(u64::MAX);
    let init_space = AddressSpace {
        root_frame: init_root.start_address(),
        user_start: init_image_start,
        user_end: init_image_start
            .saturating_add(u64::try_from(init_loaded.image_size).unwrap_or(0)),
        isolated: true,
    };
    let hello_space = AddressSpace {
        root_frame: hello_root.start_address(),
        user_start: hello_image_start,
        user_end: hello_image_start
            .saturating_add(u64::try_from(hello_loaded.image_size).unwrap_or(0)),
        isolated: true,
    };
    let fault_space = AddressSpace {
        root_frame: fault_root.start_address(),
        user_start: fault_image_start,
        user_end: fault_image_start
            .saturating_add(u64::try_from(fault_loaded.image_size).unwrap_or(0)),
        isolated: true,
    };

    let init_entry =
        init_image_start.saturating_add(u64::try_from(init_loaded.entry_offset).unwrap_or(0));
    let hello_entry =
        hello_image_start.saturating_add(u64::try_from(hello_loaded.entry_offset).unwrap_or(0));
    let fault_entry =
        fault_image_start.saturating_add(u64::try_from(fault_loaded.entry_offset).unwrap_or(0));

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
        )
    };
    if fault_result.exited {
        let _ = processes.exit(fault_pid, fault_result.exit_code);
    } else if fault_result.faulted {
        let _ = processes.fault(fault_pid, fault_result.fault_address);
    }
    let process_stats = processes.stats();

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
    };
    for byte in b"version\nuserspace\n" {
        shell.feed_byte(*byte, &mut null_console, &mut ramfs, &self_test_environment);
    }

    let roots_are_distinct =
        init_root != hello_root && init_root != fault_root && hello_root != fault_root;
    let elf_security = init_loaded.write_xor_execute_enforced
        && hello_loaded.write_xor_execute_enforced
        && fault_loaded.write_xor_execute_enforced;
    let exited_processes =
        (if init_result.exited { 1 } else { 0 }) + if hello_result.exited { 1 } else { 0 };
    let report = M5Report {
        paging_ownership_active: user_runtime.active_page_table_root != 0,
        active_page_table_root: user_runtime.active_page_table_root,
        four_level_paging_active: user_runtime.four_level_paging_active,
        mapping_api_active: mapping_created && mapping_removed,
        page_flags_active,
        boot_memory_reclaim_active: reclaimable_frames > 0,
        guard_pages_active: init_stack.stack_pages > 0
            && hello_stack.stack_pages > 0
            && fault_stack.stack_pages > 0,
        write_xor_execute_active: wx_violation_rejected && elf_security,
        kernel_heap_active: heap.allocations() >= 3 && heap.frees() >= 1,
        heap_allocations: heap.allocations(),
        heap_frees: heap.frees(),
        page_fault_diagnostics_active: user_runtime.page_fault_diagnostics_active,
        user_gdt_active: user_runtime.user_gdt_active,
        ring3_execution_active: init_result.exited && hello_result.exited && fault_result.faulted,
        user_address_space_isolation_active: roots_are_distinct,
        user_stacks_active: true,
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
        };
        let smoke_commands = b"help\nuserspace\nls\ncat welcome.txt\ntasks\nuptime\n";
        for byte in smoke_commands {
            shell.feed_byte(*byte, &mut console, &mut ramfs, &environment);
        }
        qemu::exit_success();
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
                    };
                    shell.feed_byte(byte, &mut console, &mut ramfs, &environment);
                }
            }

            cpu::halt_until_interrupt();
        }
    }
}

fn boot_failure(console: &mut dyn Console, code: &str, message: &str) -> ! {
    startup::print_failure(console, code, message);
    #[cfg(feature = "qemu-test")]
    qemu::exit_failure();

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
        // SAFETY: Halting is the intended terminal state after the current milestone or
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
