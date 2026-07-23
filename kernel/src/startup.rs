//! SanjuOS branded startup experience.

use crate::Console;

pub const STARTUP_LOGO: &[&str] = &[
    "  _____              _        ____   _____ ",
    " / ____|            (_)      / __ \\ / ____|",
    "| (___   __ _ _ __  _ _   _| |  | | (___  ",
    " \\___ \\ / _` | '_ \\| | | | | |  | |\\___ \\ ",
    " ____) | (_| | | | | | |_| | |__| |____) |",
    "|_____/ \\__,_|_| |_|_|\\__,_|\\____/|_____/ ",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupStage {
    Firmware,
    Memory,
    Cpu,
    Interrupts,
    Paging,
    Heap,
    Userspace,
    Shell,
}

impl StartupStage {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Firmware => "Firmware ownership",
            Self::Memory => "Physical memory",
            Self::Cpu => "CPU protection",
            Self::Interrupts => "Interrupt runtime",
            Self::Paging => "Virtual memory",
            Self::Heap => "Kernel heap",
            Self::Userspace => "Protected userspace",
            Self::Shell => "System shell",
        }
    }
}

pub fn print_logo(console: &mut dyn Console) {
    console.write_line("");
    for line in STARTUP_LOGO {
        console.write_line(line);
    }
    console.write_line("Secure. Fast. Yours.");
    console.write_line("");
}

pub fn print_stage(console: &mut dyn Console, stage: StartupStage, active: bool) {
    console.write_str(if active { "[OK] " } else { "[!!] " });
    console.write_line(stage.label());
}

pub fn print_failure(console: &mut dyn Console, code: &str, message: &str) {
    console.write_line("");
    console.write_line("SANJUOS STARTUP FAILURE");
    console.write_str("Code: ");
    console.write_line(code);
    console.write_str("Reason: ");
    console.write_line(message);
}

#[cfg(test)]
mod tests {
    use super::{StartupStage, print_logo, print_stage};
    use crate::Console;
    use std::string::String;

    #[derive(Default)]
    struct RecordingConsole(String);

    impl Console for RecordingConsole {
        fn write_byte(&mut self, byte: u8) {
            self.0.push(char::from(byte));
        }
    }

    #[test]
    fn startup_prints_sanjuos_brand() {
        let mut console = RecordingConsole::default();
        print_logo(&mut console);
        print_stage(&mut console, StartupStage::Userspace, true);
        assert!(console.0.contains("Secure. Fast. Yours."));
        assert!(console.0.contains("[OK] Protected userspace"));
    }
}
