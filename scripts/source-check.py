#!/usr/bin/env python3
"""Dependency-free structural checks runnable before the Rust toolchain exists."""

from __future__ import annotations

import ctypes
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
BOOT = ROOT / "boot/uefi/src/main.rs"
CPU = ROOT / "boot/uefi/src/cpu.rs"
KERNEL = ROOT / "kernel/src/lib.rs"
MEMORY = ROOT / "kernel/src/memory.rs"
INPUT = ROOT / "kernel/src/input.rs"
SCHEDULER = ROOT / "kernel/src/scheduler.rs"
SHELL = ROOT / "kernel/src/shell.rs"
FILESYSTEM = ROOT / "kernel/src/fs.rs"
PAGING = ROOT / "kernel/src/paging.rs"
HEAP = ROOT / "kernel/src/heap.rs"
PROCESS = ROOT / "kernel/src/process.rs"
SYSCALL = ROOT / "kernel/src/syscall.rs"
ELF = ROOT / "kernel/src/elf.rs"
STARTUP = ROOT / "kernel/src/startup.rs"
INIT_ELF = ROOT / "user/programs/bin/init.elf"
HELLO_ELF = ROOT / "user/programs/bin/hello.elf"
FAULT_ELF = ROOT / "user/programs/bin/fault-test.elf"
LOGO = ROOT / "assets/branding/sanjuos-logo.png"
SMOKE = ROOT / "scripts/smoke-test.sh"
USER_BUILD = ROOT / "scripts/build-user-programs.sh"


def require(text: str, needle: str, source: Path) -> None:
    if needle not in text:
        raise AssertionError(f"{source}: missing required text: {needle!r}")


class EfiTableHeader(ctypes.Structure):
    _fields_ = [
        ("signature", ctypes.c_uint64),
        ("revision", ctypes.c_uint32),
        ("header_size", ctypes.c_uint32),
        ("crc32", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32),
    ]


class EfiMemoryDescriptor(ctypes.Structure):
    _fields_ = [
        ("memory_type", ctypes.c_uint32),
        ("padding", ctypes.c_uint32),
        ("physical_start", ctypes.c_uint64),
        ("virtual_start", ctypes.c_uint64),
        ("number_of_pages", ctypes.c_uint64),
        ("attribute", ctypes.c_uint64),
    ]


class TaskStateSegment(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("reserved_1", ctypes.c_uint32),
        ("privilege_stack_table", ctypes.c_uint64 * 3),
        ("reserved_2", ctypes.c_uint64),
        ("interrupt_stack_table", ctypes.c_uint64 * 7),
        ("reserved_3", ctypes.c_uint64),
        ("reserved_4", ctypes.c_uint16),
        ("io_map_base", ctypes.c_uint16),
    ]


class IdtEntry(ctypes.Structure):
    _fields_ = [
        ("offset_low", ctypes.c_uint16),
        ("selector", ctypes.c_uint16),
        ("ist", ctypes.c_uint8),
        ("type_attributes", ctypes.c_uint8),
        ("offset_middle", ctypes.c_uint16),
        ("offset_high", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32),
    ]


def validate_elf64(path: Path) -> None:
    data = path.read_bytes()
    if len(data) < 64 or data[:4] != b"\x7fELF":
        raise AssertionError(f"{path}: missing ELF64 header")
    if data[4:6] != bytes((2, 1)):
        raise AssertionError(f"{path}: expected 64-bit little-endian ELF")
    elf_type, machine = struct.unpack_from("<HH", data, 16)
    if elf_type != 3 or machine != 62:
        raise AssertionError(f"{path}: expected x86-64 position-independent ELF")
    program_offset = struct.unpack_from("<Q", data, 32)[0]
    program_size = struct.unpack_from("<H", data, 54)[0]
    program_count = struct.unpack_from("<H", data, 56)[0]
    if program_size < 56 or program_count == 0:
        raise AssertionError(f"{path}: missing program headers")
    load_segments = 0
    for index in range(program_count):
        offset = program_offset + index * program_size
        if offset + 56 > len(data):
            raise AssertionError(f"{path}: truncated program header")
        kind, flags = struct.unpack_from("<II", data, offset)
        if kind == 1:
            load_segments += 1
            if flags & 0x3 == 0x3:
                raise AssertionError(f"{path}: writable+executable load segment")
    if load_segments == 0:
        raise AssertionError(f"{path}: no loadable segment")


