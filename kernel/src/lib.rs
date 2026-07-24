#![cfg_attr(not(test), no_std)]
#![allow(clippy::pedantic)]

pub mod boot_info;
pub mod capabilities;
pub mod elf;
pub mod generated;
pub mod ownership;
pub mod fs;
pub mod heap;
pub mod input;
pub mod memory;
pub mod paging;
pub mod process;
pub mod scheduler;
pub mod shell;
pub mod startup;
pub mod syscall;

/// Minimal output boundary used before the full device and logging stacks exist.
pub trait Console {
    /// Writes one byte. Early consoles currently accept printable ASCII plus
    /// carriage-return and line-feed bytes.
    fn write_byte(&mut self, byte: u8);

    /// Writes an ASCII string through [`Console::write_byte`].
    fn write_str(&mut self, text: &str) {
        for byte in text.bytes() {
            self.write_byte(byte);
        }
    }

    /// Writes an ASCII line using CRLF, accepted by UEFI and serial consoles.
    fn write_line(&mut self, text: &str) {
        self.write_str(text);
        self.write_str("\r\n");
    }

    /// Writes an unsigned pointer-sized integer without allocation.
    fn write_usize(&mut self, value: usize) {
        self.write_u64(u64::try_from(value).unwrap_or(u64::MAX));
    }

    /// Writes an unsigned 64-bit integer without allocation.
    fn write_u64(&mut self, mut value: u64) {
        if value == 0 {
            self.write_byte(b'0');
            return;
        }

        let mut digits = [0_u8; 20];
        let mut cursor = digits.len();
        while value != 0 {
            cursor -= 1;
            digits[cursor] = b'0' + u8::try_from(value % 10).unwrap_or(0);
            value /= 10;
        }

        for digit in &digits[cursor..] {
            self.write_byte(*digit);
        }
    }
}

pub use boot_info::{BootInfo, BootInfoV1, MemoryMapInfo};

/// Runtime evidence produced by the combined M3/M4 kernel batch.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct M4Report {
    pub kernel_stack_active: bool,
    pub gdt_active: bool,
    pub tss_active: bool,
    pub idt_active: bool,
    pub breakpoint_self_test_passed: bool,
    pub timer_interrupts_active: bool,
    pub timer_ticks: u64,
    pub timer_hz: u64,
    pub keyboard_interrupt_path_active: bool,
    pub keyboard_irqs: u64,
    pub keyboard_scancodes_dropped: u64,
    pub usable_frames: usize,
    pub allocated_frames: usize,
    pub heap_allocations: usize,
    pub heap_remaining_bytes: usize,
    pub scheduler_active: bool,
    pub scheduler_tasks: usize,
    pub scheduler_context_switches: u64,
    pub scheduler_dispatches: u64,
    pub shell_active: bool,
    pub shell_commands_executed: usize,
    pub ramfs_active: bool,
    pub ramfs_files: usize,
}

impl M4Report {
    #[must_use]
    pub const fn gate_passed(self) -> bool {
        self.kernel_stack_active
            && self.gdt_active
            && self.tss_active
            && self.idt_active
            && self.breakpoint_self_test_passed
            && self.timer_interrupts_active
            && self.timer_ticks > 0
            && self.timer_hz > 0
            && self.keyboard_interrupt_path_active
            && self.keyboard_irqs > 0
            && self.usable_frames > 0
            && self.allocated_frames > 0
            && self.heap_allocations > 0
            && self.scheduler_active
            && self.scheduler_tasks >= 3
            && self.scheduler_context_switches > 0
            && self.scheduler_dispatches > 0
            && self.shell_active
            && self.shell_commands_executed > 0
            && self.ramfs_active
            && self.ramfs_files >= 2
    }
}

