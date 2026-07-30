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
- Branded startup, error codes, historical ASCII output, and graphical logo asset.

## Foundation Hardening Phase 1 — accepted (`v0.0.6-fh1`)

- Pinned toolchain and generated capability truth registry.
- Versioned BootInfoV1 and architecture-boundary refactor.
- Physical ownership map, bitmap allocator with free, and page-table pool.

## Foundation Hardening Phase 2 — regression baseline

- Frozen x86-64 virtual-memory layout.
- Complete inherited page-table reservation.
- Fresh OS-owned PML4 and safe CR3 transition.
- Physical direct map and hardware map/translate/protect/unmap API.
- PE-section W^X permissions and real kernel guard holes.
- M5 and FH1 regression preservation.

## Foundation Hardening Phase 3 — accepted (`v0.0.8-fh3`)

- Private activated process CR3 roots.
- Hardware-unmapped lower and upper guards for user and Ring 0 stacks.
- Complete interrupt-frame register context switching.
- Timer-driven process preemption and CR3/TSS switching.
- Blocking, wakeup, and deterministic page-table resource reclamation.

## M6A — PCI and Storage Discovery — accepted (`v0.0.9-m6a`)

- PCI enumeration and driver matching.
- Bounded multifunction and bridge-aware inventory.
- QEMU virtio-blk target discovery.
- Shell PCI diagnostics.

## M6B — Virtio Block Transport — accepted (`v0.0.10-m6b`)

- Block-device abstraction and polling virtio-blk transport.
- Dedicated disk identity, known-sector read, and confined write/restore gate.

## M6C — Bounded Cache and VFS — accepted (`v0.0.11-m6c`)

- Fixed-capacity read-through cache with a hard no-dirty-data policy.
- VFS inode, superblock, mount, canonical path, and file-handle contracts.
- RAMFS adapter and generation-protected bounded user handles.
- Live first-miss/repeat-hit hardware evidence and shell diagnostics.

## M6D — Read-Only Persistent Filesystem

- Validated read-only FAT32 on the dedicated virtio disk.
- Persistent root, long-name, nested, offset, and multi-cluster reads.
- Secondary VFS mount, shell access, and zero-dirty enforcement.

## M6E — Process-Facing Persistent Reads

- Extend the syscall/VFS boundary to persistent mount-aware file handles.
- Copy persistent file data safely into private user address spaces.
- Load signed/accepted ELF64 executables from read-only FAT32.
- Preserve process, path, handle, and address-space isolation.

## Later major epics

PCI and driver model; storage and persistent VFS; networking; USB; graphics and compositor; audio; power management; user SDK; packages, signed updates, recovery, installer, encrypted storage, supported-laptop enablement, and sandboxed AI services.
