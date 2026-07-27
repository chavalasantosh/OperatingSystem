#!/usr/bin/env python3
"""Dependency-free structural checks runnable before the Rust toolchain exists."""

from __future__ import annotations

import ctypes
import hashlib
from pathlib import Path
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
BOOT = ROOT / "boot/uefi/src/main.rs"
CPU = ROOT / "boot/uefi/src/arch/x86_64/mod.rs"
HARDWARE_PAGING = ROOT / "boot/uefi/src/arch/x86_64/paging.rs"
HARDWARE_PCI = ROOT / "boot/uefi/src/arch/x86_64/pci.rs"
HARDWARE_VIRTIO_BLOCK = ROOT / "boot/uefi/src/arch/x86_64/virtio_block.rs"
KERNEL = ROOT / "kernel/src/lib.rs"
BOOT_INFO = ROOT / "kernel/src/boot_info.rs"
OWNERSHIP = ROOT / "kernel/src/ownership.rs"
CAPABILITY_REGISTRY = ROOT / "capabilities/capabilities.toml"
TOOLCHAIN = ROOT / "rust-toolchain.toml"
WORKSPACE = ROOT / "Cargo.toml"
MEMORY = ROOT / "kernel/src/memory.rs"
INPUT = ROOT / "kernel/src/input.rs"
SCHEDULER = ROOT / "kernel/src/scheduler.rs"
SHELL = ROOT / "kernel/src/shell.rs"
FILESYSTEM = ROOT / "kernel/src/fs.rs"
PAGING = ROOT / "kernel/src/paging.rs"
HEAP = ROOT / "kernel/src/heap.rs"
PROCESS = ROOT / "kernel/src/process.rs"
PCI = ROOT / "kernel/src/pci.rs"
BLOCK = ROOT / "kernel/src/block.rs"
SYSCALL = ROOT / "kernel/src/syscall.rs"
ELF = ROOT / "kernel/src/elf.rs"
STARTUP = ROOT / "kernel/src/startup.rs"
INIT_ELF = ROOT / "user/programs/bin/init.elf"
HELLO_ELF = ROOT / "user/programs/bin/hello.elf"
FAULT_ELF = ROOT / "user/programs/bin/fault-test.elf"
LOGO = ROOT / "assets/branding/sanjuos-logo.png"
SMOKE = ROOT / "scripts/smoke-test.sh"
USER_BUILD = ROOT / "scripts/build-user-programs.sh"
SETUP = ROOT / "scripts/setup.sh"
SOURCE_MANIFEST = ROOT / "SOURCE_MANIFEST.sha256"

MANIFEST_BINARY_SUFFIXES = {
    ".elf", ".png", ".jpg", ".jpeg", ".gif",
    ".bmp", ".ico", ".woff", ".woff2", ".ttf", ".otf",
}


def source_digest(path: Path) -> str:
    data = path.read_bytes()

    # Text files are hashed with canonical LF line endings so Windows and
    # Linux produce the same source-manifest digest.
    if path.suffix.lower() not in MANIFEST_BINARY_SUFFIXES:
        data = data.replace(b"\r\n", b"\n").replace(b"\r", b"\n")

    return hashlib.sha256(data).hexdigest()



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


class EfiGuid(ctypes.Structure):
    _fields_ = [
        ("data1", ctypes.c_uint32),
        ("data2", ctypes.c_uint16),
        ("data3", ctypes.c_uint16),
        ("data4", ctypes.c_uint8 * 8),
    ]


class EfiConfigurationTable(ctypes.Structure):
    _fields_ = [
        ("vendor_guid", EfiGuid),
        ("vendor_table", ctypes.c_void_p),
    ]


class EfiPixelBitmask(ctypes.Structure):
    _fields_ = [
        ("red_mask", ctypes.c_uint32),
        ("green_mask", ctypes.c_uint32),
        ("blue_mask", ctypes.c_uint32),
        ("reserved_mask", ctypes.c_uint32),
    ]


class EfiGraphicsOutputModeInformation(ctypes.Structure):
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("horizontal_resolution", ctypes.c_uint32),
        ("vertical_resolution", ctypes.c_uint32),
        ("pixel_format", ctypes.c_uint32),
        ("pixel_information", EfiPixelBitmask),
        ("pixels_per_scan_line", ctypes.c_uint32),
    ]


