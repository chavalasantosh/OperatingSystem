use core::arch::asm;
use sanju_kernel::Console;

const COM1: u16 = 0x03f8;
const TRANSMITTER_HOLDING: u16 = 0;
const INTERRUPT_ENABLE: u16 = 1;
const FIFO_CONTROL: u16 = 2;
const LINE_CONTROL: u16 = 3;
const MODEM_CONTROL: u16 = 4;
const LINE_STATUS: u16 = 5;
const DIVISOR_LATCH_LOW: u16 = 0;
const DIVISOR_LATCH_HIGH: u16 = 1;
const DATA_READY_TO_SEND: u8 = 0x20;

/// Minimal 16550-compatible COM1 logger used before a full driver framework.
pub struct SerialConsole;

impl SerialConsole {
    /// Configures COM1 for 115200 baud, 8 data bits, no parity, one stop bit.
    #[must_use]
    pub fn initialize() -> Self {
        // SAFETY: SanjuOS executes at firmware/kernel privilege on x86-64. The
        // selected ports are the conventional 16550 COM1 register range. QEMU
        // provides this device for M2; physical deployment is not yet allowed.
        unsafe {
            outb(COM1 + INTERRUPT_ENABLE, 0x00);
            outb(COM1 + LINE_CONTROL, 0x80);
            outb(COM1 + DIVISOR_LATCH_LOW, 0x01);
            outb(COM1 + DIVISOR_LATCH_HIGH, 0x00);
            outb(COM1 + LINE_CONTROL, 0x03);
            outb(COM1 + FIFO_CONTROL, 0xc7);
            outb(COM1 + MODEM_CONTROL, 0x0b);
        }
        Self
    }

    fn write_raw(byte: u8) {
        // Bound the polling loop so missing physical serial hardware cannot
        // deadlock the boot path. QEMU normally becomes ready immediately.
        for _ in 0..100_000 {
            // SAFETY: See `initialize`; this reads the COM1 line-status port.
            if unsafe { inb(COM1 + LINE_STATUS) } & DATA_READY_TO_SEND != 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // SAFETY: See `initialize`; this writes the COM1 transmit register.
        unsafe {
            outb(COM1 + TRANSMITTER_HOLDING, byte);
        }
    }
}

impl Console for SerialConsole {
    fn write_byte(&mut self, byte: u8) {
        Self::write_raw(byte);
    }
}

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: The caller documents ownership and validity of the I/O port.
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: The caller documents ownership and validity of the I/O port.
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
