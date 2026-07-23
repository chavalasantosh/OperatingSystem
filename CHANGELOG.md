# Changelog

## M2-alpha implementation — 2026-07-24

### Added

- Dedicated 64 KiB kernel stack and one-way post-firmware stack transition.
- x86-64 GDT, TSS, ring-0 stack, and double-fault IST stack.
- IDT handlers for breakpoint, double fault, general protection, and page fault.
- Recoverable breakpoint exception self-test and fatal CR2 diagnostics.
- Physical frame allocator restricted to UEFI conventional memory.
- 256 KiB allocation-only bootstrap heap.
- M2 host tests, ABI checks, timeout-protected QEMU gate, ADR, and Sprint 2 plan.

### Verification status

M1 is QEMU-verified. M2 source checks pass locally; Rust formatting, Clippy, unit tests, UEFI build, and QEMU execution must pass in CI before M2 is accepted.

### Safety status

Emulator-only. Physical installation remains unsupported.

## M1-alpha checkpoint — 2026-07-21

### Added

- x86-64 UEFI boot-services ABI through `ExitBootServices`.
- Aligned, retained 256 KiB firmware memory-map storage.
- Memory-map metadata validation and bounded map-key retry logic.
- Early 16550/COM1 serial diagnostics independent of UEFI console services.
- Owned `BootInfo` and `MemoryMapInfo` kernel handoff.
- Kernel tests for memory-map invariants and allocation-free integer output.
- Dependency-free source and UEFI ABI verification script.
- Sprint 1 plan and firmware-exit ADR.

### Verification status

Source and shell checks pass. Rust compilation, linting, and QEMU/OVMF execution remain pending because the current workspace cannot install the required toolchain.

### Safety status

Emulator-only. Physical installation remains intentionally unsupported.

## 0.0.1-prealpha / M0 scaffold — 2026-07-21

### Added

- Sprint 0 project charter, requirements, architecture, security, testing, SDLC, backlog, definition of done, and risk register.
- Dependency-free Rust UEFI boot layer for x86-64.
- Freestanding architecture-independent kernel core.
- Deterministic M0 boot banner.
- Host unit tests and QEMU/OVMF smoke-test automation.
- CI workflow and coding standards.

## M1 verification checkpoint

- Added a dependency-free LLVM/LLD UEFI verification probe.
- Produced a real x86-64 PE32+ `BOOTX64.EFI` artifact.
- Verified EFI application subsystem, entry point, embedded boot messages, and checksum.
- Kept the probe separate from the Rust-first product implementation.
- QEMU execution remains blocked in the current restricted container because QEMU/OVMF cannot be installed.
