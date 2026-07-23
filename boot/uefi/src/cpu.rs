#![allow(clippy::module_name_repetitions, clippy::similar_names)]

use core::arch::{asm, global_asm};
use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU8, Ordering};

const KERNEL_CODE_SELECTOR: u16 = 0x08;
const KERNEL_DATA_SELECTOR: u16 = 0x10;
const TSS_SELECTOR: u16 = 0x18;
const DOUBLE_FAULT_IST: u8 = 1;
const KERNEL_STACK_SIZE: usize = 64 * 1024;
const DOUBLE_FAULT_STACK_SIZE: usize = 32 * 1024;
const IDT_ENTRY_COUNT: usize = 256;

#[repr(C, align(16))]
struct Stack([u8; KERNEL_STACK_SIZE]);

#[repr(C, align(16))]
struct DoubleFaultStack([u8; DOUBLE_FAULT_STACK_SIZE]);

static mut KERNEL_STACK: Stack = Stack([0; KERNEL_STACK_SIZE]);
static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; DOUBLE_FAULT_STACK_SIZE]);

#[repr(C, packed)]
struct TaskStateSegment {
    reserved_1: u32,
    privilege_stack_table: [u64; 3],
    reserved_2: u64,
    interrupt_stack_table: [u64; 7],
    reserved_3: u64,
    reserved_4: u16,
    io_map_base: u16,
}

impl TaskStateSegment {
    #[allow(clippy::cast_possible_truncation)]
    const fn new() -> Self {
        Self {
            reserved_1: 0,
            privilege_stack_table: [0; 3],
            reserved_2: 0,
            interrupt_stack_table: [0; 7],
            reserved_3: 0,
            reserved_4: 0,
            io_map_base: size_of::<Self>() as u16,
        }
    }
}

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attributes: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attributes: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    const fn interrupt_gate(handler: u64, ist: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector: KERNEL_CODE_SELECTOR,
            ist: ist & 0x07,
            type_attributes: 0x8e,
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut GDT: [u64; 5] = [0; 5];
static mut IDT: [IdtEntry; IDT_ENTRY_COUNT] = [IdtEntry::missing(); IDT_ENTRY_COUNT];

#[unsafe(no_mangle)]
pub static SANJU_BREAKPOINT_SEEN: AtomicU8 = AtomicU8::new(0);

/// Evidence that the M2 CPU-protection setup executed successfully.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuProtectionReport {
    pub kernel_stack_active: bool,
    pub gdt_active: bool,
    pub tss_active: bool,
    pub idt_active: bool,
    pub breakpoint_self_test_passed: bool,
}

global_asm!(
    r#"
    .section .text

    .global sanju_breakpoint_stub
sanju_breakpoint_stub:
    push rax
    mov al, 1
    xchg byte ptr [rip + SANJU_BREAKPOINT_SEEN], al
    pop rax
    iretq

    .global sanju_double_fault_stub
sanju_double_fault_stub:
    mov rcx, 8
    mov rdx, [rsp]
    xor r8, r8
    and rsp, -16
    sub rsp, 32
    call sanju_fatal_exception_dispatch
    ud2

    .global sanju_general_protection_stub
sanju_general_protection_stub:
    mov rcx, 13
    mov rdx, [rsp]
    xor r8, r8
    and rsp, -16
    sub rsp, 32
    call sanju_fatal_exception_dispatch
    ud2

    .global sanju_page_fault_stub
sanju_page_fault_stub:
    mov rcx, 14
    mov rdx, [rsp]
    mov r8, cr2
    and rsp, -16
    sub rsp, 32
    call sanju_fatal_exception_dispatch
    ud2
"#
);

unsafe extern "C" {
    fn sanju_breakpoint_stub();
    fn sanju_double_fault_stub();
    fn sanju_general_protection_stub();
    fn sanju_page_fault_stub();
}

