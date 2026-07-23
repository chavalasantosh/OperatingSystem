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

## M6 — Hardware-Owned Process Runtime

- Kernel relocation/high-half policy.
- Private activated CR3 roots and page-table cloning.
- Real unmapped guard pages.
- Per-process kernel stacks and complete register context switching.
- Timer-driven process preemption and blocking/wakeup.
- User VFS handles and executable spawning.

## Later major epics

PCI and driver model; storage and persistent VFS; networking; USB; graphics and compositor; audio; power management; user SDK; packages, signed updates, recovery, installer, encrypted storage, supported-laptop enablement, and sandboxed AI services.
