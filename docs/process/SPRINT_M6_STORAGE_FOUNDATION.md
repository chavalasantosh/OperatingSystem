# M6 PCI, Storage, and VFS Foundation

## Goal

Reach a read-only persistent filesystem through four independently gated
hardware and kernel layers without exposing physical disks to experimental
writes.

## M6A — PCI discovery

Status: accepted as `v0.0.9-m6a`.

- PCI configuration-mechanism #1 presence probe.
- Bounded bus/device/function inventory.
- Multifunction and PCI bridge traversal.
- Architecture-independent identity and class decoding.
- Virtio block target matching.
- `pci` shell diagnostics.
- QEMU second-disk discovery gate.

Exit criteria:

- at least one PCI bus and function are discovered;
- the inventory does not overflow;
- a dedicated virtio-blk function is matched;
- FH3 and every earlier regression gate remains passed.

## M6B — block transport

Status: accepted as `v0.0.10-m6b`.

- [x] block-device trait and sector geometry contract;
- [x] PCI BAR and modern virtio capability parsing;
- [x] dedicated DMA-safe request, descriptor, available, and used rings;
- [x] polling virtio-blk initialization;
- [x] bounded single-sector read and write against a disposable test disk;
- [x] restore the original disposable sector after the write probe;
- [x] timeout, reset, unsupported-feature, bounds, and status error handling;
- [x] pass the pinned-toolchain headless QEMU smoke gate.

Exit criteria:

- a known sector pattern is read correctly;
- a write is confined to a disposable test sector and read back;
- the EFI system partition is never selected;
- failed operations return without corrupting kernel state.

## M6C — buffer cache and VFS

Status: implementation candidate; QEMU acceptance required.

- [x] 16-sector, allocation-free, read-through LRU block cache;
- [x] hard read-only dirty-state policy that rejects writes before transport;
- [x] failed reads preserve existing cache contents;
- [x] inode, superblock, mount, path, and file-handle types;
- [x] canonical absolute-path normalization with bounded root traversal;
- [x] fixed mount table with component-boundary longest-prefix resolution;
- [x] generation-protected, fixed-capacity user handle table;
- [x] RAMFS adapted behind the VFS contract;
- [x] live virtio first-miss/repeat-hit cache probe;
- [x] `cache` and `mounts` shell diagnostics;
- [ ] pass the pinned-toolchain headless QEMU smoke gate.

Exit criteria:

- one live sector read produces exactly one miss and one device request;
- repeating the read produces one hit without another device request;
- cached bytes equal the hardware-read bytes;
- an attempted cache write is rejected and leaves zero dirty entries;
- path normalization cannot escape root or exceed traversal bounds;
- RAMFS files resolve and read through VFS inode and handle contracts;
- a closed generation-tagged handle is rejected as stale;
- M6B and every earlier regression gate remains passed.

## M6D — read-only FAT32

- validate BPB geometry and FAT bounds;
- mount the dedicated second disk read-only;
- support root-directory listing and bounded file reads;
- reject malformed chains, loops, invalid clusters, and unsupported layouts;
- expose mounted files through `ls` and `cat`;
- prove a seeded file survives a fresh QEMU boot.

## Deferred beyond M6D

- writable FAT32;
- asynchronous block completion and MSI-X;
- AHCI and NVMe drivers;
- partition editing;
- journaling and power-loss recovery;
- encryption;
- physical-disk installation.