/// Moves execution to the statically reserved M2 kernel stack.
///
/// # Safety
///
/// This must be called only after firmware boot services have been exited. The
/// supplied entry point must never return, because the previous firmware stack
/// is abandoned permanently.
pub unsafe fn switch_to_kernel_stack(entry: extern "efiapi" fn() -> !) -> ! {
    let stack_top = kernel_stack_top();

    // SAFETY: The stack is statically reserved and 16-byte aligned. The 32-byte
    // home area satisfies the x86-64 UEFI calling convention before the call.
    unsafe {
        asm!(
            "mov rsp, {stack_top}",
            "and rsp, -16",
            "sub rsp, 32",
            "xor rbp, rbp",
            "call {entry}",
            "ud2",
            stack_top = in(reg) stack_top,
            entry = in(reg) entry,
            options(noreturn)
        );
    }
}

/// Installs the GDT, TSS, protected exception stacks, and initial IDT.
///
/// # Safety
///
/// The caller must execute at x86-64 kernel privilege after switching to the
/// dedicated kernel stack. Interrupts must remain disabled until a complete
/// interrupt-controller policy is installed.
#[must_use]
pub unsafe fn initialize() -> CpuProtectionReport {
    // SAFETY: The caller owns early CPU initialization and no other core can
    // access these tables during the single-core M2 boot path.
    unsafe {
        install_gdt_and_tss();
        install_idt();
    }

    SANJU_BREAKPOINT_SEEN.store(0, Ordering::SeqCst);
    // SAFETY: Vector 3 now points at a handler that preserves RAX, marks the
    // self-test flag, and returns with `iretq`.
    unsafe {
        asm!("int3", options(nomem, nostack));
    }

    CpuProtectionReport {
        kernel_stack_active: true,
        gdt_active: true,
        tss_active: true,
        idt_active: true,
        breakpoint_self_test_passed: SANJU_BREAKPOINT_SEEN.load(Ordering::SeqCst) == 1,
    }
}