/// Architecture-independent M4 status entry point after runtime initialization.
#[allow(clippy::too_many_lines)]
pub fn kernel_main(console: &mut dyn Console, boot_info: BootInfo, report: M4Report) {
    console.write_line("");
    console.write_line("SanjuOS");
    console.write_line(boot_info.milestone());
    console.write_str("Architecture: ");
    console.write_line(boot_info.architecture());
    console.write_str("Firmware: ");
    console.write_line(boot_info.firmware());
    console.write_line("Firmware boot services: exited");
    write_state(
        console,
        "Protected kernel stack",
        report.kernel_stack_active,
    );
    write_state(console, "GDT", report.gdt_active);
    write_state(console, "TSS", report.tss_active);
    write_state(console, "IDT exception handling", report.idt_active);
    write_state(
        console,
        "Breakpoint exception self-test",
        report.breakpoint_self_test_passed,
    );
    write_state(
        console,
        "PIT timer interrupts",
        report.timer_interrupts_active,
    );
    console.write_str("Timer ticks observed: ");
    console.write_u64(report.timer_ticks);
    console.write_str(" at ");
    console.write_u64(report.timer_hz);
    console.write_line(" Hz");
    write_state(
        console,
        "PS/2 keyboard interrupt path",
        report.keyboard_interrupt_path_active,
    );
    console.write_str("Keyboard IRQs observed: ");
    console.write_u64(report.keyboard_irqs);
    console.write_str(", dropped scancodes: ");
    console.write_u64(report.keyboard_scancodes_dropped);
    console.write_line("");
    console.write_str("Usable physical frames: ");
    console.write_usize(report.usable_frames);
    console.write_line("");
    console.write_str("Allocated bootstrap frames: ");
    console.write_usize(report.allocated_frames);
    console.write_line("");
    console.write_str("Bootstrap heap allocations: ");
    console.write_usize(report.heap_allocations);
    console.write_line("");
    console.write_str("Bootstrap heap remaining bytes: ");
    console.write_usize(report.heap_remaining_bytes);
    console.write_line("");
    write_state(console, "Round-robin scheduler", report.scheduler_active);
    console.write_str("Scheduler tasks: ");
    console.write_usize(report.scheduler_tasks);
    console.write_str(", context switches: ");
    console.write_u64(report.scheduler_context_switches);
    console.write_str(", dispatches: ");
    console.write_u64(report.scheduler_dispatches);
    console.write_line("");
    write_state(console, "Interactive kernel shell", report.shell_active);
    console.write_str("Shell commands executed: ");
    console.write_usize(report.shell_commands_executed);
    console.write_line("");
    write_state(console, "RAM filesystem", report.ramfs_active);
    console.write_str("RAM filesystem files: ");
    console.write_usize(report.ramfs_files);
    console.write_line("");

    if report.gate_passed() {
        console.write_line("M4 interactive runtime gate: passed");
        console
            .write_line("Next gate: paging ownership, user mode, syscalls, and executable loading");
    } else {
        console.write_line("M4 interactive runtime gate: failed");
    }
}

/// Runtime evidence produced by the M5 protected-userspace batch.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct M5Report {
    pub paging_ownership_active: bool,
    pub active_page_table_root: u64,
    pub four_level_paging_active: bool,
    pub mapping_api_active: bool,
    pub page_flags_active: bool,
    pub boot_memory_reclaim_active: bool,
    pub guard_pages_active: bool,
    pub write_xor_execute_active: bool,
    pub kernel_heap_active: bool,
    pub heap_allocations: usize,
    pub heap_frees: usize,
    pub page_fault_diagnostics_active: bool,
    pub user_gdt_active: bool,
    pub ring3_execution_active: bool,
    pub user_address_space_isolation_active: bool,
    pub user_stacks_active: bool,
    pub process_control_blocks_active: bool,
    pub context_switching_active: bool,
    pub preemptive_scheduling_active: bool,
    pub syscall_interface_active: bool,
    pub safe_user_memory_active: bool,
    pub elf64_loader_active: bool,
    pub user_programs_launched: usize,
    pub user_processes_exited: usize,
    pub user_fault_isolation_passed: bool,
    pub startup_experience_active: bool,
    pub sanjuos_brand_printed: bool,
}

impl M5Report {
    #[must_use]
    pub const fn gate_passed(self) -> bool {
        self.paging_ownership_active
            && self.active_page_table_root != 0
            && self.four_level_paging_active
            && self.mapping_api_active
            && self.page_flags_active
            && self.boot_memory_reclaim_active
            && self.guard_pages_active
            && self.write_xor_execute_active
            && self.kernel_heap_active
            && self.heap_allocations > 0
            && self.heap_frees > 0
            && self.page_fault_diagnostics_active
            && self.user_gdt_active
            && self.ring3_execution_active
            && self.user_address_space_isolation_active
            && self.user_stacks_active
            && self.process_control_blocks_active
            && self.context_switching_active
            && self.preemptive_scheduling_active
            && self.syscall_interface_active
            && self.safe_user_memory_active
            && self.elf64_loader_active
            && self.user_programs_launched >= 3
            && self.user_processes_exited >= 2
            && self.user_fault_isolation_passed
            && self.startup_experience_active
            && self.sanjuos_brand_printed
    }
}

