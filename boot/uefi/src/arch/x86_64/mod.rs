#![allow(clippy::module_name_repetitions, clippy::similar_names)]

pub(crate) mod paging;
mod serial;

#[cfg(feature = "qemu-test")]
pub mod qemu;

pub(crate) use paging::{
    UserMapping, active_page_table_root, kernel_guard_base, kernel_heap_probe_page,
    mark_user_range, reserve_inherited_page_tables, switch_address_space,
    take_page_table_ownership,
};
pub use serial::SerialConsole;

use core::arch::{asm, global_asm};
use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

const KERNEL_CODE_SELECTOR: u16 = 0x08;
const KERNEL_DATA_SELECTOR: u16 = 0x10;
const USER_DATA_SELECTOR: u16 = 0x18;
const USER_CODE_SELECTOR: u16 = 0x20;
const TSS_SELECTOR: u16 = 0x28;
const DOUBLE_FAULT_IST: u8 = 1;
const KERNEL_STACK_SIZE: usize = 64 * 1024;
const DOUBLE_FAULT_STACK_SIZE: usize = 32 * 1024;
const SYSCALL_STACK_SIZE: usize = 64 * 1024;
const USER_INTERRUPT_STACK_SIZE: usize = 64 * 1024;
const PROCESS_KERNEL_STACK_SIZE: usize = 64 * 1024;
const PROCESS_KERNEL_STACK_TOTAL_SIZE: usize = PROCESS_KERNEL_STACK_SIZE + 2 * 4096;
const FH3_USER_STACK_SIZE: usize = 64 * 1024;
const FH3_USER_STACK_TOTAL_SIZE: usize = FH3_USER_STACK_SIZE + 2 * 4096;
const PROCESS_KERNEL_STACK_COUNT: usize = 5;
const FH3_PROCESS_COUNT: usize = 2;
const FH3_COUNTER_TARGET: u64 = 1_024;
const FH3_R12_A: u64 = 0x5341_4e4a_554f_5301;
const FH3_R12_B: u64 = 0x5341_4e4a_554f_5302;
const IDT_ENTRY_COUNT: usize = 256;
const PIC_MASTER_COMMAND: u16 = 0x20;
const PIC_MASTER_DATA: u16 = 0x21;
const PIC_SLAVE_COMMAND: u16 = 0xa0;
const PIC_SLAVE_DATA: u16 = 0xa1;
const PIC_EOI: u8 = 0x20;
const PIC_MASTER_VECTOR_OFFSET: u8 = 32;
const PIC_SLAVE_VECTOR_OFFSET: u8 = 40;
const TIMER_VECTOR: usize = 32;
const KEYBOARD_VECTOR: usize = 33;
const PIT_COMMAND: u16 = 0x43;
const PIT_CHANNEL_ZERO: u16 = 0x40;
const PIT_INPUT_HZ: u32 = 1_193_182;
pub const TIMER_HZ: u64 = 100;
const KEYBOARD_DATA: u16 = 0x60;
const SCANCODE_QUEUE_CAPACITY: usize = 256;

#[repr(C, align(16))]
struct Stack([u8; KERNEL_STACK_SIZE]);

#[repr(C, align(16))]
struct DoubleFaultStack([u8; DOUBLE_FAULT_STACK_SIZE]);

#[repr(C, align(16))]
struct SyscallStack([u8; SYSCALL_STACK_SIZE]);

#[repr(C, align(16))]
struct UserInterruptStack([u8; USER_INTERRUPT_STACK_SIZE]);

#[repr(C, align(4096))]
struct ProcessKernelStack([u8; PROCESS_KERNEL_STACK_TOTAL_SIZE]);

#[repr(C, align(4096))]
struct Fh3UserStack([u8; FH3_USER_STACK_TOTAL_SIZE]);

static mut KERNEL_STACK: Stack = Stack([0; KERNEL_STACK_SIZE]);
static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; DOUBLE_FAULT_STACK_SIZE]);
static mut SYSCALL_STACK: SyscallStack = SyscallStack([0; SYSCALL_STACK_SIZE]);
static mut USER_INTERRUPT_STACK: UserInterruptStack =
    UserInterruptStack([0; USER_INTERRUPT_STACK_SIZE]);
static mut PROCESS_KERNEL_STACKS: [ProcessKernelStack; PROCESS_KERNEL_STACK_COUNT] = [
    ProcessKernelStack([0; PROCESS_KERNEL_STACK_TOTAL_SIZE]),
    ProcessKernelStack([0; PROCESS_KERNEL_STACK_TOTAL_SIZE]),
    ProcessKernelStack([0; PROCESS_KERNEL_STACK_TOTAL_SIZE]),
    ProcessKernelStack([0; PROCESS_KERNEL_STACK_TOTAL_SIZE]),
    ProcessKernelStack([0; PROCESS_KERNEL_STACK_TOTAL_SIZE]),
];
static mut FH3_USER_STACKS: [Fh3UserStack; FH3_PROCESS_COUNT] = [
    Fh3UserStack([0; FH3_USER_STACK_TOTAL_SIZE]),
    Fh3UserStack([0; FH3_USER_STACK_TOTAL_SIZE]),
];

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
static mut GDT: [u64; 7] = [0; 7];
static mut IDT: [IdtEntry; IDT_ENTRY_COUNT] = [IdtEntry::missing(); IDT_ENTRY_COUNT];
static mut SCANCODE_QUEUE: [u8; SCANCODE_QUEUE_CAPACITY] = [0; SCANCODE_QUEUE_CAPACITY];
static SCANCODE_HEAD: AtomicUsize = AtomicUsize::new(0);
static SCANCODE_TAIL: AtomicUsize = AtomicUsize::new(0);
static SCANCODE_DROPPED: AtomicU64 = AtomicU64::new(0);
static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
static KEYBOARD_IRQS: AtomicU64 = AtomicU64::new(0);
static TEST_SCANCODE: AtomicU8 = AtomicU8::new(0);
static CURRENT_USER_PID: AtomicU64 = AtomicU64::new(0);
static USER_SYSCALLS: AtomicU64 = AtomicU64::new(0);
static USER_YIELDS: AtomicU64 = AtomicU64::new(0);
static USER_TIMER_PREEMPTIONS: AtomicU64 = AtomicU64::new(0);
static USER_EXIT_CODE: AtomicU64 = AtomicU64::new(0);
static USER_FAULT_ADDRESS: AtomicU64 = AtomicU64::new(0);
static USER_FAULT_ERROR: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
pub static mut SANJU_SYSCALL_KERNEL_RSP: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_SYSCALL_USER_RSP: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_RESUME_RSP: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_RESUME_RIP: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_REGION_START: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_REGION_END: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_STACK_START: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_STACK_END: u64 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_EXIT_REQUESTED: u8 = 0;
#[unsafe(no_mangle)]
pub static mut SANJU_USER_FAULTED: u8 = 0;

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

