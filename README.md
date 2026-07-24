# SanjuOS

![SanjuOS logo](assets/branding/sanjuos-logo.png)

SanjuOS is an independent, Rust-first desktop operating-system project. It is
not a Linux distribution. Development proceeds through emulator-verified kernel
milestones before any physical-disk work.

## Current checkpoint: Foundation Hardening Phase 1

The accepted rollback point is `v0.0.5-m5`. M0 through M5 proved UEFI firmware
exit, protected kernel execution, hardware interrupts, Ring 3 entry,
`SYSCALL`/`SYSRET`, ELF64 loading, recoverable user faults, and the interactive
kernel shell.

Phase 1 freezes the foundation before storage and graphics:

- Rust 1.97.0, rustfmt, Clippy, and the x86-64 UEFI target are pinned;
- one capability registry generates Rust data, documentation, and smoke-test
  expectations;
- x86-64 assembly, registers, interrupts, syscalls, and port I/O are isolated
  under `boot/uefi/src/arch/x86_64/`;
- `BootInfoV1` provides a versioned, reference-free firmware handoff with memory,
  loaded-image, ACPI, SMBIOS, active-CR3, and optional GOP framebuffer metadata;
- a physical ownership map reserves every known live physical range;
- a bitmap frame allocator supports allocation, contiguous allocation, release,
  reservations, exhaustion handling, and misuse detection;
- a dedicated 256-frame page-table bootstrap pool avoids allocator recursion;
- the complete M5 QEMU path remains a regression gate.

The authoritative maturity status is generated at
[`docs/CAPABILITY_MATRIX.md`](docs/CAPABILITY_MATRIX.md). In particular, M5
private CR3 isolation, hardware guard holes, and full process preemption remain
models—not completed hardware guarantees.

## Shell commands

```text
help version userspace uptime memory irq tasks ls cat write echo clear
```

## Build and verify

```bash
make setup
make user-programs
python3 scripts/generate-capabilities.py --check
make source-check
make fmt
make lint
make test
make smoke
```

## Repository map

```text
boot/uefi/          UEFI orchestration and x86-64 platform implementation
kernel/             Boot contracts, memory ownership, allocators, kernel models
capabilities/       Canonical capability registry and generated smoke evidence
user/programs/      Position-independent Ring 3 assembly programs
assets/branding/    Approved SanjuOS graphical logo
scripts/            Build, generation, ABI, QEMU, and validation automation
docs/               Requirements, architecture, ADRs, testing, security, process
```

## Current boundary

Phase 1 does not replace the inherited firmware page tables and does not reload
`CR3`. The next hardening phase constructs a fresh SanjuOS PML4, activates a real
hardware mapper, and creates genuinely unmapped guard pages. Real per-process
contexts and private address spaces follow only after that memory gate passes.

## Safety

SanjuOS remains emulator-only. Do not install it on a physical disk.
