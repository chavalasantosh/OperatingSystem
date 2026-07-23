#![allow(clippy::module_name_repetitions)]

//! Allocation-free command shell for the early kernel runtime.

use core::str;

use crate::Console;
use crate::fs::{FsError, RamFs};

const COMMAND_BUFFER_BYTES: usize = 128;

/// Snapshot of runtime state displayed by shell commands.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShellEnvironment {
    pub timer_ticks: u64,
    pub timer_hz: u64,
    pub keyboard_irqs: u64,
    pub usable_frames: usize,
    pub allocated_frames: usize,
    pub scheduler_tasks: usize,
    pub scheduler_switches: u64,
    pub scheduler_dispatches: u64,
}

/// Interactive line editor and command dispatcher.
pub struct Shell {
    command: [u8; COMMAND_BUFFER_BYTES],
    command_len: usize,
    commands_executed: usize,
}

impl Shell {
    /// Creates an empty shell.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            command: [0; COMMAND_BUFFER_BYTES],
            command_len: 0,
            commands_executed: 0,
        }
    }

    /// Prints the shell banner and first prompt.
    pub fn start(console: &mut dyn Console) {
        console.write_line("");
        console.write_line("SanjuOS kernel shell ready.");
        console.write_line("Type 'help' for commands.");
        write_prompt(console);
    }

    /// Processes one decoded ASCII byte.
    pub fn feed_byte(
        &mut self,
        byte: u8,
        console: &mut dyn Console,
        fs: &mut RamFs,
        environment: &ShellEnvironment,
    ) {
        match byte {
            b'\r' | b'\n' => {
                console.write_line("");
                let command_len = self.command_len;
                let mut command_copy = [0_u8; COMMAND_BUFFER_BYTES];
                command_copy[..command_len].copy_from_slice(&self.command[..command_len]);
                self.command_len = 0;

                if let Ok(line) = str::from_utf8(&command_copy[..command_len]) {
                    if !line.trim().is_empty() {
                        execute_line(line.trim(), console, fs, environment);
                        self.commands_executed = self.commands_executed.saturating_add(1);
                    }
                }
                write_prompt(console);
            }
            0x08 | 0x7f => {
                if self.command_len > 0 {
                    self.command_len -= 1;
                    console.write_str("\x08 \x08");
                }
            }
            b'\t' => {
                self.push_byte(b' ', console);
                self.push_byte(b' ', console);
            }
            0x20..=0x7e => self.push_byte(byte, console),
            _ => {}
        }
    }

    /// Returns the number of non-empty commands executed.
    #[must_use]
    pub const fn commands_executed(&self) -> usize {
        self.commands_executed
    }

    fn push_byte(&mut self, byte: u8, console: &mut dyn Console) {
        if self.command_len < self.command.len() - 1 {
            self.command[self.command_len] = byte;
            self.command_len += 1;
            console.write_byte(byte);
        }
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::too_many_lines)]
fn execute_line(
    line: &str,
    console: &mut dyn Console,
    fs: &mut RamFs,
    environment: &ShellEnvironment,
) {
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return;
    };

    match command {
        "help" => {
            console.write_line(
                "Commands: help version uptime memory irq tasks ls cat write echo clear",
            );
        }
        "version" => console.write_line("SanjuOS 0.0.4-prealpha (M4)"),
        "uptime" => {
            console.write_str("Timer ticks: ");
            console.write_u64(environment.timer_ticks);
            console.write_str(" at ");
            console.write_u64(environment.timer_hz);
            console.write_line(" Hz");
        }
        "memory" => {
            console.write_str("Usable frames: ");
            console.write_usize(environment.usable_frames);
            console.write_str(", allocated bootstrap frames: ");
            console.write_usize(environment.allocated_frames);
            console.write_line("");
        }
        "irq" => {
            console.write_str("Keyboard IRQs: ");
            console.write_u64(environment.keyboard_irqs);
            console.write_line("");
        }
        "tasks" => {
            console.write_str("Tasks: ");
            console.write_usize(environment.scheduler_tasks);
            console.write_str(", switches: ");
            console.write_u64(environment.scheduler_switches);
            console.write_str(", dispatches: ");
            console.write_u64(environment.scheduler_dispatches);
            console.write_line("");
        }
        "ls" => {
            if fs.file_count() == 0 {
                console.write_line("<empty>");
            } else {
                fs.visit_names(|name| console.write_line(name));
            }
        }
        "cat" => {
            let Some(name) = parts.next() else {
                console.write_line("usage: cat <file>");
                return;
            };
            match fs.read(name) {
                Ok(data) => write_bytes(console, data),
                Err(FsError::NotFound) => console.write_line("file not found"),
                Err(_) => console.write_line("filesystem error"),
            }
        }
        "write" => {
            let mut split = line.splitn(3, ' ');
            let _ = split.next();
            let Some(name) = split.next().filter(|name| !name.is_empty()) else {
                console.write_line("usage: write <file> <text>");
                return;
            };
            let Some(data) = split.next() else {
                console.write_line("usage: write <file> <text>");
                return;
            };
            match fs.write(name, data.as_bytes()) {
                Ok(()) => console.write_line("written"),
                Err(FsError::NameTooLong) => console.write_line("filename too long"),
                Err(FsError::DataTooLarge) => console.write_line("file data too large"),
                Err(FsError::FileTableFull) => console.write_line("file table full"),
                Err(_) => console.write_line("filesystem error"),
            }
        }
        "echo" => {
            let text = line.strip_prefix("echo").unwrap_or("").trim_start();
            console.write_line(text);
        }
        "clear" => console.write_str("\x1b[2J\x1b[H"),
        _ => console.write_line("unknown command; type 'help'"),
    }
}

fn write_prompt(console: &mut dyn Console) {
    console.write_str("sanju> ");
}

fn write_bytes(console: &mut dyn Console, bytes: &[u8]) {
    for byte in bytes {
        if *byte == b'\n' {
            console.write_line("");
        } else if byte.is_ascii() {
            console.write_byte(*byte);
        }
    }
    if bytes.last().is_some_and(|byte| *byte != b'\n') {
        console.write_line("");
    }
}

#[cfg(test)]
mod tests {
    use super::{Shell, ShellEnvironment};
    use crate::fs::RamFs;
    use crate::Console;
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

    #[test]
    fn shell_executes_commands_and_writes_files() {
        let mut shell = Shell::new();
        let mut console = RecordingConsole::default();
        let mut fs = RamFs::with_defaults();
        Shell::start(&mut console);

        for byte in b"write note.txt hello\ncat note.txt\n" {
            shell.feed_byte(
                *byte,
                &mut console,
                &mut fs,
                &ShellEnvironment::default(),
            );
        }

        assert!(console.output.contains("written\r\n"));
        assert!(console.output.contains("hello\r\n"));
        assert_eq!(shell.commands_executed(), 2);
    }
}
