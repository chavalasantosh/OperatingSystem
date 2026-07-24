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
