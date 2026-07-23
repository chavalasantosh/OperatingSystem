#!/usr/bin/env python3
"""Dependency-free structural checks runnable before the Rust toolchain exists."""

from __future__ import annotations

import ctypes
from pathlib import Path
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
SMOKE = ROOT / "scripts/smoke-test.sh"


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


def main() -> int:
    boot = BOOT.read_text(encoding="utf-8")
    cpu = CPU.read_text(encoding="utf-8")
    kernel = KERNEL.read_text(encoding="utf-8")
    memory = MEMORY.read_text(encoding="utf-8")
    input_code = INPUT.read_text(encoding="utf-8")
    scheduler = SCHEDULER.read_text(encoding="utf-8")
    shell = SHELL.read_text(encoding="utf-8")
    filesystem = FILESYSTEM.read_text(encoding="utf-8")
    smoke = SMOKE.read_text(encoding="utf-8")

    require(boot, 'extern "efiapi" fn efi_main', BOOT)
    require(boot, "get_memory_map", BOOT)
    require(boot, "exit_boot_services", BOOT)
    require(boot, "cpu::switch_to_kernel_stack", BOOT)
    require(boot, "FrameAllocator::from_memory_map", BOOT)
    require(boot, "BumpAllocator::new", BOOT)
    require(cpu, 'asm!("int3"', CPU)
    require(cpu, 'asm!("lidt', CPU)
    require(cpu, '"ltr ax"', CPU)
    require(cpu, "sanju_double_fault_stub", CPU)
    require(cpu, "sanju_page_fault_stub", CPU)
    require(cpu, "sanju_timer_interrupt_stub", CPU)
    require(cpu, "sanju_keyboard_interrupt_stub", CPU)
    require(cpu, "remap_and_unmask_pic", CPU)
    require(cpu, "configure_pit", CPU)
    require(memory, "pub struct FrameAllocator", MEMORY)
    require(memory, "EFI_CONVENTIONAL_MEMORY", MEMORY)
    require(memory, "pub struct BumpAllocator", MEMORY)
    require(input_code, "pub struct KeyboardDecoder", INPUT)
    require(scheduler, "pub struct Scheduler", SCHEDULER)
    require(shell, "pub struct Shell", SHELL)
    require(filesystem, "pub struct RamFs", FILESYSTEM)
    require(kernel, "pub struct M4Report", KERNEL)
    require(kernel, "M4 interactive runtime gate: passed", KERNEL)
    require(
        smoke,
        "Milestone M4: interrupt-driven runtime and interactive kernel environment.",
        SMOKE,
    )

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

    print("SanjuOS M4 source checks passed.")
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