def main() -> int:
    boot = BOOT.read_text(encoding="utf-8")
    cpu = CPU.read_text(encoding="utf-8")
    kernel = KERNEL.read_text(encoding="utf-8")
    memory = MEMORY.read_text(encoding="utf-8")
    input_code = INPUT.read_text(encoding="utf-8")
    scheduler = SCHEDULER.read_text(encoding="utf-8")
    shell = SHELL.read_text(encoding="utf-8")
    filesystem = FILESYSTEM.read_text(encoding="utf-8")
    paging = PAGING.read_text(encoding="utf-8")
    heap = HEAP.read_text(encoding="utf-8")
    process = PROCESS.read_text(encoding="utf-8")
    syscall = SYSCALL.read_text(encoding="utf-8")
    elf = ELF.read_text(encoding="utf-8")
    startup = STARTUP.read_text(encoding="utf-8")
    smoke = SMOKE.read_text(encoding="utf-8")
    user_build = USER_BUILD.read_text(encoding="utf-8")

    require(boot, 'extern "efiapi" fn efi_main', BOOT)
    require(boot, "get_memory_map", BOOT)
    require(boot, "exit_boot_services", BOOT)
    require(boot, "cpu::switch_to_kernel_stack", BOOT)
    require(boot, "FrameAllocator::from_memory_map", BOOT)
    require(boot, "KernelHeap::new", BOOT)
    require(boot, "load_position_independent", BOOT)
    require(boot, "cpu::run_user_process", BOOT)
    require(cpu, 'asm!("int3"', CPU)
    require(cpu, 'asm!("lidt', CPU)
    require(cpu, '"ltr ax"', CPU)
    require(cpu, "sanju_double_fault_stub", CPU)
    require(cpu, "sanju_page_fault_stub", CPU)
    require(cpu, "sanju_timer_interrupt_stub", CPU)
    require(cpu, "sanju_keyboard_interrupt_stub", CPU)
    require(cpu, "remap_and_unmask_pic", CPU)
    require(cpu, "configure_pit", CPU)
    require(cpu, "USER_INTERRUPT_STACK", CPU)
    require(cpu, "configure_syscall_msrs", CPU)
    require(cpu, "sanju_enter_user_mode_asm", CPU)
    require(cpu, "sysretq", CPU)
    require(cpu, "iretq", CPU)
    require(cpu, "mark_user_range", CPU)
    require(memory, "pub struct FrameAllocator", MEMORY)
    require(memory, "EFI_CONVENTIONAL_MEMORY", MEMORY)
    require(memory, "pub struct BumpAllocator", MEMORY)
    require(input_code, "pub struct KeyboardDecoder", INPUT)
    require(scheduler, "pub struct Scheduler", SCHEDULER)
    require(shell, "pub struct Shell", SHELL)
    require(filesystem, "pub struct RamFs", FILESYSTEM)
    require(paging, "pub struct PageTableManager", PAGING)
    require(paging, "WriteExecuteViolation", PAGING)
    require(heap, "pub struct KernelHeap", HEAP)
    require(process, "pub struct ProcessControlBlock", PROCESS)
    require(process, "pub struct ProcessTable", PROCESS)
    require(syscall, "pub struct SyscallDispatcher", SYSCALL)
    for syscall_name in ("Write", "Read", "Exit", "Yield", "GetPid", "Open", "Close", "Spawn"):
        require(syscall, syscall_name, SYSCALL)
    require(elf, "load_position_independent", ELF)
    require(startup, "STARTUP_LOGO", STARTUP)
    require(startup, "Secure. Fast. Yours.", STARTUP)
    require(kernel, "pub struct M5Report", KERNEL)
    require(kernel, "Ring 3 execution", KERNEL)
    require(kernel, "M5 protected user-space gate: passed", KERNEL)
    require(
        smoke,
        "Milestone M5: protected user-space foundation and branded startup.",
        SMOKE,
    )
    require(user_build, "-z noseparate-code", USER_BUILD)
    for executable in (INIT_ELF, HELLO_ELF, FAULT_ELF):
        validate_elf64(executable)
    if LOGO.read_bytes()[:8] != b"\x89PNG\r\n\x1a\n":
        raise AssertionError(f"{LOGO}: missing PNG signature")

    assert ctypes.sizeof(EfiTableHeader) == 24
    assert ctypes.sizeof(EfiMemoryDescriptor) == 40
    assert EfiMemoryDescriptor.physical_start.offset == 8
    assert EfiMemoryDescriptor.number_of_pages.offset == 24
    assert EfiMemoryDescriptor.attribute.offset == 32
    assert ctypes.sizeof(TaskStateSegment) == 104
    assert TaskStateSegment.interrupt_stack_table.offset == 36
    assert ctypes.sizeof(IdtEntry) == 16

    exit_boot_services_offset = 24 + (26 * 8)
    assert exit_boot_services_offset == 232

    print("SanjuOS M5 source checks passed.")
    print("UEFI memory descriptor base size: 40 bytes")
    print("x86-64 TSS size: 104 bytes")
    print("x86-64 IDT entry size: 16 bytes")
    print("ExitBootServices ABI offset: 232 bytes")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError) as exc:
        print(f"source check failed: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