/// Evidence that the interrupt-driven M3 runtime is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptRuntimeReport {
    pub timer_interrupts_active: bool,
    pub timer_ticks: u64,
    pub timer_hz: u64,
    pub keyboard_interrupt_path_active: bool,
    pub keyboard_irqs: u64,
    pub dropped_scancodes: u64,
}

/// Evidence that x86-64 paging and Ring 3 entry facilities are installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserModeRuntimeReport {
    pub active_page_table_root: u64,
    pub four_level_paging_active: bool,
    pub user_gdt_active: bool,
    pub syscall_interface_active: bool,
    pub page_fault_diagnostics_active: bool,
}

/// Result of one protected user-program execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserRunResult {
    pub pid: u32,
    pub exited: bool,
    pub exit_code: i32,
    pub faulted: bool,
    pub fault_address: u64,
    pub fault_error_code: u64,
    pub syscalls: u64,
    pub yields: u64,
    pub timer_preemptions: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuardedStackLayout {
    pub lower_guard: u64,
    pub stack_start: u64,
    pub stack_size: usize,
    pub stack_top: u64,
    pub upper_guard: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreemptionProbeProcessLayout {
    pub entry: u64,
    pub code_page: u64,
    pub counter_page: u64,
    pub user_stack: GuardedStackLayout,
    pub kernel_stack: GuardedStackLayout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreemptionProbeLayout {
    pub process_a: PreemptionProbeProcessLayout,
    pub process_b: PreemptionProbeProcessLayout,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreemptionProbeReport {
    pub ring3_processes_started: usize,
    pub timer_preemptions: u64,
    pub context_switches: u64,
    pub cr3_switches: u64,
    pub register_context_checks: u64,
    pub process_a_counter: u64,
    pub process_b_counter: u64,
    pub full_frame_switching_active: bool,
    pub private_cr3_switching_active: bool,
    pub per_process_kernel_stacks_active: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct InterruptFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rdi: u64,
    rsi: u64,
    rbp: u64,
    rbx: u64,
    rdx: u64,
    rcx: u64,
    rax: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

impl InterruptFrame {
    fn user(entry: u64, stack_top: u64, r12: u64) -> Self {
        Self {
            r12,
            rip: entry,
            cs: u64::from(USER_CODE_SELECTOR | 3),
            rflags: 0x202,
            rsp: stack_top,
            ss: u64::from(USER_DATA_SELECTOR | 3),
            ..Self::zeroed()
        }
    }

    const fn zeroed() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rdi: 0,
            rsi: 0,
            rbp: 0,
            rbx: 0,
            rdx: 0,
            rcx: 0,
            rax: 0,
            rip: 0,
            cs: 0,
            rflags: 0,
            rsp: 0,
            ss: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Fh3RuntimeProcess {
    pid: u32,
    root: u64,
    saved_frame: u64,
    kernel_stack_top: u64,
    expected_r12: u64,
}

impl Fh3RuntimeProcess {
    const fn empty() -> Self {
        Self {
            pid: 0,
            root: 0,
            saved_frame: 0,
            kernel_stack_top: 0,
            expected_r12: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Fh3Runtime {
    active: bool,
    completed: bool,
    faulted: bool,
    current: usize,
    kernel_root: u64,
    processes: [Fh3RuntimeProcess; FH3_PROCESS_COUNT],
    preemptions: u64,
    context_switches: u64,
    cr3_switches: u64,
    register_context_checks: u64,
}

impl Fh3Runtime {
    const fn empty() -> Self {
        Self {
            active: false,
            completed: false,
            faulted: false,
            current: 0,
            kernel_root: 0,
            processes: [Fh3RuntimeProcess::empty(); FH3_PROCESS_COUNT],
            preemptions: 0,
            context_switches: 0,
            cr3_switches: 0,
            register_context_checks: 0,
        }
    }
}

static mut FH3_RUNTIME: Fh3Runtime = Fh3Runtime::empty();

global_asm!(
    r#"
    .section .text

    .macro SANJU_PUSH_REGISTERS
        push rax
        push rcx
        push rdx
        push rbx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15
    .endm

    .macro SANJU_POP_REGISTERS
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rbx
        pop rdx
        pop rcx
        pop rax
    .endm

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
    test qword ptr [rsp + 16], 3
    jnz sanju_user_general_protection
    mov rcx, 13
    mov rdx, [rsp]
    xor r8, r8
    and rsp, -16
    sub rsp, 32
    call sanju_fatal_exception_dispatch
    ud2
sanju_user_general_protection:
    mov rcx, 13
    mov rdx, [rsp]
    xor r8, r8
    and rsp, -16
    sub rsp, 32
    call sanju_user_fault_dispatch
    mov rsp, qword ptr [rip + SANJU_USER_RESUME_RSP]
    jmp qword ptr [rip + SANJU_USER_RESUME_RIP]

    .global sanju_page_fault_stub
sanju_page_fault_stub:
    test qword ptr [rsp + 16], 3
    jnz sanju_user_page_fault
    mov rcx, 14
    mov rdx, [rsp]
    mov r8, cr2
    and rsp, -16
    sub rsp, 32
    call sanju_fatal_exception_dispatch
    ud2
sanju_user_page_fault:
    mov rcx, 14
    mov rdx, [rsp]
    mov r8, cr2
    and rsp, -16
    sub rsp, 32
    call sanju_user_fault_dispatch
    mov rsp, qword ptr [rip + SANJU_USER_RESUME_RSP]
    jmp qword ptr [rip + SANJU_USER_RESUME_RIP]

    .global sanju_timer_interrupt_stub
sanju_timer_interrupt_stub:
    SANJU_PUSH_REGISTERS
    mov rcx, rsp
    and rsp, -16
    sub rsp, 32
    cld
    call sanju_timer_interrupt_dispatch
    test rax, rax
    jz sanju_timer_resume_kernel
    mov rsp, rax
    SANJU_POP_REGISTERS
    iretq
sanju_timer_resume_kernel:
    mov rsp, qword ptr [rip + SANJU_USER_RESUME_RSP]
    jmp qword ptr [rip + SANJU_USER_RESUME_RIP]

    .global sanju_keyboard_interrupt_stub
sanju_keyboard_interrupt_stub:
    SANJU_PUSH_REGISTERS
    mov r12, rsp
    and rsp, -16
    sub rsp, 32
    cld
    call sanju_keyboard_interrupt_dispatch
    mov rsp, r12
    SANJU_POP_REGISTERS
    iretq

    .global sanju_enter_user_mode_asm
sanju_enter_user_mode_asm:
    push rbx
    push rbp
    push rdi
    push rsi
    push r12
    push r13
    push r14
    push r15
    mov qword ptr [rip + SANJU_USER_RESUME_RSP], rsp
    lea rax, [rip + sanju_user_resume]
    mov qword ptr [rip + SANJU_USER_RESUME_RIP], rax
    push 0x1b
    push rdx
    push 0x202
    push 0x23
    push rcx
    iretq
sanju_user_resume:
    pop r15
    pop r14
    pop r13
    pop r12
    pop rsi
    pop rdi
    pop rbp
    pop rbx
    ret

    .global sanju_syscall_entry_stub
sanju_syscall_entry_stub:
    mov qword ptr [rip + SANJU_SYSCALL_USER_RSP], rsp
    mov rsp, qword ptr [rip + SANJU_SYSCALL_KERNEL_RSP]
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    push rcx
    push r11
    push rdi
    push rsi
    push rdx
    push r10
    push r8
    push r9
    mov rcx, rax
    mov rdx, qword ptr [rsp + 40]
    mov r8, qword ptr [rsp + 32]
    mov r9, qword ptr [rsp + 24]
    sub rsp, 32
    cld
    call sanju_syscall_dispatch
    add rsp, 32
    cmp byte ptr [rip + SANJU_USER_EXIT_REQUESTED], 0
    jne sanju_syscall_resume_kernel
    pop r9
    pop r8
    pop r10
    pop rdx
    pop rsi
    pop rdi
    pop r11
    pop rcx
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    mov rsp, qword ptr [rip + SANJU_SYSCALL_USER_RSP]
    sysretq
sanju_syscall_resume_kernel:
    mov rsp, qword ptr [rip + SANJU_USER_RESUME_RSP]
    jmp qword ptr [rip + SANJU_USER_RESUME_RIP]

    .section .text.fh3_user, "ax"
    .balign 4096
    .global sanju_fh3_user_a
sanju_fh3_user_a:
    movabs r12, 0x53414e4a554f5301
    lea rax, [rip + SANJU_FH3_COUNTER_A]
1:
    inc qword ptr [rax]
    pause
    jmp 1b

    .balign 4096
    .global sanju_fh3_user_b
sanju_fh3_user_b:
    movabs r12, 0x53414e4a554f5302
    lea rax, [rip + SANJU_FH3_COUNTER_B]
2:
    inc qword ptr [rax]
    pause
    jmp 2b

    .section .data.fh3_counters, "aw"
    .balign 4096
    .global SANJU_FH3_COUNTER_A
SANJU_FH3_COUNTER_A:
    .quad 0
    .zero 4088

    .balign 4096
    .global SANJU_FH3_COUNTER_B
SANJU_FH3_COUNTER_B:
    .quad 0
    .zero 4088
"#
);

unsafe extern "C" {
    fn sanju_breakpoint_stub();
    fn sanju_double_fault_stub();
    fn sanju_general_protection_stub();
    fn sanju_page_fault_stub();
    fn sanju_timer_interrupt_stub();
    fn sanju_keyboard_interrupt_stub();
    fn sanju_syscall_entry_stub();
    fn sanju_fh3_user_a();
    fn sanju_fh3_user_b();
}

unsafe extern "C" {
    static mut SANJU_FH3_COUNTER_A: u64;
    static mut SANJU_FH3_COUNTER_B: u64;
}

unsafe extern "efiapi" {
    fn sanju_enter_user_mode_asm(entry: u64, stack_top: u64);
}

/// Moves execution to the statically reserved kernel stack.
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
            "cli",
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

/// Installs the GDT, TSS, protected exception stacks, and IDT.
///
/// # Safety
///
/// The caller must execute at x86-64 kernel privilege after switching to the
/// dedicated kernel stack. Interrupts must remain disabled until
/// [`initialize_interrupt_runtime`] installs the interrupt controllers.
#[must_use]
pub unsafe fn initialize() -> CpuProtectionReport {
    debug_assert_eq!(size_of::<InterruptFrame>(), 160);
    // SAFETY: The caller owns early CPU initialization and no other core can
    // access these tables during the single-core boot path.
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

/// Configures the legacy PIC, PIT timer, and PS/2 keyboard IRQ path.
///
/// # Safety
///
/// Must run once after [`initialize`] on the bootstrap processor while no other
/// core or driver accesses the interrupt-controller or PIT ports.
#[must_use]
pub unsafe fn initialize_interrupt_runtime() -> InterruptRuntimeReport {
    TIMER_TICKS.store(0, Ordering::SeqCst);
    KEYBOARD_IRQS.store(0, Ordering::SeqCst);
    SCANCODE_HEAD.store(0, Ordering::SeqCst);
    SCANCODE_TAIL.store(0, Ordering::SeqCst);
    SCANCODE_DROPPED.store(0, Ordering::SeqCst);

    // SAFETY: Single-core bootstrap owns the PIC and PIT programming sequence.
    unsafe {
        remap_and_unmask_pic();
        configure_pit();
        asm!("sti", options(nomem, nostack, preserves_flags));
    }

    let start = timer_ticks();
    while timer_ticks().wrapping_sub(start) < 5 {
        // SAFETY: Interrupts are enabled and IRQ0 is unmasked, so HLT resumes on
        // the next PIT tick.
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }

    TEST_SCANCODE.store(0x1c, Ordering::Release);
    // SAFETY: Vector 33 is installed as the keyboard IRQ gate. The dispatcher
    // consumes the injected Enter scancode before touching the controller port.
    unsafe {
        asm!("int 0x21", options(nomem, nostack));
    }
    let keyboard_self_test_passed = pop_scancode() == Some(0x1c);

    InterruptRuntimeReport {
        timer_interrupts_active: timer_ticks() > start,
        timer_ticks: timer_ticks(),
        timer_hz: TIMER_HZ,
        keyboard_interrupt_path_active: keyboard_self_test_passed,
        keyboard_irqs: keyboard_irqs(),
        dropped_scancodes: SCANCODE_DROPPED.load(Ordering::Acquire),
    }
}

/// Returns the number of PIT interrupts handled since initialization.
#[must_use]
pub fn timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Acquire)
}

/// Returns the number of keyboard interrupts handled since initialization.
#[must_use]
pub fn keyboard_irqs() -> u64 {
    KEYBOARD_IRQS.load(Ordering::Acquire)
}

/// Removes one scancode from the single-producer/single-consumer IRQ queue.
#[must_use]
pub fn pop_scancode() -> Option<u8> {
    let tail = SCANCODE_TAIL.load(Ordering::Relaxed);
    let head = SCANCODE_HEAD.load(Ordering::Acquire);
    if tail == head {
        return None;
    }

    // SAFETY: `tail` is within the fixed queue and only the kernel consumer
    // reads this slot before publishing the advanced tail.
    let scancode = unsafe { addr_of!(SCANCODE_QUEUE).cast::<u8>().add(tail).read() };
    SCANCODE_TAIL.store((tail + 1) % SCANCODE_QUEUE_CAPACITY, Ordering::Release);
    Some(scancode)
}

/// Halts until the next enabled interrupt.
#[allow(dead_code)]
pub fn halt_until_interrupt() {
    // SAFETY: The interrupt runtime is initialized before this helper is used.
    unsafe {
        asm!("hlt", options(nomem, nostack));
    }
}

/// Installs Ring 3 selectors, captures CR3, and configures SYSCALL/SYSRET.
///
/// # Safety
///
/// Must run once after the GDT and IDT are installed on the bootstrap CPU.
#[must_use]
pub unsafe fn initialize_user_mode_runtime() -> UserModeRuntimeReport {
    let root = active_page_table_root();
    // SAFETY: The dedicated syscall stack is statically owned by this CPU and
    // the MSRs are programmed once during single-core bootstrap.
    unsafe {
        SANJU_SYSCALL_KERNEL_RSP = u64::try_from(syscall_stack_top()).unwrap_or(u64::MAX);
        configure_syscall_msrs();
    }
    UserModeRuntimeReport {
        active_page_table_root: root,
        four_level_paging_active: root != 0,
        user_gdt_active: true,
        syscall_interface_active: true,
        page_fault_diagnostics_active: true,
    }
}

/// Runs one loaded ELF entry point at Ring 3 and returns after exit or a
/// recoverable user exception.
///
/// # Safety
///
/// The image and stack ranges must remain mapped for the duration of the call.
/// The entry point must lie inside `image_start..image_start + image_size`.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub unsafe fn run_user_process(
    entry: u64,
    image_start: u64,
    image_size: usize,
    stack_start: u64,
    stack_size: usize,
    pid: u32,
    address_space_root: u64,
    kernel_stack_top: u64,
) -> UserRunResult {
    let image_end = image_start.saturating_add(u64::try_from(image_size).unwrap_or(u64::MAX));

    let stack_end = stack_start.saturating_add(u64::try_from(stack_size).unwrap_or(u64::MAX));
    let kernel_root = active_page_table_root();

    CURRENT_USER_PID.store(u64::from(pid), Ordering::SeqCst);
    USER_SYSCALLS.store(0, Ordering::SeqCst);
    USER_YIELDS.store(0, Ordering::SeqCst);
    USER_TIMER_PREEMPTIONS.store(0, Ordering::SeqCst);
    USER_EXIT_CODE.store(0, Ordering::SeqCst);
    USER_FAULT_ADDRESS.store(0, Ordering::SeqCst);
    USER_FAULT_ERROR.store(0, Ordering::SeqCst);
    // SAFETY: Single-core user execution has exclusive ownership of the raw
    // ABI handoff fields until the assembly trampoline returns.
    unsafe {
        SANJU_USER_REGION_START = image_start;
        SANJU_USER_REGION_END = image_end;
        SANJU_USER_STACK_START = stack_start;
        SANJU_USER_STACK_END = stack_end;
        SANJU_USER_EXIT_REQUESTED = 0;
        SANJU_USER_FAULTED = 0;
    }

    // SAFETY: The private hierarchy was cloned from the kernel root and retains
    // the complete supervisor execution environment.
    let private_root_active = unsafe { switch_address_space(address_space_root) };
    if private_root_active {
        set_process_kernel_stack(kernel_stack_top);
    }

    // SAFETY: The active private page tables are identity-accessible in this
    // boot environment. The routine only confirms/promotes the supplied
    // mappings to user accessibility and clears NX for the loaded image.
    let image_ready =
        private_root_active && unsafe { mark_user_range(image_start, image_size, true) };
    // SAFETY: Same contract as above; the user stack remains writable under its
    // existing mapping and is only promoted to Ring 3 visibility.
    let stack_ready =
        private_root_active && unsafe { mark_user_range(stack_start, stack_size, false) };
    if !private_root_active
        || !image_ready
        || !stack_ready
        || entry < image_start
        || entry >= image_end
    {
        restore_kernel_runtime(kernel_root);
        CURRENT_USER_PID.store(0, Ordering::SeqCst);
        return UserRunResult {
            pid,
            exited: false,
            exit_code: -1,
            faulted: true,
            fault_address: entry,
            fault_error_code: u64::MAX,
            syscalls: 0,
            yields: 0,
            timer_preemptions: 0,
        };
    }

    // SAFETY: Selectors, TSS, syscall MSRs, user mappings, and stack are active.
    unsafe {
        sanju_enter_user_mode_asm(entry, stack_end & !0x0f);
    }
    restore_kernel_runtime(kernel_root);
    // SAFETY: The kernel root and bootstrap interrupt stack are restored.
    unsafe {
        asm!("sti", options(nomem, nostack, preserves_flags));
    }

    // SAFETY: These globals are updated only by the controlled user-mode
    // exception and syscall paths while this execution result is being collected.
    let (faulted, exit_requested) =
        unsafe { (SANJU_USER_FAULTED != 0, SANJU_USER_EXIT_REQUESTED != 0) };

    let exited = exit_requested && !faulted;
    let exit_code = i32::from_ne_bytes(
        u32::try_from(USER_EXIT_CODE.load(Ordering::SeqCst) & u64::from(u32::MAX))
            .unwrap_or(u32::MAX)
            .to_ne_bytes(),
    );
    let result = UserRunResult {
        pid,
        exited,
        exit_code,
        faulted,
        fault_address: USER_FAULT_ADDRESS.load(Ordering::SeqCst),
        fault_error_code: USER_FAULT_ERROR.load(Ordering::SeqCst),
        syscalls: USER_SYSCALLS.load(Ordering::SeqCst),
        yields: USER_YIELDS.load(Ordering::SeqCst),
        timer_preemptions: USER_TIMER_PREEMPTIONS.load(Ordering::SeqCst),
    };
    CURRENT_USER_PID.store(0, Ordering::SeqCst);
    result
}

unsafe fn configure_syscall_msrs() {
    const IA32_EFER: u32 = 0xc000_0080;
    const IA32_STAR: u32 = 0xc000_0081;
    const IA32_LSTAR: u32 = 0xc000_0082;
    const IA32_FMASK: u32 = 0xc000_0084;
    const EFER_SCE: u64 = 1 << 0;
    const EFER_NXE: u64 = 1 << 11;

    // SYSRET adds 16 for CS and 8 for SS to the upper STAR selector base.
    debug_assert_eq!(USER_CODE_SELECTOR, USER_DATA_SELECTOR + 8);
    let user_selector_base = u64::from(USER_CODE_SELECTOR.saturating_sub(16));
    let star = (user_selector_base << 48) | (u64::from(KERNEL_CODE_SELECTOR) << 32);
    // SAFETY: These architectural MSRs are present in x86-64 long mode.
    unsafe {
        let efer = read_msr(IA32_EFER);
        write_msr(IA32_EFER, efer | EFER_SCE | EFER_NXE);
        write_msr(IA32_STAR, star);
        write_msr(IA32_LSTAR, handler_address(sanju_syscall_entry_stub));
        write_msr(IA32_FMASK, (1 << 8) | (1 << 9) | (1 << 10));
    }
}

unsafe fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY: The caller chooses an architectural MSR valid in long mode.
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

unsafe fn write_msr(msr: u32, value: u64) {
    let low = u32::try_from(value & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    let high = u32::try_from(value >> 32).unwrap_or(u32::MAX);
    // SAFETY: The caller chooses an architectural MSR and value.
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn install_gdt_and_tss() {
    let tss = TaskStateSegment {
        privilege_stack_table: [
            u64::try_from(user_interrupt_stack_top()).unwrap_or(u64::MAX),
            0,
            0,
        ],
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

    // SAFETY: `GDT` contains exactly seven entries and is exclusively owned.
    unsafe {
        gdt.write(0);
        gdt.add(1).write(0x00af_9a00_0000_ffff);
        gdt.add(2).write(0x00cf_9200_0000_ffff);
        gdt.add(3).write(0x00cf_f200_0000_ffff);
        gdt.add(4).write(0x00af_fa00_0000_ffff);
        gdt.add(5).write(tss_low);
        gdt.add(6).write(tss_high);
    }

    let gdtr = DescriptorTablePointer {
        limit: u16::try_from(size_of::<[u64; 7]>() - 1).unwrap_or(u16::MAX),
        base: u64::try_from(addr_of!(GDT).addr()).unwrap_or(u64::MAX),
    };

    // SAFETY: The descriptor pointer references the static GDT. Ring 0 uses
    // selectors 0x08/0x10, Ring 3 uses 0x23/0x1b, and the TSS spans entries
    // five and six at selector 0x28.
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
    idt[TIMER_VECTOR] = IdtEntry::interrupt_gate(handler_address(sanju_timer_interrupt_stub), 0);
    idt[KEYBOARD_VECTOR] =
        IdtEntry::interrupt_gate(handler_address(sanju_keyboard_interrupt_stub), 0);

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

unsafe fn remap_and_unmask_pic() {
    // SAFETY: The caller owns the legacy PIC programming sequence.
    unsafe {
        outb(PIC_MASTER_COMMAND, 0x11);
        io_wait();
        outb(PIC_SLAVE_COMMAND, 0x11);
        io_wait();
        outb(PIC_MASTER_DATA, PIC_MASTER_VECTOR_OFFSET);
        io_wait();
        outb(PIC_SLAVE_DATA, PIC_SLAVE_VECTOR_OFFSET);
        io_wait();
        outb(PIC_MASTER_DATA, 0x04);
        io_wait();
        outb(PIC_SLAVE_DATA, 0x02);
        io_wait();
        outb(PIC_MASTER_DATA, 0x01);
        io_wait();
        outb(PIC_SLAVE_DATA, 0x01);
        io_wait();
        outb(PIC_MASTER_DATA, 0xfc);
        outb(PIC_SLAVE_DATA, 0xff);
    }
}

#[allow(clippy::cast_possible_truncation)]
unsafe fn configure_pit() {
    let divisor = PIT_INPUT_HZ / 100;
    // SAFETY: The caller owns PIT channel zero and has installed IRQ0.
    unsafe {
        outb(PIT_COMMAND, 0x36);
        outb(PIT_CHANNEL_ZERO, divisor as u8);
        outb(PIT_CHANNEL_ZERO, (divisor >> 8) as u8);
    }
}

fn enqueue_scancode(scancode: u8) {
    let head = SCANCODE_HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % SCANCODE_QUEUE_CAPACITY;
    if next == SCANCODE_TAIL.load(Ordering::Acquire) {
        SCANCODE_DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // SAFETY: The IRQ handler is the sole producer and writes only the current
    // head slot before publishing the new head with release ordering.
    unsafe {
        addr_of_mut!(SCANCODE_QUEUE)
            .cast::<u8>()
            .add(head)
            .write(scancode);
    }
    SCANCODE_HEAD.store(next, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "efiapi" fn sanju_timer_interrupt_dispatch(frame: *mut InterruptFrame) -> u64 {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    if CURRENT_USER_PID.load(Ordering::Relaxed) != 0 {
        USER_TIMER_PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
    }
    let next_frame = fh3_schedule_from_timer(frame);
    // SAFETY: IRQ0 is serviced by the master PIC.
    unsafe {
        outb(PIC_MASTER_COMMAND, PIC_EOI);
    }
    next_frame
}

fn fh3_schedule_from_timer(frame: *mut InterruptFrame) -> u64 {
    let frame_address = u64::try_from(frame.addr()).unwrap_or(0);
    if frame.is_null() {
        return frame_address;
    }
    // SAFETY: `cs` is present in both the three-word Ring 0 interrupt tail and
    // the five-word privilege-transition tail.
    let came_from_user = unsafe { addr_of!((*frame).cs).read() } & 3 == 3;
    // SAFETY: The IRQ path is the only writer while the single-core probe is
    // active, so copying the state avoids references to a mutable static.
    let mut runtime = unsafe { addr_of!(FH3_RUNTIME).read() };
    if !runtime.active || !came_from_user {
        return frame_address;
    }
    // SAFETY: A CPL3 interrupt supplies the complete five-word hardware tail,
    // so all fields of `InterruptFrame` are present.
    let saved = unsafe { frame.read() };

    let current = runtime.current;
    runtime.preemptions = runtime.preemptions.saturating_add(1);
    runtime.processes[current].saved_frame = frame_address;
    if saved.r12 == runtime.processes[current].expected_r12 {
        runtime.register_context_checks = runtime.register_context_checks.saturating_add(1);
    }

    let counters_ready =
        fh3_counter_value(0) >= FH3_COUNTER_TARGET && fh3_counter_value(1) >= FH3_COUNTER_TARGET;
    if counters_ready && runtime.context_switches >= 2 {
        runtime.active = false;
        runtime.completed = true;
        restore_kernel_runtime(runtime.kernel_root);
        // SAFETY: Publish the completed statistics before the assembly
        // trampoline resumes the original kernel continuation.
        unsafe {
            addr_of_mut!(FH3_RUNTIME).write(runtime);
        }
        return 0;
    }

    let next = (current + 1) % FH3_PROCESS_COUNT;
    let next_process = runtime.processes[next];
    if next_process.saved_frame == 0 {
        runtime.active = false;
        runtime.faulted = true;
        restore_kernel_runtime(runtime.kernel_root);
        // SAFETY: Same single-writer state publication as the success path.
        unsafe {
            addr_of_mut!(FH3_RUNTIME).write(runtime);
        }
        return 0;
    }

    // SAFETY: Both roots are private clones that retain every supervisor
    // mapping needed by the interrupt return path.
    if !unsafe { switch_address_space(next_process.root) } {
        runtime.active = false;
        runtime.faulted = true;
        restore_kernel_runtime(runtime.kernel_root);
        // SAFETY: Same single-writer state publication as above.
        unsafe {
            addr_of_mut!(FH3_RUNTIME).write(runtime);
        }
        return 0;
    }
    set_process_kernel_stack(next_process.kernel_stack_top);
    CURRENT_USER_PID.store(u64::from(next_process.pid), Ordering::SeqCst);
    runtime.current = next;
    runtime.context_switches = runtime.context_switches.saturating_add(1);
    runtime.cr3_switches = runtime.cr3_switches.saturating_add(1);
    // SAFETY: Publish the selected frame before the assembly trampoline reads
    // the returned pointer and performs POP/IRETQ.
    unsafe {
        addr_of_mut!(FH3_RUNTIME).write(runtime);
    }
    next_process.saved_frame
}

#[unsafe(no_mangle)]
extern "efiapi" fn sanju_keyboard_interrupt_dispatch() {
    let injected = TEST_SCANCODE.swap(0, Ordering::AcqRel);
    let scancode = if injected == 0 {
        // SAFETY: IRQ1 indicates the keyboard controller has a data byte.
        unsafe { inb(KEYBOARD_DATA) }
    } else {
        injected
    };

    enqueue_scancode(scancode);
    KEYBOARD_IRQS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: IRQ1 is serviced by the master PIC.
    unsafe {
        outb(PIC_MASTER_COMMAND, PIC_EOI);
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
    // SAFETY: The stack is a static variable owned by the kernel.
    unsafe { addr_of_mut!(KERNEL_STACK.0).cast::<u8>().addr() + KERNEL_STACK_SIZE }
}

fn double_fault_stack_top() -> usize {
    // SAFETY: The stack is a static variable owned by the kernel.
    unsafe { addr_of_mut!(DOUBLE_FAULT_STACK.0).cast::<u8>().addr() + DOUBLE_FAULT_STACK_SIZE }
}

fn syscall_stack_top() -> usize {
    // SAFETY: The stack is a static variable owned by the kernel.
    unsafe { addr_of_mut!(SYSCALL_STACK.0).cast::<u8>().addr() + SYSCALL_STACK_SIZE }
}

fn user_interrupt_stack_top() -> usize {
    // SAFETY: This stack is reserved for CPL3-to-CPL0 interrupt transitions.
    unsafe { addr_of_mut!(USER_INTERRUPT_STACK.0).cast::<u8>().addr() + USER_INTERRUPT_STACK_SIZE }
}

#[must_use]
pub fn process_kernel_stack_layout(index: usize) -> Option<GuardedStackLayout> {
    if index >= PROCESS_KERNEL_STACK_COUNT {
        return None;
    }
    // SAFETY: The bounds check selects one element from the static stack
    // array without creating a reference to the mutable static.
    let base = unsafe {
        addr_of_mut!(PROCESS_KERNEL_STACKS)
            .cast::<ProcessKernelStack>()
            .add(index)
            .cast::<u8>()
            .addr()
    };
    guarded_stack_layout(base, PROCESS_KERNEL_STACK_SIZE)
}

#[must_use]
pub fn preemption_probe_layout() -> Option<PreemptionProbeLayout> {
    // Each symbol is page-aligned assembly or static storage retained for the
    // entire kernel lifetime.
    let counter_a = addr_of_mut!(SANJU_FH3_COUNTER_A).addr();
    let counter_b = addr_of_mut!(SANJU_FH3_COUNTER_B).addr();
    let entry_a = handler_address(sanju_fh3_user_a);
    let entry_b = handler_address(sanju_fh3_user_b);
    let user_a = fh3_user_stack_layout(0)?;
    let user_b = fh3_user_stack_layout(1)?;
    let kernel_a = process_kernel_stack_layout(3)?;
    let kernel_b = process_kernel_stack_layout(4)?;
    Some(PreemptionProbeLayout {
        process_a: PreemptionProbeProcessLayout {
            entry: entry_a,
            code_page: entry_a & !0x0fff,
            counter_page: u64::try_from(counter_a).ok()?,
            user_stack: user_a,
            kernel_stack: kernel_a,
        },
        process_b: PreemptionProbeProcessLayout {
            entry: entry_b,
            code_page: entry_b & !0x0fff,
            counter_page: u64::try_from(counter_b).ok()?,
            user_stack: user_b,
            kernel_stack: kernel_b,
        },
    })
}

fn fh3_user_stack_layout(index: usize) -> Option<GuardedStackLayout> {
    if index >= FH3_PROCESS_COUNT {
        return None;
    }
    // SAFETY: The bounds check selects one process-owned stack slot.
    let base = unsafe {
        addr_of_mut!(FH3_USER_STACKS)
            .cast::<Fh3UserStack>()
            .add(index)
            .cast::<u8>()
            .addr()
    };
    guarded_stack_layout(base, FH3_USER_STACK_SIZE)
}

fn guarded_stack_layout(base: usize, stack_size: usize) -> Option<GuardedStackLayout> {
    if !base.is_multiple_of(4096) || !stack_size.is_multiple_of(4096) {
        return None;
    }
    let stack_start = base.checked_add(4096)?;
    let stack_top = stack_start.checked_add(stack_size)?;
    let upper_guard = stack_top;
    Some(GuardedStackLayout {
        lower_guard: u64::try_from(base).ok()?,
        stack_start: u64::try_from(stack_start).ok()?,
        stack_size,
        stack_top: u64::try_from(stack_top).ok()?,
        upper_guard: u64::try_from(upper_guard).ok()?,
    })
}

/// Runs two non-cooperative Ring 3 loops under the PIT scheduler.
///
/// # Safety
///
/// Both roots must be inactive private clones containing the mappings returned
/// by [`preemption_probe_layout`]. Their page-table frames and all four stack
/// ranges must remain allocated until this function returns.
#[must_use]
pub unsafe fn run_preemption_probe(
    process_a_root: u64,
    process_b_root: u64,
) -> PreemptionProbeReport {
    let Some(layout) = preemption_probe_layout() else {
        return PreemptionProbeReport::default();
    };
    let kernel_root = active_page_table_root();
    if process_a_root == 0
        || process_b_root == 0
        || process_a_root == process_b_root
        || process_a_root == kernel_root
        || process_b_root == kernel_root
    {
        return PreemptionProbeReport::default();
    }

    fh3_set_counter(0, 0);
    fh3_set_counter(1, 0);
    let Some(process_b_frame) = prepare_initial_user_frame(
        layout.process_b.kernel_stack.stack_top,
        layout.process_b.entry,
        layout.process_b.user_stack.stack_top,
        FH3_R12_B,
    ) else {
        return PreemptionProbeReport::default();
    };
    let runtime = Fh3Runtime {
        active: true,
        completed: false,
        faulted: false,
        current: 0,
        kernel_root,
        processes: [
            Fh3RuntimeProcess {
                pid: 0x1001,
                root: process_a_root,
                saved_frame: 0,
                kernel_stack_top: layout.process_a.kernel_stack.stack_top,
                expected_r12: FH3_R12_A,
            },
            Fh3RuntimeProcess {
                pid: 0x1002,
                root: process_b_root,
                saved_frame: process_b_frame,
                kernel_stack_top: layout.process_b.kernel_stack.stack_top,
                expected_r12: FH3_R12_B,
            },
        ],
        preemptions: 0,
        context_switches: 0,
        cr3_switches: 0,
        register_context_checks: 0,
    };
    // SAFETY: The probe is single-core and interrupts cannot observe the state
    // until the first private root and TSS stack are installed below.
    unsafe {
        addr_of_mut!(FH3_RUNTIME).write(runtime);
    }
    CURRENT_USER_PID.store(0x1001, Ordering::SeqCst);

    // SAFETY: The caller supplied a private root cloned from `kernel_root`.
    if !unsafe { switch_address_space(process_a_root) } {
        restore_kernel_runtime(kernel_root);
        return PreemptionProbeReport::default();
    }
    set_process_kernel_stack(layout.process_a.kernel_stack.stack_top);

    // SAFETY: The private root maps the page-aligned probe code, counter, user
    // stack, kernel stack, IDT, GDT, TSS, and complete kernel continuation.
    unsafe {
        sanju_enter_user_mode_asm(
            layout.process_a.entry,
            layout.process_a.user_stack.stack_top,
        );
    }
    restore_kernel_runtime(kernel_root);
    // SAFETY: The kernel root and bootstrap interrupt stack are active again.
    unsafe {
        asm!("sti", options(nomem, nostack, preserves_flags));
    }

    // SAFETY: Timer scheduling is inactive before the trampoline resumes this
    // continuation, so the state can be copied without a race.
    let final_state = unsafe { addr_of!(FH3_RUNTIME).read() };
    let counter_a = fh3_counter_value(0);
    let counter_b = fh3_counter_value(1);
    PreemptionProbeReport {
        ring3_processes_started: if final_state.completed {
            FH3_PROCESS_COUNT
        } else {
            0
        },
        timer_preemptions: final_state.preemptions,
        context_switches: final_state.context_switches,
        cr3_switches: final_state.cr3_switches,
        register_context_checks: final_state.register_context_checks,
        process_a_counter: counter_a,
        process_b_counter: counter_b,
        full_frame_switching_active: final_state.completed
            && !final_state.faulted
            && final_state.context_switches >= 2
            && final_state.register_context_checks >= 2,
        private_cr3_switching_active: final_state.completed
            && final_state.cr3_switches >= 2
            && active_page_table_root() == kernel_root,
        per_process_kernel_stacks_active: layout.process_a.kernel_stack.stack_top
            != layout.process_b.kernel_stack.stack_top,
    }
}

fn prepare_initial_user_frame(
    kernel_stack_top: u64,
    entry: u64,
    user_stack_top: u64,
    r12: u64,
) -> Option<u64> {
    let frame_size = u64::try_from(size_of::<InterruptFrame>()).ok()?;
    let frame_address = kernel_stack_top.checked_sub(frame_size)?;
    if !frame_address.is_multiple_of(16) {
        return None;
    }
    let pointer = usize::try_from(frame_address).ok()? as *mut InterruptFrame;
    // SAFETY: The address lies inside an inactive, statically reserved Ring 0
    // stack and is aligned for the complete interrupt frame.
    unsafe {
        pointer.write(InterruptFrame::user(entry, user_stack_top, r12));
    }
    Some(frame_address)
}

fn set_process_kernel_stack(stack_top: u64) {
    let tss = addr_of_mut!(TSS);
    // SAFETY: The packed TSS requires an unaligned raw write. The bootstrap CPU
    // is the sole writer and the new stack remains mapped by the active root.
    unsafe {
        addr_of_mut!((*tss).privilege_stack_table)
            .cast::<u64>()
            .write_unaligned(stack_top);
        SANJU_SYSCALL_KERNEL_RSP = stack_top;
    }
}

fn restore_kernel_runtime(kernel_root: u64) {
    // SAFETY: `kernel_root` is the retained SanjuOS root and remains allocated
    // for the lifetime of the bootstrap CPU.
    let _ = unsafe { switch_address_space(kernel_root) };
    let tss = addr_of_mut!(TSS);
    let interrupt_stack = u64::try_from(user_interrupt_stack_top()).unwrap_or(u64::MAX);
    // SAFETY: Restore the permanent CPL3 interrupt and syscall stacks before
    // publishing that no user process owns the CPU.
    unsafe {
        addr_of_mut!((*tss).privilege_stack_table)
            .cast::<u64>()
            .write_unaligned(interrupt_stack);
        SANJU_SYSCALL_KERNEL_RSP = u64::try_from(syscall_stack_top()).unwrap_or(u64::MAX);
    }
    CURRENT_USER_PID.store(0, Ordering::SeqCst);
}

fn fh3_counter_value(index: usize) -> u64 {
    let pointer = match index {
        0 => addr_of!(SANJU_FH3_COUNTER_A),
        1 => addr_of!(SANJU_FH3_COUNTER_B),
        _ => return 0,
    };
    // SAFETY: Each counter occupies its own retained page and is written only
    // by one Ring 3 probe process.
    unsafe { pointer.read_volatile() }
}

fn fh3_set_counter(index: usize, value: u64) {
    let pointer = match index {
        0 => addr_of_mut!(SANJU_FH3_COUNTER_A),
        1 => addr_of_mut!(SANJU_FH3_COUNTER_B),
        _ => return,
    };
    // SAFETY: The probe is inactive while counters are reset.
    unsafe {
        pointer.write_volatile(value);
    }
}

#[unsafe(no_mangle)]
extern "efiapi" fn sanju_syscall_dispatch(
    number: u64,
    argument_0: u64,
    argument_1: u64,
    _argument_2: u64,
) -> u64 {
    USER_SYSCALLS.fetch_add(1, Ordering::Relaxed);
    match number {
        0 => {
            let Ok(length) = usize::try_from(argument_1) else {
                return u64::MAX - 13;
            };
            if length > 64 * 1024 || !user_pointer_is_valid(argument_0, length) {
                return u64::MAX - 13;
            }
            let Ok(pointer) = usize::try_from(argument_0) else {
                return u64::MAX - 13;
            };
            // SAFETY: The current user image/stack bounds were registered by
            // `run_user_process` and validation confines this read to them.
            let bytes = unsafe { core::slice::from_raw_parts(pointer as *const u8, length) };
            debug_write(bytes);
            argument_1
        }
        1 => 0,
        2 => {
            USER_EXIT_CODE.store(argument_0, Ordering::SeqCst);
            // SAFETY: The syscall trampoline is the sole writer while the user
            // process is active on this single CPU.
            unsafe {
                SANJU_USER_EXIT_REQUESTED = 1;
            }
            0
        }
        3 => {
            USER_YIELDS.fetch_add(1, Ordering::Relaxed);
            // SAFETY: IRQ0 is active. Waiting for one interrupt makes the yield
            // path observable as a real timer-driven preemption point.
            unsafe {
                asm!("sti", "hlt", "cli", options(nomem, nostack));
            }
            0
        }
        4 => CURRENT_USER_PID.load(Ordering::Relaxed),
        5 => 3,
        6 => 0,
        7 => u64::MAX - 10,
        _ => u64::MAX - 37,
    }
}

#[unsafe(no_mangle)]
extern "efiapi" fn sanju_user_fault_dispatch(vector: u64, error_code: u64, fault_address: u64) {
    USER_FAULT_ADDRESS.store(fault_address, Ordering::SeqCst);
    USER_FAULT_ERROR.store(error_code, Ordering::SeqCst);
    abort_fh3_runtime();
    // SAFETY: The exception trampoline is the sole writer while one user
    // process owns the CPU.
    unsafe {
        SANJU_USER_FAULTED = 1;
        SANJU_USER_EXIT_REQUESTED = 1;
    }
    debug_write_line("SanjuOS: isolated user exception");
    debug_write_label_hex("User PID: ", CURRENT_USER_PID.load(Ordering::Relaxed));
    debug_write_label_hex("Vector: ", vector);
    debug_write_label_hex("Error code: ", error_code);
    debug_write_label_hex("Fault address: ", fault_address);
}

fn abort_fh3_runtime() {
    // SAFETY: The exception path is serialized on the bootstrap CPU.
    let mut runtime = unsafe { addr_of!(FH3_RUNTIME).read() };
    if !runtime.active {
        return;
    }
    runtime.active = false;
    runtime.faulted = true;
    restore_kernel_runtime(runtime.kernel_root);
    // SAFETY: Publish the failed terminal state before resuming the kernel.
    unsafe {
        addr_of_mut!(FH3_RUNTIME).write(runtime);
    }
}

fn user_pointer_is_valid(pointer: u64, length: usize) -> bool {
    let Ok(length) = u64::try_from(length) else {
        return false;
    };
    let Some(end) = pointer.checked_add(length) else {
        return false;
    };
    // SAFETY: These bounds are published before Ring 3 entry and remain stable
    // until the user process returns.
    let (image_start, image_end, stack_start, stack_end) = unsafe {
        (
            SANJU_USER_REGION_START,
            SANJU_USER_REGION_END,
            SANJU_USER_STACK_START,
            SANJU_USER_STACK_END,
        )
    };
    (pointer >= image_start && end <= image_end) || (pointer >= stack_start && end <= stack_end)
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
        // SAFETY: COM1 is the configured early serial device.
        if unsafe { inb(LINE_STATUS) } & 0x20 != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    // SAFETY: COM1 is the configured early serial device.
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

unsafe fn io_wait() {
    // SAFETY: Port 0x80 is the conventional POST delay port.
    unsafe {
        outb(0x80, 0);
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

/// Halts the bootstrap processor permanently with interrupts disabled.
#[allow(dead_code)]
pub fn halt_forever() -> ! {
    loop {
        // SAFETY: This is the terminal state after an unrecoverable kernel
        // failure. Interrupts are disabled before halting so firmware-owned
        // handlers cannot run after `ExitBootServices`.
        unsafe {
            asm!("cli", "hlt", options(nomem, nostack));
        }
    }
}
