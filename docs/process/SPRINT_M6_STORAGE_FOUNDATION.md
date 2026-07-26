# M6 PCI, Storage, and VFS Foundation

## Goal

Reach a read-only persistent filesystem through four independently gated
hardware and kernel layers without exposing physical disks to experimental
writes.

## M6A — PCI discovery

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

- block-device trait and sector geometry contract;
- PCI BAR and virtio capability parsing;
- dedicated DMA-safe request, descriptor, available, and used rings;
- polling virtio-blk initialization;
- bounded single-sector read and write against a disposable test disk;
- timeout, unsupported-feature, bounds, and status error handling.

Exit criteria:

- a known sector pattern is read correctly;
- a write is confined to a disposable test sector and read back;
- the EFI system partition is never selected;
- failed operations return without corrupting kernel state.

## M6C — buffer cache and VFS

- fixed-capacity block cache with explicit dirty-state policy;
- inode, superblock, mount, path, and file-handle types;
- absolute-path normalization and traversal bounds;
- RAMFS adapted behind the VFS contract;
- user handle table design without enabling persistent writes.

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