unsafe fn install_gdt_and_tss() {
    let tss = TaskStateSegment {
        privilege_stack_table: [u64::try_from(kernel_stack_top()).unwrap_or(u64::MAX), 0, 0],
        interrupt_stack_table: [
            u64::try_from(double_fault_stack_top()).unwrap_or(u64::MAX),
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        ..TaskStateSegment::new()
    };
    // SAFETY: Early boot has exclusive ownership of the static TSS.
    unsafe {
        addr_of_mut!(TSS).write(tss);
    }

    let tss_base = u64::try_from(addr_of!(TSS).addr()).unwrap_or(u64::MAX);
    let (tss_low, tss_high) = tss_descriptor(tss_base);
    let gdt = addr_of_mut!(GDT).cast::<u64>();

    // SAFETY: `GDT` contains exactly five entries and is exclusively owned.
    unsafe {
        gdt.write(0);
        gdt.add(1).write(0x00af_9a00_0000_ffff);
        gdt.add(2).write(0x00cf_9200_0000_ffff);
        gdt.add(3).write(tss_low);
        gdt.add(4).write(tss_high);
    }

    let gdtr = DescriptorTablePointer {
        limit: u16::try_from(size_of::<[u64; 5]>() - 1).unwrap_or(u16::MAX),
        base: u64::try_from(addr_of!(GDT).addr()).unwrap_or(u64::MAX),
    };

    // SAFETY: The descriptor pointer references the static GDT, the selectors
    // match its entries, and TSS selector 0x18 spans entries three and four.
    unsafe {
        asm!(
            "lgdt [{gdtr}]",
            "push {code_selector}",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, {data_selector}",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ax, {tss_selector}",
            "ltr ax",
            gdtr = in(reg) addr_of!(gdtr),
            code_selector = const KERNEL_CODE_SELECTOR,
            data_selector = const KERNEL_DATA_SELECTOR,
            tss_selector = const TSS_SELECTOR,
            out("rax") _,
        );
    }
}

unsafe fn install_idt() {
    let mut idt = [IdtEntry::missing(); IDT_ENTRY_COUNT];
    idt[3] = IdtEntry::interrupt_gate(handler_address(sanju_breakpoint_stub), 0);
    idt[8] = IdtEntry::interrupt_gate(handler_address(sanju_double_fault_stub), DOUBLE_FAULT_IST);
    idt[13] = IdtEntry::interrupt_gate(handler_address(sanju_general_protection_stub), 0);
    idt[14] = IdtEntry::interrupt_gate(handler_address(sanju_page_fault_stub), 0);

    // SAFETY: Early boot has exclusive ownership of the static IDT.
    unsafe {
        addr_of_mut!(IDT).write(idt);
    }

    let idtr = DescriptorTablePointer {
        limit: u16::try_from(size_of::<[IdtEntry; IDT_ENTRY_COUNT]>() - 1).unwrap_or(u16::MAX),
        base: u64::try_from(addr_of!(IDT).addr()).unwrap_or(u64::MAX),
    };

    // SAFETY: `idtr` references the fully initialized static IDT.
    unsafe {
        asm!("lidt [{idtr}]", idtr = in(reg) addr_of!(idtr), options(readonly, nostack));
    }
}

#[allow(clippy::fn_to_numeric_cast)]
fn handler_address(handler: unsafe extern "C" fn()) -> u64 {
    u64::try_from(handler as usize).unwrap_or(u64::MAX)
}

fn tss_descriptor(base: u64) -> (u64, u64) {
    let limit = u64::try_from(size_of::<TaskStateSegment>() - 1).unwrap_or(u64::MAX);
    let low = (limit & 0xffff)
        | ((base & 0x00ff_ffff) << 16)
        | (0x89_u64 << 40)
        | (((limit >> 16) & 0x0f) << 48)
        | (((base >> 24) & 0xff) << 56);
    let high = base >> 32;
    (low, high)
}

fn kernel_stack_top() -> usize {
    unsafe { addr_of_mut!(KERNEL_STACK.0).cast::<u8>().addr() + KERNEL_STACK_SIZE }
}

fn double_fault_stack_top() -> usize {
    unsafe { addr_of_mut!(DOUBLE_FAULT_STACK.0).cast::<u8>().addr() + DOUBLE_FAULT_STACK_SIZE }
}

#[unsafe(no_mangle)]
extern "efiapi" fn sanju_fatal_exception_dispatch(
    vector: u64,
    error_code: u64,
    fault_address: u64,
) -> ! {
    debug_write_line("FATAL: CPU exception");
    debug_write_label_hex("Vector: ", vector);
    debug_write_label_hex("Error code: ", error_code);
    if vector == 14 {
        debug_write_label_hex("Page-fault address: ", fault_address);
    }

    #[cfg(feature = "qemu-test")]
    qemu_exit_failure();

    #[cfg(not(feature = "qemu-test"))]
    halt()
}

#[allow(clippy::cast_possible_truncation)]
fn debug_write_label_hex(label: &str, value: u64) {
    debug_write(label.as_bytes());
    debug_write(b"0x");
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0x0f) as u8;
        let byte = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        debug_byte(byte);
    }
    debug_write(b"\r\n");
}

fn debug_write_line(text: &str) {
    debug_write(text.as_bytes());
    debug_write(b"\r\n");
}

fn debug_write(bytes: &[u8]) {
    for byte in bytes {
        debug_byte(*byte);
    }
}

fn debug_byte(byte: u8) {
    const COM1: u16 = 0x03f8;
    const LINE_STATUS: u16 = COM1 + 5;
    for _ in 0..100_000 {
        // SAFETY: COM1 is the configured M2 early serial device.
        if unsafe { inb(LINE_STATUS) } & 0x20 != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    // SAFETY: COM1 is the configured M2 early serial device.
    unsafe {
        outb(COM1, byte);
    }

    #[cfg(feature = "qemu-test")]
    // SAFETY: The smoke-test machine maps the debug console at port 0xE9.
    unsafe {
        outb(0x00e9, byte);
    }
}

#[cfg(feature = "qemu-test")]
fn qemu_exit_failure() -> ! {
    // SAFETY: The smoke-test machine maps `isa-debug-exit` at port 0xF4.
    unsafe {
        outl(0x00f4, 0x11);
    }
    halt()
}

fn halt() -> ! {
    loop {
        // SAFETY: Fatal exception handling is a terminal kernel state.
        unsafe {
            asm!("cli", "hlt", options(nomem, nostack));
        }
    }
}

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: The caller owns the specified x86 I/O port.
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[cfg(feature = "qemu-test")]
unsafe fn outl(port: u16, value: u32) {
    // SAFETY: The caller owns the specified x86 I/O port.
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: The caller owns the specified x86 I/O port.
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
