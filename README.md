# SanjuOS

![SanjuOS logo](assets/branding/sanjuos-logo.png)

SanjuOS is an independent, Rust-first desktop operating-system project. It is
not a Linux distribution. Development proceeds through emulator-verified kernel
milestones before any physical-disk work.

## Current checkpoint: Foundation Hardening Phase 2 candidate

The accepted rollback point is `v0.0.6-fh1`. M0 through M5 proved UEFI firmware
exit, protected kernel execution, hardware interrupts, Ring 3 entry,
`SYSCALL`/`SYSRET`, ELF64 loading, recoverable user faults, and the interactive
kernel shell.

Phase 2 takes page-table sovereignty away from the firmware:

- a frozen x86-64 virtual-address layout defines user, physical-direct-map,
  heap, stack, device, and temporary-mapping regions;
- every inherited page-table frame is discovered and reserved before general
  allocation begins;
- a fresh SanjuOS-owned PML4 and all intermediate tables are created only from
  the dedicated bootstrap pool;
- a bounded identity transition window and higher-half physical direct map are
  established explicitly;
- the loaded PE/COFF image is split to 4 KiB pages and protected according to
  section permissions, with supervisor write protection and NX enabled;
- hardware-backed map, translate, protect, and unmap operations are exercised
  after the CR3 transition;
- real unmapped kernel guard holes are verified around a mapped stack page;
- the complete M5 and FH1 QEMU paths remain regression gates.

The authoritative maturity status is generated at
[`docs/CAPABILITY_MATRIX.md`](docs/CAPABILITY_MATRIX.md). In particular, M5
private CR3 isolation, per-process user-stack guard holes, and full process
preemption remain models—not completed hardware guarantees.

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

Phase 2 owns the kernel page-table root, but the combined EFI-stub kernel still
retains a bounded identity mapping while a separate kernel image/relocation
contract is designed. User programs still share the kernel CR3: private process
address spaces, one Ring 0 stack per task, and full register-context preemption
remain the next hardening epoch. Storage and graphics stay blocked on that gate.

## Safety

SanjuOS remains emulator-only. Do not install it on a physical disk.
