# Soma OS

Soma OS is the current working identity of an independent, Rust-first desktop
operating-system project. Historical release tags and internal `sanju-*` crate
identifiers remain unchanged until the public product identity is frozen. It is
not a Linux distribution. Development proceeds through emulator-verified kernel
milestones before any physical-disk work.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/chavalasantosh/OperatingSystem)

## Current checkpoint: M6C Bounded Cache and VFS

The accepted baseline entering this phase is `v0.0.10-m6b`. M0 through FH2
proved UEFI ownership transfer, protected kernel
execution, interrupts, Ring 3 entry, `SYSCALL`/`SYSRET`, ELF64 loading,
recoverable user faults, physical ownership, and a fresh Soma OS page-table
root.

The accepted `v0.0.8-fh3` release turned the process runtime into an active
hardware boundary:

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

The accepted `v0.0.9-m6a` release begins the storage stack without issuing
disk I/O:

- x86 PCI configuration mechanism #1 is probed directly;
- bus/device/function discovery handles multifunction devices and bridges;
- an allocation-free kernel inventory classifies storage controllers;
- QEMU attaches a disposable second disk through `virtio-blk-pci`;
- boot and shell evidence must identify that exact block target;
- sector I/O remains a separate acceptance gate.

M6B implements that gate on the dedicated QEMU test disk:

- an architecture-independent, sector-based block-device contract;
- validated PCI BARs and modern virtio PCI capabilities;
- `VIRTIO_F_VERSION_1` negotiation with unsupported read-only media rejected;
- one allocator-owned, direct-mapped DMA page containing a split virtqueue;
- bounded polling with reset, status, and timeout failures returned safely;
- a seeded read test plus a disposable write/readback/restore transaction;
- a `block` shell diagnostic and exact QEMU acceptance evidence.

M6C builds the filesystem boundary without enabling persistent writes:

- a 16-sector, allocation-free, read-through LRU cache;
- one live first-miss/repeat-hit probe proving the second read avoids transport;
- a hard read-only cache policy with zero dirty entries;
- fixed inode, superblock, mount, canonical path, and directory contracts;
- component-boundary mount resolution and bounded `.`/`..` normalization;
- generation-protected user file handles that reject stale identifiers;
- RAMFS accessed through the same VFS interface reserved for FAT32;
- `cache` and `mounts` shell diagnostics plus exact smoke evidence.

## Shell commands

```text
help version userspace uptime memory irq tasks pci block cache mounts ls cat write echo clear
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
assets/branding/    Historical and future product identity assets
scripts/            Build, generation, ABI, QEMU, and validation automation
docs/               Requirements, architecture, ADRs, testing, security, process
```

## Current boundary

M6C and FH3 remain single-core and PIT-driven. Block completion is synchronous
and polling, one request is outstanding at a time, and only the explicitly
identified disposable QEMU disk is used. The cache cannot issue writes and
cannot contain dirty entries. RAMFS remains the only writable filesystem. The
combined EFI-stub kernel still retains a bounded identity mapping while a
separate high-half kernel image is designed. Read-only FAT32, persistent
writes, physical-disk installation, graphics, SMP, and local APIC timers remain
later gates.

## Safety

Soma OS remains emulator-only. Do not install it on a physical disk.