/// Prints the M5 protected-userspace acceptance report.
#[allow(clippy::too_many_lines)]
pub fn kernel_main_m5(console: &mut dyn Console, boot_info: BootInfo, report: M5Report) {
    startup::print_logo(console);
    console.write_line("SanjuOS M5");
    console.write_line(boot_info.milestone());
    console.write_str("Architecture: ");
    console.write_line(boot_info.architecture());
    console.write_str("Firmware: ");
    console.write_line(boot_info.firmware());
    write_state(console, "Inherited page-table root captured", report.paging_ownership_active);
    console.write_str("Active page-table root: 0x");
    write_hex_u64(console, report.active_page_table_root);
    console.write_line("");
    write_state(
        console,
        "Page-table mapping acceptance model",
        report.four_level_paging_active,
    );
    write_state(console, "Page map/unmap API", report.mapping_api_active);
    write_state(console, "Page protection flags", report.page_flags_active);
    write_state(
        console,
        "Boot-service reclaim inventory",
        report.boot_memory_reclaim_active,
    );
    write_state(console, "Guarded-stack layout", report.guard_pages_active);
    write_state(
        console,
        "W^X memory security",
        report.write_xor_execute_active,
    );
    write_state(console, "Kernel heap", report.kernel_heap_active);
    console.write_str("Kernel heap allocations/frees: ");
    console.write_usize(report.heap_allocations);
    console.write_str("/");
    console.write_usize(report.heap_frees);
    console.write_line("");
    write_state(
        console,
        "Enhanced page-fault diagnostics",
        report.page_fault_diagnostics_active,
    );
    write_state(console, "User-mode GDT segments", report.user_gdt_active);
    write_state(console, "Ring 3 execution", report.ring3_execution_active);
    write_state(
        console,
        "User address-space model",
        report.user_address_space_isolation_active,
    );
    write_state(console, "Guarded user stacks", report.user_stacks_active);
    write_state(
        console,
        "Process control blocks",
        report.process_control_blocks_active,
    );
    write_state(
        console,
        "Saved CPU context model",
        report.context_switching_active,
    );
    write_state(
        console,
        "Timer-driven scheduling model",
        report.preemptive_scheduling_active,
    );
    write_state(
        console,
        "System-call interface",
        report.syscall_interface_active,
    );
    write_state(
        console,
        "Safe user-memory access",
        report.safe_user_memory_active,
    );
    write_state(console, "ELF64 loader", report.elf64_loader_active);
    console.write_str("User processes launched: ");
    console.write_usize(report.user_programs_launched);
    console.write_line("");
    console.write_str("User processes exited: ");
    console.write_usize(report.user_processes_exited);
    console.write_line("");
    if report.user_fault_isolation_passed {
        console.write_line("User fault isolation: passed");
    } else {
        console.write_line("User fault isolation: failed");
    }
    write_state(
        console,
        "Branded startup experience",
        report.startup_experience_active,
    );
    write_state(console, "SanjuOS logo print", report.sanjuos_brand_printed);

    if report.gate_passed() {
        console.write_line("M5 protected user-space gate: passed");
        console.write_line("M5 regression status: preserved for foundation hardening");
    } else {
        console.write_line("M5 protected user-space gate: failed");
    }
}

/// Runtime evidence for Foundation Hardening Phase 1.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FoundationHardeningReport {
    pub toolchain_pinned: bool,
    pub capability_registry_synchronized: bool,
    pub architecture_separation_verified: bool,
    pub boot_info_version: u32,
    pub ownership_map_active: bool,
    pub ownership_ranges: usize,
    pub overlap_detection_passed: bool,
    pub frame_allocation_unique: bool,
    pub frame_reuse_passed: bool,
    pub double_free_detection_passed: bool,
    pub reserved_frame_detection_passed: bool,
    pub bootstrap_pool_active: bool,
    pub bootstrap_pool_capacity: usize,
    pub bootstrap_pool_remaining: usize,
    pub m5_regression_passed: bool,
}

impl FoundationHardeningReport {
    #[must_use]
    pub const fn gate_passed(self) -> bool {
        self.toolchain_pinned
            && self.capability_registry_synchronized
            && self.architecture_separation_verified
            && self.boot_info_version == boot_info::BOOT_INFO_VERSION
            && self.ownership_map_active
            && self.ownership_ranges > 0
            && self.overlap_detection_passed
            && self.frame_allocation_unique
            && self.frame_reuse_passed
            && self.double_free_detection_passed
            && self.reserved_frame_detection_passed
            && self.bootstrap_pool_active
            && self.bootstrap_pool_capacity > 0
            && self.bootstrap_pool_remaining == self.bootstrap_pool_capacity
            && self.m5_regression_passed
    }
}