class EfiGraphicsOutputProtocolMode(ctypes.Structure):
    _fields_ = [
        ("max_mode", ctypes.c_uint32),
        ("mode", ctypes.c_uint32),
        ("info", ctypes.c_void_p),
        ("size_of_info", ctypes.c_size_t),
        ("frame_buffer_base", ctypes.c_uint64),
        ("frame_buffer_size", ctypes.c_size_t),
    ]


class FixedText16(ctypes.Structure):
    _fields_ = [
        ("length", ctypes.c_uint16),
        ("reserved", ctypes.c_uint16),
        ("bytes", ctypes.c_uint8 * 16),
    ]


class FixedText128(ctypes.Structure):
    _fields_ = [
        ("length", ctypes.c_uint16),
        ("reserved", ctypes.c_uint16),
        ("bytes", ctypes.c_uint8 * 128),
    ]


class FixedText256(ctypes.Structure):
    _fields_ = [
        ("length", ctypes.c_uint16),
        ("reserved", ctypes.c_uint16),
        ("bytes", ctypes.c_uint8 * 256),
    ]


class MemoryMapInfoV1(ctypes.Structure):
    _fields_ = [
        ("buffer_address", ctypes.c_uint64),
        ("buffer_capacity", ctypes.c_uint64),
        ("map_size", ctypes.c_uint64),
        ("map_key", ctypes.c_uint64),
        ("descriptor_size", ctypes.c_uint64),
        ("descriptor_version", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32),
        ("descriptor_count", ctypes.c_uint64),
    ]


class PhysicalRangeV1(ctypes.Structure):
    _fields_ = [("start", ctypes.c_uint64), ("length", ctypes.c_uint64)]


class FramebufferInfoV1(ctypes.Structure):
    _fields_ = [
        ("present", ctypes.c_uint8),
        ("reserved", ctypes.c_uint8 * 7),
        ("physical_start", ctypes.c_uint64),
        ("byte_length", ctypes.c_uint64),
        ("width", ctypes.c_uint32),
        ("height", ctypes.c_uint32),
        ("stride", ctypes.c_uint32),
        ("pixel_format", ctypes.c_uint32),
        ("red_mask", ctypes.c_uint32),
        ("green_mask", ctypes.c_uint32),
        ("blue_mask", ctypes.c_uint32),
        ("reserved_mask", ctypes.c_uint32),
    ]


class OptionalPhysicalAddressV1(ctypes.Structure):
    _fields_ = [
        ("present", ctypes.c_uint8),
        ("reserved", ctypes.c_uint8 * 7),
        ("address", ctypes.c_uint64),
    ]


class BootInfoV1Layout(ctypes.Structure):
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("size", ctypes.c_uint32),
        ("architecture", FixedText16),
        ("firmware", FixedText16),
        ("milestone", FixedText128),
        ("memory_map", MemoryMapInfoV1),
        ("kernel_image", PhysicalRangeV1),
        ("boot_image", PhysicalRangeV1),
        ("boot_info_range", PhysicalRangeV1),
        ("framebuffer", FramebufferInfoV1),
        ("acpi_rsdp", OptionalPhysicalAddressV1),
        ("smbios_entry", OptionalPhysicalAddressV1),
        ("initrd", PhysicalRangeV1),
        ("command_line", FixedText256),
        ("active_page_table_root", ctypes.c_uint64),
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


class InterruptFrame(ctypes.Structure):
    _fields_ = [
        ("r15", ctypes.c_uint64),
        ("r14", ctypes.c_uint64),
        ("r13", ctypes.c_uint64),
        ("r12", ctypes.c_uint64),
        ("r11", ctypes.c_uint64),
        ("r10", ctypes.c_uint64),
        ("r9", ctypes.c_uint64),
        ("r8", ctypes.c_uint64),
        ("rdi", ctypes.c_uint64),
        ("rsi", ctypes.c_uint64),
        ("rbp", ctypes.c_uint64),
        ("rbx", ctypes.c_uint64),
        ("rdx", ctypes.c_uint64),
        ("rcx", ctypes.c_uint64),
        ("rax", ctypes.c_uint64),
        ("rip", ctypes.c_uint64),
        ("cs", ctypes.c_uint64),
        ("rflags", ctypes.c_uint64),
        ("rsp", ctypes.c_uint64),
        ("ss", ctypes.c_uint64),
    ]


