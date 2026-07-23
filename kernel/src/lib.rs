#![cfg_attr(not(test), no_std)]

pub mod fs;
pub mod input;
pub mod memory;
pub mod scheduler;
pub mod shell;

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

/// Firmware memory map retained after UEFI boot services are terminated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryMapInfo {
    pub buffer_address: usize,
    pub buffer_capacity: usize,
    pub map_size: usize,
    pub map_key: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,
    pub descriptor_count: usize,
}

impl MemoryMapInfo {
    #[must_use]
    pub const fn is_structurally_valid(self) -> bool {
        self.buffer_address != 0
            && self.map_size <= self.buffer_capacity
            && self.descriptor_size != 0
            && self.map_size.is_multiple_of(self.descriptor_size)
            && self.descriptor_count == self.map_size / self.descriptor_size
    }
}

/// Owned facts transferred from the platform boot layer into the kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootInfo {
    pub architecture: &'static str,
    pub firmware: &'static str,
    pub milestone: &'static str,
    pub memory_map: MemoryMapInfo,
}

impl BootInfo {
    #[must_use]
    pub const fn new(
        architecture: &'static str,
        firmware: &'static str,
        milestone: &'static str,
        memory_map: MemoryMapInfo,
    ) -> Self {
        Self {
            architecture,
            firmware,
            milestone,
            memory_map,
        }
    }
}

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
    console.write_line(boot_info.milestone);
    console.write_str("Architecture: ");
    console.write_line(boot_info.architecture);
    console.write_str("Firmware: ");
    console.write_line(boot_info.firmware);
    console.write_line("Firmware boot services: exited");
    write_state(console, "Protected kernel stack", report.kernel_stack_active);
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
        console.write_line(
            "Next gate: paging ownership, user mode, syscalls, and executable loading",
        );
    } else {
        console.write_line("M4 interactive runtime gate: failed");
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
    use super::{BootInfo, Console, M4Report, MemoryMapInfo, kernel_main};
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

    #[test]
    fn m4_banner_confirms_interrupt_scheduler_shell_and_fs_gates() {
        let mut console = RecordingConsole::default();
        let info = BootInfo::new(
            "x86_64",
            "UEFI",
            "Milestone M4: interrupt-driven runtime and interactive kernel environment.",
            sample_map(),
        );

        kernel_main(&mut console, info, sample_report());

        assert!(console.output.contains("PIT timer interrupts: active\r\n"));
        assert!(
            console
                .output
                .contains("PS/2 keyboard interrupt path: active\r\n")
        );
        assert!(
            console
                .output
                .contains("Round-robin scheduler: active\r\n")
        );
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
