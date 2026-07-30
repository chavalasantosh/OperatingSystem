use core::arch::asm;

const DEBUG_PORT: u16 = 0x00e9;
const EXIT_PORT: u16 = 0x00f4;
const EXIT_SUCCESS: u32 = 0x10;
const EXIT_FAILURE: u32 = 0x11;
const EXIT_BOOT_FAILURE: u32 = 0x12;

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

pub fn debug_write(bytes: &[u8]) {
    for byte in bytes {
        debug_byte(*byte);
    }
}

pub fn debug_write_line(text: &str) {
    debug_write(text.as_bytes());
    debug_write(b"\r\n");
}

#[allow(clippy::cast_possible_truncation)]
pub fn debug_write_label_hex(label: &str, value: u64) {
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

pub fn exit_success() -> ! {
    exit(EXIT_SUCCESS)
}

pub fn exit_failure() -> ! {
    exit(EXIT_FAILURE)
}

pub fn exit_boot_failure() -> ! {
    exit(EXIT_BOOT_FAILURE)
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
