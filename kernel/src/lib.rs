#![cfg_attr(not(test), no_std)]

pub mod memory;

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

    /// Writes an unsigned integer without allocation or formatting machinery.
    fn write_usize(&mut self, mut value: usize) {
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
///
/// The buffer is owned by the boot image and remains mapped when control moves
/// into the kernel. The kernel must interpret entries using `descriptor_size`;
/// it must not assume descriptors are tightly packed using a Rust structure.
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

/// Runtime evidence produced by the M2 protection and memory subsystems.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct M2Report {
    pub kernel_stack_active: bool,
    pub gdt_active: bool,
    pub tss_active: bool,
    pub idt_active: bool,
    pub breakpoint_self_test_passed: bool,
    pub usable_frames: usize,
    pub allocated_frames: usize,
    pub heap_allocations: usize,
    pub heap_remaining_bytes: usize,
}

impl M2Report {
    #[must_use]
    pub const fn gate_passed(self) -> bool {
        self.kernel_stack_active
            && self.gdt_active
            && self.tss_active
            && self.idt_active
            && self.breakpoint_self_test_passed
            && self.usable_frames > 0
            && self.allocated_frames > 0
            && self.heap_allocations > 0
    }
}

/// Architecture-independent M2 status entry point after firmware exit.
pub fn kernel_main(console: &mut dyn Console, boot_info: BootInfo, report: M2Report) {
    console.write_line("");
    console.write_line("SanjuOS");
    console.write_line(boot_info.milestone);
    console.write_str("Architecture: ");
    console.write_line(boot_info.architecture);
    console.write_str("Firmware: ");
    console.write_line(boot_info.firmware);
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
    console.write_str("Usable physical frames: ");
    console.write_usize(report.usable_frames);
    console.write_line("");
    console.write_str("Allocated test frames: ");
    console.write_usize(report.allocated_frames);
    console.write_line("");
    console.write_str("Bootstrap heap allocations: ");
    console.write_usize(report.heap_allocations);
    console.write_line("");
    console.write_str("Bootstrap heap remaining bytes: ");
    console.write_usize(report.heap_remaining_bytes);
    console.write_line("");

    if report.gate_passed() {
        console.write_line("M2 core kernel gate: passed");
        console.write_line("Next gate: timer interrupts, keyboard, scheduler, and shell");
    } else {
        console.write_line("M2 core kernel gate: failed");
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
    use super::{BootInfo, Console, M2Report, MemoryMapInfo, kernel_main};
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

    const fn sample_report() -> M2Report {
        M2Report {
            kernel_stack_active: true,
            gdt_active: true,
            tss_active: true,
            idt_active: true,
            breakpoint_self_test_passed: true,
            usable_frames: 1_024,
            allocated_frames: 3,
            heap_allocations: 2,
            heap_remaining_bytes: 250_000,
        }
    }

    #[test]
    fn m2_banner_confirms_protection_and_memory_gates() {
        let mut console = RecordingConsole::default();
        let info = BootInfo::new(
            "x86_64",
            "UEFI",
            "Milestone M2: CPU protection and early memory management.",
            sample_map(),
        );

        kernel_main(&mut console, info, sample_report());

        assert!(console.output.contains("SanjuOS\r\n"));
        assert!(
            console
                .output
                .contains("Protected kernel stack: active\r\n")
        );
        assert!(console.output.contains("GDT: active\r\n"));
        assert!(console.output.contains("TSS: active\r\n"));
        assert!(
            console
                .output
                .contains("Breakpoint exception self-test: active\r\n")
        );
        assert!(console.output.contains("M2 core kernel gate: passed\r\n"));
    }

    #[test]
    fn m2_gate_rejects_missing_runtime_evidence() {
        assert!(sample_report().gate_passed());
        let mut failed = sample_report();
        failed.idt_active = false;
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
    fn write_line_and_integer_output_are_allocation_free_boundaries() {
        let mut console = RecordingConsole::default();
        console.write_line("ready");
        console.write_usize(0);
        console.write_byte(b' ');
        console.write_usize(12_345);

        assert_eq!(console.output, "ready\r\n0 12345");
    }
}
