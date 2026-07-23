#![cfg_attr(not(test), no_std)]

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
/// into the M1 kernel. The kernel must interpret entries using
/// `descriptor_size`; it must not assume descriptors are tightly packed using
/// the Rust structure size.
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

/// Earliest architecture-independent kernel entry point after firmware exit.
///
/// M1 proves that the kernel receives an owned boot-information structure and
/// can continue emitting diagnostics without UEFI console services.
pub fn kernel_main(console: &mut dyn Console, boot_info: BootInfo) {
    console.write_line("");
    console.write_line("SanjuOS");
    console.write_line(boot_info.milestone);
    console.write_str("Architecture: ");
    console.write_line(boot_info.architecture);
    console.write_str("Firmware: ");
    console.write_line(boot_info.firmware);
    console.write_line("Firmware boot services: exited");
    console.write_str("Memory descriptors: ");
    console.write_usize(boot_info.memory_map.descriptor_count);
    console.write_line("");
    console.write_str("Memory-map bytes: ");
    console.write_usize(boot_info.memory_map.map_size);
    console.write_line("");
    console.write_line("Kernel ownership gate: passed");
    console.write_line("Next gate: CPU exceptions and protected kernel stack");
}

#[cfg(test)]
mod tests {
    use super::{BootInfo, Console, MemoryMapInfo, kernel_main};
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

    #[test]
    fn boot_banner_confirms_kernel_ownership() {
        let mut console = RecordingConsole::default();
        let info = BootInfo::new(
            "x86_64",
            "UEFI",
            "Milestone M1: firmware exit and kernel ownership.",
            sample_map(),
        );

        kernel_main(&mut console, info);

        assert!(console.output.contains("SanjuOS\r\n"));
        assert!(console.output.contains("Architecture: x86_64\r\n"));
        assert!(
            console
                .output
                .contains("Firmware boot services: exited\r\n")
        );
        assert!(console.output.contains("Memory descriptors: 100\r\n"));
        assert!(console.output.contains("Kernel ownership gate: passed\r\n"));
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