def validate_source_manifest() -> None:
    expected: dict[Path, str] = {}
    for line in SOURCE_MANIFEST.read_text(encoding="utf-8").splitlines():
        digest, relative = line.split("  ", 1)
        path = (ROOT / relative.removeprefix("./")).resolve()
        expected[path] = digest

    actual_files = {
        path.resolve()
        for path in ROOT.rglob("*")
        if path.is_file()
        and ".git" not in path.parts
        and "target" not in path.parts
        and "build" not in path.parts
        and "__pycache__" not in path.parts
        and path.name != SOURCE_MANIFEST.name
        and path.suffix not in {".patch", ".zip", ".pyc"}
    }
    if set(expected) != actual_files:
        missing = sorted(str(path.relative_to(ROOT)) for path in actual_files - set(expected))
        stale = sorted(str(path.relative_to(ROOT)) for path in set(expected) - actual_files)
        raise AssertionError(
            f"source manifest file set differs; missing={missing}, stale={stale}"
        )

    for path, expected_digest in expected.items():
        actual_digest = source_digest(path)
        if actual_digest != expected_digest:
            raise AssertionError(f"source manifest digest mismatch: {path.relative_to(ROOT)}")


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
    hardware_paging = HARDWARE_PAGING.read_text(encoding="utf-8")
    hardware_pci = HARDWARE_PCI.read_text(encoding="utf-8")
    hardware_virtio_block = HARDWARE_VIRTIO_BLOCK.read_text(encoding="utf-8")
    kernel = KERNEL.read_text(encoding="utf-8")
    boot_info = BOOT_INFO.read_text(encoding="utf-8")
    ownership = OWNERSHIP.read_text(encoding="utf-8")
    capability_registry = CAPABILITY_REGISTRY.read_text(encoding="utf-8")
    toolchain = TOOLCHAIN.read_text(encoding="utf-8")
    workspace = WORKSPACE.read_text(encoding="utf-8")
    memory = MEMORY.read_text(encoding="utf-8")
    input_code = INPUT.read_text(encoding="utf-8")
    scheduler = SCHEDULER.read_text(encoding="utf-8")
    shell = SHELL.read_text(encoding="utf-8")
    filesystem = FILESYSTEM.read_text(encoding="utf-8")
    paging = PAGING.read_text(encoding="utf-8")
    heap = HEAP.read_text(encoding="utf-8")
    process = PROCESS.read_text(encoding="utf-8")
    pci = PCI.read_text(encoding="utf-8")
    block = BLOCK.read_text(encoding="utf-8")
    syscall = SYSCALL.read_text(encoding="utf-8")
    elf = ELF.read_text(encoding="utf-8")
    startup = STARTUP.read_text(encoding="utf-8")
    smoke = SMOKE.read_text(encoding="utf-8")
    user_build = USER_BUILD.read_text(encoding="utf-8")
    setup = SETUP.read_text(encoding="utf-8")

    require(boot, 'extern "efiapi" fn efi_main', BOOT)
    require(boot, "get_memory_map", BOOT)
    require(boot, "exit_boot_services", BOOT)
    require(boot, "cpu::switch_to_kernel_stack", BOOT)
    require(boot, "FrameAllocator::from_memory_map", BOOT)
    require(boot, "PhysicalOwnershipMap::from_boot_info", BOOT)
    require(boot, "PageTableBootstrapPool", BOOT)
    require(boot, "PagingError::WriteExecuteViolation", BOOT)
    require(boot, "EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID", BOOT)
    require(boot, "fn framebuffer_info", BOOT)
    require(boot, "KernelHeap::new", BOOT)
    require(boot, "load_position_independent", BOOT)
    require(boot, "cpu::run_user_process", BOOT)
    require(boot, "create_private_address_space", BOOT)
    require(boot, "cpu::run_preemption_probe", BOOT)
    require(boot, "reclaim_private_address_space", BOOT)
    require(boot, "FoundationHardeningPhase3Report", BOOT)
    require(boot, "cpu::discover_pci", BOOT)
    require(boot, "M6aReport", BOOT)
    require(boot, "kernel_main_m6a", BOOT)
    require(boot, "M6bReport", BOOT)
    require(boot, "kernel_main_m6b", BOOT)
    require(boot, "cpu::initialize_virtio_block", BOOT)
    if "core::arch" in boot or "asm!(" in boot or "global_asm!(" in boot:
        raise AssertionError(f"{BOOT}: architecture-specific assembly leaked into main.rs")
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
    require(cpu, "struct InterruptFrame", CPU)
    require(cpu, "fh3_schedule_from_timer", CPU)
    require(cpu, "run_preemption_probe", CPU)
    require(cpu, "set_process_kernel_stack", CPU)
    require(cpu, "switch_address_space", CPU)
    require(cpu, "SANJU_FH3_COUNTER_A", CPU)
    require(hardware_paging, "pub struct HardwarePageTable", HARDWARE_PAGING)
    require(hardware_paging, "pub struct PrivateAddressSpace", HARDWARE_PAGING)
    require(hardware_paging, "take_page_table_ownership", HARDWARE_PAGING)
    require(hardware_paging, "reserve_inherited_page_tables", HARDWARE_PAGING)
    require(hardware_paging, "map_huge_2m", HARDWARE_PAGING)
    require(hardware_paging, "split_huge_2m", HARDWARE_PAGING)
    require(hardware_paging, "protect_page", HARDWARE_PAGING)
    require(hardware_paging, "write_cr3", HARDWARE_PAGING)
    require(hardware_paging, "PHYSICAL_DIRECT_MAP_START", HARDWARE_PAGING)
    require(hardware_paging, "IMAGE_SCN_MEM_EXECUTE", HARDWARE_PAGING)
    require(hardware_paging, "enable_execute_disable", HARDWARE_PAGING)
    require(hardware_paging, "enable_kernel_write_protect", HARDWARE_PAGING)
    require(hardware_paging, "switch_cr3_and_flush_global", HARDWARE_PAGING)
    require(hardware_paging, "create_private_address_space", HARDWARE_PAGING)
    require(hardware_paging, "clone_private_table", HARDWARE_PAGING)
    require(hardware_paging, "reclaim_private_address_space", HARDWARE_PAGING)
    require(hardware_paging, "mark_user_range_in_root", HARDWARE_PAGING)
    require(hardware_paging, "clear_leaf_in_root", HARDWARE_PAGING)
    require(hardware_paging, "update_direct_alias_permissions", HARDWARE_PAGING)
    require(hardware_pci, "pub unsafe fn discover_pci", HARDWARE_PCI)
    require(hardware_pci, "CONFIG_ADDRESS_PORT", HARDWARE_PCI)
    require(hardware_pci, "configuration_mechanism_one_available", HARDWARE_PCI)
    require(hardware_pci, "read_config_u32", HARDWARE_PCI)
    require(hardware_pci, "pub(super) struct PciConfigSession", HARDWARE_PCI)
    require(hardware_virtio_block, "VIRTIO_F_VERSION_1_HIGH", HARDWARE_VIRTIO_BLOCK)
    require(hardware_virtio_block, "VIRTIO_PCI_CAP_PCI_CFG", HARDWARE_VIRTIO_BLOCK)
    require(hardware_virtio_block, "struct DmaQueue", HARDWARE_VIRTIO_BLOCK)
    require(hardware_virtio_block, "VIRTIO_BLK_T_GET_ID", HARDWARE_VIRTIO_BLOCK)
    require(hardware_virtio_block, "EXPECTED_READ_PATTERN", HARDWARE_VIRTIO_BLOCK)
    require(hardware_virtio_block, "DISPOSABLE_WRITE_SECTOR", HARDWARE_VIRTIO_BLOCK)
    require(hardware_virtio_block, "validate_sector_range", HARDWARE_VIRTIO_BLOCK)
    require(hardware_virtio_block, "BlockError::Timeout", HARDWARE_VIRTIO_BLOCK)
    require(memory, "pub struct FrameAllocator", MEMORY)
    require(memory, "pub struct FrameBitmap", MEMORY)
    require(memory, "pub fn free_frame", MEMORY)
    require(memory, "pub struct PageTableBootstrapPool", MEMORY)
    require(memory, "EFI_CONVENTIONAL_MEMORY", MEMORY)
    require(memory, "pub struct BumpAllocator", MEMORY)
    for test_name in (
        "allocates_unique_frames",
        "reuses_freed_frames",
        "rejects_double_free",
        "rejects_reserved_frame_free",
        "reserves_unaligned_ranges_correctly",
        "handles_allocator_exhaustion",
        "bootstrap_pool_does_not_use_heap",
    ):
        require(memory, f"fn {test_name}", MEMORY)
    require(input_code, "pub struct KeyboardDecoder", INPUT)
    require(scheduler, "pub struct Scheduler", SCHEDULER)
    require(shell, "pub struct Shell", SHELL)
    require(filesystem, "pub struct RamFs", FILESYSTEM)
    require(paging, "pub struct PageTableManager", PAGING)
    require(paging, "WriteExecuteViolation", PAGING)
    require(heap, "pub struct KernelHeap", HEAP)
    require(process, "pub struct ProcessControlBlock", PROCESS)
    require(process, "pub struct ProcessTable", PROCESS)
    require(process, "pub fn block", PROCESS)
    require(process, "pub fn wake", PROCESS)
    require(process, "pub fn reap", PROCESS)
    require(pci, "pub struct PciInventory", PCI)
    require(pci, "pub struct PciDevice", PCI)
    require(pci, "StorageControllerKind::VirtioBlock", PCI)
    require(pci, "pub fn decode_bar", PCI)
    if "asm!(" in pci or "0x0cf8" in pci or "0x0cfc" in pci:
        raise AssertionError(f"{PCI}: architecture-specific PCI access leaked into kernel")
    require(block, "pub trait BlockDevice", BLOCK)
    require(block, "pub struct BlockGeometry", BLOCK)
    require(block, "pub const fn validate_sector_range", BLOCK)
    require(syscall, "pub struct SyscallDispatcher", SYSCALL)
    for syscall_name in ("Write", "Read", "Exit", "Yield", "GetPid", "Open", "Close", "Spawn"):
        require(syscall, syscall_name, SYSCALL)
    require(elf, "load_position_independent", ELF)
    require(startup, "STARTUP_LOGO", STARTUP)
    require(startup, "Secure. Fast. Yours.", STARTUP)
    require(boot_info, "pub struct BootInfoV1", BOOT_INFO)
    require(boot_info, "pub struct FramebufferInfo", BOOT_INFO)
    require(boot_info, "pub active_page_table_root: u64", BOOT_INFO)
    require(boot_info, "fn boot_info_v1_layout_is_frozen", BOOT_INFO)
    require(boot_info, "#[repr(C)]", BOOT_INFO)
    require(ownership, "pub struct PhysicalOwnershipMap", OWNERSHIP)
    require(ownership, "OwnershipError::Overlap", OWNERSHIP)
    for test_name in (
        "detects_overlapping_ranges",
        "preserves_kernel_image",
        "preserves_active_page_tables",
        "preserves_framebuffer",
    ):
        require(ownership, f"fn {test_name}", OWNERSHIP)
    require(capability_registry, "SYS-TC-001", CAPABILITY_REGISTRY)
    require(capability_registry, 'registry_version = 5', CAPABILITY_REGISTRY)
    require(capability_registry, "PCI-ENUM-001", CAPABILITY_REGISTRY)
    require(capability_registry, "STOR-DISC-001", CAPABILITY_REGISTRY)
    require(capability_registry, "STOR-BLK-001", CAPABILITY_REGISTRY)
    require(capability_registry, "STOR-VIRTIO-001", CAPABILITY_REGISTRY)
    require(capability_registry, "software_model", CAPABILITY_REGISTRY)
    require(toolchain, 'channel = "1.97.0"', TOOLCHAIN)
    require(toolchain, 'components = ["clippy", "rustfmt"]', TOOLCHAIN)
    require(toolchain, 'targets = ["x86_64-unknown-uefi"]', TOOLCHAIN)
    require(workspace, 'rust-version = "1.97.0"', WORKSPACE)
    require(kernel, "pub struct M5Report", KERNEL)
    require(kernel, "pub struct FoundationHardeningReport", KERNEL)
    require(kernel, "pub struct FoundationHardeningPhase3Report", KERNEL)
    require(kernel, "pub struct M6aReport", KERNEL)
    require(kernel, "pub struct M6bReport", KERNEL)
    require(kernel, "Foundation hardening phase 1: passed", KERNEL)
    require(kernel, "Foundation hardening phase 3: passed", KERNEL)
    require(kernel, "M6A PCI discovery gate: passed", KERNEL)
    require(kernel, "M6B block transport gate: passed", KERNEL)
    require(kernel, "Ring 3 execution", KERNEL)
    require(kernel, "M5 protected user-space gate: passed", KERNEL)
    require(
        smoke,
        "Milestone M5: protected user-space foundation and branded startup.",
        SMOKE,
    )
    require(user_build, "-z noseparate-code", USER_BUILD)
    require(setup, "rustup toolchain install 1.97.0", SETUP)
    require(setup, "rustup override set 1.97.0", SETUP)
    require(smoke, "virtio-blk-pci", SMOKE)
    require(smoke, "M6A PCI discovery gate: passed", SMOKE)
    require(smoke, "SANJUOS-M6B-READ-PATTERN", SMOKE)
    require(smoke, "serial=SANJU-M6B", SMOKE)
    require(smoke, "M6B block transport gate: passed", SMOKE)
    require(smoke, "Disposable sector restoration: passed", SMOKE)
    for executable in (INIT_ELF, HELLO_ELF, FAULT_ELF):
        validate_elf64(executable)
    if LOGO.read_bytes()[:8] != b"\x89PNG\r\n\x1a\n":
        raise AssertionError(f"{LOGO}: missing PNG signature")

    assert ctypes.sizeof(EfiTableHeader) == 24
    assert ctypes.sizeof(EfiMemoryDescriptor) == 40
    assert ctypes.sizeof(EfiGuid) == 16
    assert ctypes.sizeof(EfiConfigurationTable) == 24
    assert ctypes.sizeof(EfiGraphicsOutputModeInformation) == 36
    assert ctypes.sizeof(EfiGraphicsOutputProtocolMode) == 40
    assert EfiGraphicsOutputProtocolMode.frame_buffer_base.offset == 24
    assert ctypes.sizeof(MemoryMapInfoV1) == 56
    assert ctypes.sizeof(FramebufferInfoV1) == 56
    assert ctypes.sizeof(BootInfoV1Layout) == 664
    assert BootInfoV1Layout.memory_map.offset == 184
    assert BootInfoV1Layout.active_page_table_root.offset == 656
    assert EfiMemoryDescriptor.physical_start.offset == 8
    assert EfiMemoryDescriptor.number_of_pages.offset == 24
    assert EfiMemoryDescriptor.attribute.offset == 32
    assert ctypes.sizeof(TaskStateSegment) == 104
    assert TaskStateSegment.interrupt_stack_table.offset == 36
    assert ctypes.sizeof(IdtEntry) == 16
    assert ctypes.sizeof(InterruptFrame) == 160
    assert InterruptFrame.rip.offset == 120
    assert InterruptFrame.cs.offset == 128
    assert InterruptFrame.rsp.offset == 144

    exit_boot_services_offset = 24 + (26 * 8)
    assert exit_boot_services_offset == 232

    subprocess.run(
        [sys.executable, str(ROOT / "scripts/generate-capabilities.py"), "--check"],
        cwd=ROOT,
        check=True,
    )
    if (ROOT / ".git").exists():
        tracked_output = subprocess.run(
            ["git", "ls-files", "*.patch", "*.zip"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.splitlines()
        tracked_artifacts = [
            relative for relative in tracked_output if (ROOT / relative).exists()
        ]
        if tracked_artifacts:
            raise AssertionError(
                f"delivery artifacts must not be tracked: {tracked_artifacts}"
            )
    validate_source_manifest()

    print("SanjuOS M6B virtio-block transport source checks passed.")
    print("UEFI memory descriptor base size: 40 bytes")
    print("UEFI GOP mode-information size: 36 bytes")
    print("UEFI GOP mode size: 40 bytes")
    print("BootInfo v1 ABI size: 664 bytes")
    print("x86-64 TSS size: 104 bytes")
    print("x86-64 IDT entry size: 16 bytes")
    print("x86-64 complete interrupt-frame size: 160 bytes")
    print("ExitBootServices ABI offset: 232 bytes")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError) as exc:
        print(f"source check failed: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
