# Product Backlog

## Accepted

- M0 — repository, UEFI adapter, Rust workspace, CI, QEMU automation.
- M1 — memory map, `ExitBootServices`, kernel ownership, serial diagnostics.
- M2 — protected stack, GDT/TSS/IDT, exceptions, frame allocator, bootstrap heap.
- M3/M4 — PIC/PIT, timer and keyboard IRQs, scheduler foundation, shell, RAMFS.

## M5 — Protected User-Space Foundation — implementation ready for CI

- Virtual-memory layout and active CR3 capture.
- Four-level page-table policy with map/unmap and protection flags.
- Reclaim accounting, guard descriptors, and W^X validation.
- Reusable kernel heap.
- Ring 3 selectors, `IRETQ`, `SYSCALL`/`SYSRET`, and fault recovery.
- User pointer validation and eight-call syscall ABI.
- Process/address-space/context models and timer-quantum evidence.
- ELF64 PIE loader and three embedded user programs.
- Branded startup, error codes, SanjuOS ASCII output, and graphical logo asset.

## Foundation Hardening Phase 1 — accepted (`v0.0.6-fh1`)

- Pinned toolchain and generated capability truth registry.
- Versioned BootInfoV1 and architecture-boundary refactor.
- Physical ownership map, bitmap allocator with free, and page-table pool.

## Foundation Hardening Phase 2 — regression baseline

- Frozen x86-64 virtual-memory layout.
- Complete inherited page-table reservation.
- Fresh SanjuOS-owned PML4 and safe CR3 transition.
- Physical direct map and hardware map/translate/protect/unmap API.
- PE-section W^X permissions and real kernel guard holes.
- M5 and FH1 regression preservation.

## Foundation Hardening Phase 3 — accepted (`v0.0.8-fh3`)

- Private activated process CR3 roots.
- Hardware-unmapped lower and upper guards for user and Ring 0 stacks.
- Complete interrupt-frame register context switching.
- Timer-driven process preemption and CR3/TSS switching.
- Blocking, wakeup, and deterministic page-table resource reclamation.

## M6A — PCI and Storage Discovery — in development

- PCI enumeration and driver matching.
- Bounded multifunction and bridge-aware inventory.
- QEMU virtio-blk target discovery.
- Shell PCI diagnostics.

## M6B through M6D — Storage and VFS Foundation

- Block-device abstraction and polling virtio-blk transport.
- Buffer cache and asynchronous I/O contracts.
- VFS inode, mount, path, and file-handle model.
- Read-only FAT32 persistent filesystem prototype.
- User VFS handles and executable spawning after the kernel VFS gate.

## Later major epics

PCI and driver model; storage and persistent VFS; networking; USB; graphics and compositor; audio; power management; user SDK; packages, signed updates, recovery, installer, encrypted storage, supported-laptop enablement, and sandboxed AI services.
