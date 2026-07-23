#!/usr/bin/env python3
"""Dependency-free structural checks runnable before the Rust toolchain exists."""

from __future__ import annotations

import ctypes
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
BOOT = ROOT / "boot/uefi/src/main.rs"
KERNEL = ROOT / "kernel/src/lib.rs"
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


def main() -> int:
    boot = BOOT.read_text(encoding="utf-8")
    kernel = KERNEL.read_text(encoding="utf-8")
    smoke = SMOKE.read_text(encoding="utf-8")

    require(boot, 'extern "efiapi" fn efi_main', BOOT)
    require(boot, "get_memory_map", BOOT)
    require(boot, "exit_boot_services", BOOT)
    require(boot, "EXIT_BOOT_SERVICES_RETRIES", BOOT)
    require(boot, "MEMORY_MAP_CAPACITY", BOOT)
    require(boot, "addr_of_mut!(MEMORY_MAP_STORAGE)", BOOT)
    require(boot, "UEFI console and boot-services pointers are invalid", BOOT)
    require(kernel, "pub struct MemoryMapInfo", KERNEL)
    require(kernel, "Firmware boot services: exited", KERNEL)
    require(kernel, "Kernel ownership gate: passed", KERNEL)
    require(smoke, "Milestone M1: firmware exit and kernel ownership.", SMOKE)

    assert ctypes.sizeof(EfiTableHeader) == 24
    assert ctypes.sizeof(EfiMemoryDescriptor) == 40
    assert EfiMemoryDescriptor.physical_start.offset == 8
    assert EfiMemoryDescriptor.attribute.offset == 32

    # In the x86-64 UEFI ABI, ExitBootServices is the 27th pointer-sized
    # member after the 24-byte table header, yielding byte offset 232.
    exit_boot_services_offset = 24 + (26 * 8)
    assert exit_boot_services_offset == 232

    print("SanjuOS M1 source checks passed.")
    print("UEFI table header size: 24 bytes")
    print("UEFI memory descriptor base size: 40 bytes")
    print("ExitBootServices ABI offset: 232 bytes")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError) as exc:
        print(f"source check failed: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