/// Prints the Foundation Hardening Phase 1 acceptance report.
pub fn kernel_main_foundation_hardening(
    console: &mut dyn Console,
    report: FoundationHardeningReport,
) {
    console.write_line("");
    console.write_line("SanjuOS Foundation Hardening");
    console.write_line(if report.toolchain_pinned {
        "Pinned toolchain: verified"
    } else {
        "Pinned toolchain: failed"
    });
    console.write_line(if report.capability_registry_synchronized {
        "Capability registry: synchronized"
    } else {
        "Capability registry: stale"
    });
    console.write_line(if report.architecture_separation_verified {
        "Architecture separation: verified"
    } else {
        "Architecture separation: failed"
    });
    console.write_str("BootInfo version: ");
    console.write_u64(u64::from(report.boot_info_version));
    console.write_line("");
    write_state(console, "Physical ownership map", report.ownership_map_active);
    console.write_str("Physical ownership ranges: ");
    console.write_usize(report.ownership_ranges);
    console.write_line("");
    console.write_line(if report.overlap_detection_passed {
        "Reserved-range overlap test: passed"
    } else {
        "Reserved-range overlap test: failed"
    });
    console.write_line(if report.frame_allocation_unique && report.frame_reuse_passed {
        "Frame allocation/free test: passed"
    } else {
        "Frame allocation/free test: failed"
    });
    console.write_line(if report.double_free_detection_passed {
        "Double-free detection: passed"
    } else {
        "Double-free detection: failed"
    });
    console.write_line(if report.reserved_frame_detection_passed {
        "Reserved-frame protection: passed"
    } else {
        "Reserved-frame protection: failed"
    });
    write_state(
        console,
        "Page-table bootstrap pool",
        report.bootstrap_pool_active,
    );
    console.write_str("Page-table bootstrap frames: ");
    console.write_usize(report.bootstrap_pool_capacity);
    console.write_line("");
    console.write_line(if report.m5_regression_passed {
        "M5 regression boot: passed"
    } else {
        "M5 regression boot: failed"
    });
    capabilities::print_registry(console);
    if report.gate_passed() {
        console.write_line("Foundation hardening phase 1: passed");
        console.write_line("Next gate: fresh SanjuOS PML4 and hardware page-table ownership");
    } else {
        console.write_line("Foundation hardening phase 1: failed");
    }
}

fn write_hex_u64(console: &mut dyn Console, value: u64) {
    for shift in (0..16).rev() {
        let nibble = u8::try_from((value >> (shift * 4)) & 0x0f).unwrap_or(0);
        console.write_byte(if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        });
    }
}

