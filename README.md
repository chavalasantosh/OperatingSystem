# SanjuOS

![SanjuOS logo](assets/branding/sanjuos-logo.png)

SanjuOS is an independent, Rust-first desktop operating-system project. It is
not a Linux distribution. Development proceeds through emulator-verified kernel
milestones before any physical-disk work.

## Current checkpoint: Foundation Hardening Phase 3

The accepted baseline entering this phase was the Foundation Hardening Phase 2
candidate. M0 through FH2 proved UEFI ownership transfer, protected kernel
execution, interrupts, Ring 3 entry, `SYSCALL`/`SYSRET`, ELF64 loading,
recoverable user faults, physical ownership, and a fresh SanjuOS page-table
root.

Phase 3 turns the process runtime into an active hardware boundary:

- each M5 process owns a deep-cloned four-level page-table root;
- inherited user permissions are stripped before explicit image and stack
  promotion;
- every user and Ring 0 process stack has lower and upper unmapped guard pages;
- timer interrupts save all general-purpose registers and the complete
  privilege-transition frame;
- the scheduler changes the saved frame, CR3, TSS `RSP0`, and syscall stack;
- two non-cooperative Ring 3 loops prove timer-driven forward progress;
- every private page-table frame is inventoried and returned after exit;
- the complete M5, FH1, and FH2 paths remain regression gates.

The authoritative maturity status is generated at
[`docs/CAPABILITY_MATRIX.md`](docs/CAPABILITY_MATRIX.md).

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

FH3 remains single-core and PIT-driven. The combined EFI-stub kernel still
retains a bounded identity mapping while a separate high-half kernel image is
designed. Per-process floating-point/SIMD state, SMP, local APIC timers, PCID,
copy-on-write, and demand paging remain future hardening work. PCI discovery,
storage drivers, a persistent VFS, and graphics are the next product gate.

## Safety

SanjuOS remains emulator-only. Do not install it on a physical disk.
