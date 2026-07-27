#![allow(clippy::module_name_repetitions)]

//! Allocation-free command shell for the early kernel runtime.

use core::str;

use crate::Console;
use crate::fs::{MAX_FILE_BYTES, RamFs};
use crate::vfs::{HandleRights, MAX_PATH_BYTES, Vfs, VfsError};

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
    pub pci_functions: usize,
    pub storage_controllers: usize,
    pub virtio_block_targets: usize,
    pub block_capacity_sectors: u64,
    pub block_queue_size: usize,
    pub block_read_test_passed: bool,
    pub block_write_test_passed: bool,
    pub cache_capacity: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_device_reads: u64,
    pub cache_dirty_entries: usize,
    pub cache_read_only_policy: bool,
    pub vfs_mounts: usize,
    pub vfs_handle_capacity: usize,
    pub vfs_path_normalization_passed: bool,
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
        console.write_line("Soma OS kernel shell ready.");
        console.write_line("Type 'help' for commands.");
        write_prompt(console);
    }

    /// Processes one decoded ASCII byte.
    pub fn feed_byte(
        &mut self,
        byte: u8,
        console: &mut dyn Console,
        vfs: &mut Vfs<RamFs>,
        environment: &ShellEnvironment,
    ) {
        match byte {
            b'\r' | b'\n' => {
                console.write_line("");
                let command_len = self.command_len;
                let mut command_copy = [0_u8; COMMAND_BUFFER_BYTES];
                command_copy[..command_len].copy_from_slice(&self.command[..command_len]);
                self.command_len = 0;

                if let Ok(line) = str::from_utf8(&command_copy[..command_len])
                    && !line.trim().is_empty()
                {
                    execute_line(line.trim(), console, vfs, environment);
                    self.commands_executed = self.commands_executed.saturating_add(1);
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
    vfs: &mut Vfs<RamFs>,
    environment: &ShellEnvironment,
) {
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return;
    };

    match command {
        "help" => {
            console.write_line(concat!(
                "Commands: help version uptime memory irq tasks pci block cache mounts ls cat ",
                "write echo clear userspace",
            ));
        }
        "version" => console.write_line("Soma OS 0.0.11-prealpha (M6C)"),
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
        "pci" => {
            console.write_str("PCI functions: ");
            console.write_usize(environment.pci_functions);
            console.write_str(", storage controllers: ");
            console.write_usize(environment.storage_controllers);
            console.write_str(", virtio-blk targets: ");
            console.write_usize(environment.virtio_block_targets);
            console.write_line("");
        }
        "block" => {
            console.write_str("Virtio block: ");
            console.write_u64(environment.block_capacity_sectors);
            console.write_str(" sectors, queue ");
            console.write_usize(environment.block_queue_size);
            console.write_str(", read ");
            console.write_str(if environment.block_read_test_passed {
                "passed"
            } else {
                "failed"
            });
            console.write_str(", write/readback ");
            console.write_line(if environment.block_write_test_passed {
                "passed"
            } else {
                "failed"
            });
        }
        "cache" => {
            console.write_str("Block cache: ");
            console.write_usize(environment.cache_capacity);
            console.write_str(" sectors, hits ");
            console.write_u64(environment.cache_hits);
            console.write_str(", misses ");
            console.write_u64(environment.cache_misses);
            console.write_str(", device reads ");
            console.write_u64(environment.cache_device_reads);
            console.write_str(", dirty ");
            console.write_usize(environment.cache_dirty_entries);
            console.write_str(", policy ");
            console.write_line(if environment.cache_read_only_policy {
                "read-only"
            } else {
                "unverified"
            });
        }
        "mounts" => {
            console.write_str("VFS mounts: ");
            console.write_usize(environment.vfs_mounts);
            console.write_str(", handle capacity: ");
            console.write_usize(environment.vfs_handle_capacity);
            console.write_str(", normalized paths: ");
            console.write_line(if environment.vfs_path_normalization_passed {
                "active"
            } else {
                "inactive"
            });
            vfs.mounts().visit(|mount| {
                console.write_str(mount.path.as_str());
                console.write_str(" ");
                console.write_str(mount.superblock.filesystem_name);
                console.write_str(" ");
                console.write_line(if mount.superblock.read_only {
                    "read-only"
                } else {
                    "read-write"
                });
            });
        }
        "ls" => {
            let mut found = false;
            let result = vfs.visit_directory("/", &mut |name, _inode| {
                found = true;
                console.write_line(name);
            });
            if result.is_err() {
                console.write_line("filesystem error");
            } else if !found {
                console.write_line("<empty>");
            }
        }
        "cat" => {
            let Some(input_path) = parts.next() else {
                console.write_line("usage: cat <file>");
                return;
            };
            let mut path_storage = [0_u8; MAX_PATH_BYTES];
            let Some(path) = shell_path(input_path, &mut path_storage) else {
                console.write_line("invalid path");
                return;
            };
            match vfs.open(path, HandleRights::ReadOnly) {
                Ok(handle) => {
                    let mut data = [0_u8; MAX_FILE_BYTES];
                    let read_result = vfs.read(handle, &mut data);
                    let close_result = vfs.close(handle);
                    match (read_result, close_result) {
                        (Ok(read), Ok(())) => write_bytes(console, &data[..read]),
                        _ => console.write_line("filesystem error"),
                    }
                }
                Err(VfsError::NotFound) => console.write_line("file not found"),
                Err(VfsError::Path(_)) => console.write_line("invalid path"),
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
            let mut path_storage = [0_u8; MAX_PATH_BYTES];
            let Some(path) = shell_path(name, &mut path_storage) else {
                console.write_line("invalid path");
                return;
            };
            match vfs.create_or_replace(path, data.as_bytes()) {
                Ok(_) => console.write_line("written"),
                Err(VfsError::Path(_)) => console.write_line("invalid path"),
                Err(VfsError::FileTooLarge) => console.write_line("file data too large"),
                Err(VfsError::NoSpace) => console.write_line("file table full"),
                Err(VfsError::ReadOnly) => console.write_line("filesystem is read-only"),
                Err(_) => console.write_line("filesystem error"),
            }
        }
        "echo" => {
            let text = line.strip_prefix("echo").unwrap_or("").trim_start();
            console.write_line(text);
        }
        "userspace" => {
            console.write_line("M5 protected userspace, syscalls, and ELF loader are active.")
        }
        "clear" => console.write_str("\x1b[2J\x1b[H"),
        _ => console.write_line("unknown command; type 'help'"),
    }
}

fn write_prompt(console: &mut dyn Console) {
    console.write_str("soma> ");
}

fn shell_path<'a>(input: &'a str, storage: &'a mut [u8; MAX_PATH_BYTES]) -> Option<&'a str> {
    if input.starts_with('/') {
        return Some(input);
    }
    let required = input.len().checked_add(1)?;
    if required > storage.len() {
        return None;
    }
    storage[0] = b'/';
    storage[1..required].copy_from_slice(input.as_bytes());
    str::from_utf8(&storage[..required]).ok()
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
    use crate::Console;
    use crate::fs::RamFs;
    use crate::vfs::Vfs;
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
        let mut vfs = Vfs::new(RamFs::with_defaults());
        Shell::start(&mut console);

        let environment = ShellEnvironment {
            pci_functions: 7,
            storage_controllers: 2,
            virtio_block_targets: 1,
            block_capacity_sectors: 16_384,
            block_queue_size: 8,
            block_read_test_passed: true,
            block_write_test_passed: true,
            cache_capacity: 16,
            cache_hits: 1,
            cache_misses: 1,
            cache_device_reads: 1,
            cache_dirty_entries: 0,
            cache_read_only_policy: true,
            vfs_mounts: 1,
            vfs_handle_capacity: 32,
            vfs_path_normalization_passed: true,
            ..ShellEnvironment::default()
        };
        for byte in b"write note.txt hello\ncat note.txt\npci\nblock\ncache\nmounts\n" {
            shell.feed_byte(*byte, &mut console, &mut vfs, &environment);
        }

        assert!(console.output.contains("written\r\n"));
        assert!(console.output.contains("hello\r\n"));
        assert!(
            console
                .output
                .contains("PCI functions: 7, storage controllers: 2, virtio-blk targets: 1\r\n")
        );
        assert!(console.output.contains(
            "Virtio block: 16384 sectors, queue 8, read passed, write/readback passed\r\n"
        ));
        assert!(console.output.contains(
            "Block cache: 16 sectors, hits 1, misses 1, device reads 1, dirty 0, policy read-only\r\n"
        ));
        assert!(console.output.contains("/ ramfs read-write\r\n"));
        assert_eq!(shell.commands_executed(), 6);
    }
}