fn write_state(console: &mut dyn Console, label: &str, active: bool) {
    console.write_str(label);
    if active {
        console.write_line(": active");
    } else {
        console.write_line(": inactive");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BootInfo, Console, FoundationHardeningReport, M4Report, M5Report, MemoryMapInfo,
        kernel_main, kernel_main_foundation_hardening, kernel_main_m5,
    };
    use std::string::String;

    #[derive(Default)]
    struct RecordingConsole {
        output: String,
    }

    impl Console for RecordingConsole {
        fn write_byte(&mut self, byte: u8) {
            self.output.push(char::from(byte));
        }
    }

    const fn sample_map() -> MemoryMapInfo {
        MemoryMapInfo {
            buffer_address: 0x10_0000,
            buffer_capacity: 128 * 1024,
            map_size: 4_000,
            map_key: 7,
            descriptor_size: 40,
            descriptor_version: 1,
            reserved: 0,
            descriptor_count: 100,
        }
    }

    const fn sample_report() -> M4Report {
        M4Report {
            kernel_stack_active: true,
            gdt_active: true,
            tss_active: true,
            idt_active: true,
            breakpoint_self_test_passed: true,
            timer_interrupts_active: true,
            timer_ticks: 10,
            timer_hz: 100,
            keyboard_interrupt_path_active: true,
            keyboard_irqs: 1,
            keyboard_scancodes_dropped: 0,
            usable_frames: 1_024,
            allocated_frames: 3,
            heap_allocations: 2,
            heap_remaining_bytes: 250_000,
            scheduler_active: true,
            scheduler_tasks: 3,
            scheduler_context_switches: 8,
            scheduler_dispatches: 8,
            shell_active: true,
            shell_commands_executed: 4,
            ramfs_active: true,
            ramfs_files: 2,
        }
    }

    const fn sample_m5_report() -> M5Report {
        M5Report {
            paging_ownership_active: true,
            active_page_table_root: 0x1000,
            four_level_paging_active: true,
            mapping_api_active: true,
            page_flags_active: true,
            boot_memory_reclaim_active: true,
            guard_pages_active: true,
            write_xor_execute_active: true,
            kernel_heap_active: true,
            heap_allocations: 3,
            heap_frees: 1,
            page_fault_diagnostics_active: true,
            user_gdt_active: true,
            ring3_execution_active: true,
            user_address_space_isolation_active: true,
            user_stacks_active: true,
            process_control_blocks_active: true,
            context_switching_active: true,
            preemptive_scheduling_active: true,
            syscall_interface_active: true,
            safe_user_memory_active: true,
            elf64_loader_active: true,
            user_programs_launched: 3,
            user_processes_exited: 2,
            user_fault_isolation_passed: true,
            startup_experience_active: true,
            sanjuos_brand_printed: true,
        }
    }

    #[test]
    fn foundation_hardening_banner_uses_truthful_acceptance_evidence() {
        let mut console = RecordingConsole::default();
        let report = FoundationHardeningReport {
            toolchain_pinned: true,
            capability_registry_synchronized: true,
            architecture_separation_verified: true,
            boot_info_version: 1,
            ownership_map_active: true,
            ownership_ranges: 2,
            overlap_detection_passed: true,
            frame_allocation_unique: true,
            frame_reuse_passed: true,
            double_free_detection_passed: true,
            reserved_frame_detection_passed: true,
            bootstrap_pool_active: true,
            bootstrap_pool_capacity: 256,
            bootstrap_pool_remaining: 256,
            m5_regression_passed: true,
        };
        kernel_main_foundation_hardening(&mut console, report);
        assert!(report.gate_passed());
        assert!(console.output.contains("Foundation hardening phase 1: passed\r\n"));
        assert!(console.output.contains("Frame allocation/free test: passed\r\n"));
    }

    #[test]
    fn m5_banner_confirms_protected_userspace_gate() {
        let mut console = RecordingConsole::default();
        let info = BootInfo::new(
            "x86_64",
            "UEFI",
            "Milestone M5: protected user-space foundation and branded startup.",
            sample_map(),
        )
        .unwrap();
        kernel_main_m5(&mut console, info, sample_m5_report());
        assert!(console.output.contains("SanjuOS M5\r\n"));
        assert!(console.output.contains("Inherited page-table root captured: active\r\n"));
        assert!(console.output.contains("Ring 3 execution: active\r\n"));
        assert!(console.output.contains("System-call interface: active\r\n"));
        assert!(console.output.contains("ELF64 loader: active\r\n"));
        assert!(console.output.contains("User processes launched: 3\r\n"));
        assert!(
            console
                .output
                .contains("M5 protected user-space gate: passed\r\n")
        );
    }

    #[test]
    fn m5_gate_rejects_missing_fault_isolation() {
        assert!(sample_m5_report().gate_passed());
        let mut failed = sample_m5_report();
        failed.user_fault_isolation_passed = false;
        assert!(!failed.gate_passed());
    }

    #[test]
    fn m4_banner_confirms_interrupt_scheduler_shell_and_fs_gates() {
        let mut console = RecordingConsole::default();
        let info = BootInfo::new(
            "x86_64",
            "UEFI",
            "Milestone M4: interrupt-driven runtime and interactive kernel environment.",
            sample_map(),
        )
        .unwrap();

        kernel_main(&mut console, info, sample_report());

        assert!(console.output.contains("PIT timer interrupts: active\r\n"));
        assert!(
            console
                .output
                .contains("PS/2 keyboard interrupt path: active\r\n")
        );
        assert!(console.output.contains("Round-robin scheduler: active\r\n"));
        assert!(
            console
                .output
                .contains("Interactive kernel shell: active\r\n")
        );
        assert!(console.output.contains("RAM filesystem: active\r\n"));
        assert!(
            console
                .output
                .contains("M4 interactive runtime gate: passed\r\n")
        );
    }

    #[test]
    fn m4_gate_rejects_missing_runtime_evidence() {
        assert!(sample_report().gate_passed());
        let mut failed = sample_report();
        failed.timer_ticks = 0;
        assert!(!failed.gate_passed());
    }

    #[test]
    fn memory_map_validation_rejects_inconsistent_metadata() {
        assert!(sample_map().is_structurally_valid());

        let mut invalid = sample_map();
        invalid.descriptor_count = 99;
        assert!(!invalid.is_structurally_valid());

        invalid = sample_map();
        invalid.map_size = invalid.buffer_capacity + 1;
        assert!(!invalid.is_structurally_valid());
    }

    #[test]
    fn console_integer_output_is_allocation_free() {
        let mut console = RecordingConsole::default();
        console.write_u64(0);
        console.write_byte(b' ');
        console.write_u64(12_345_678_901);
        assert_eq!(console.output, "0 12345678901");
    }
}
