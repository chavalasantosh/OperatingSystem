# Product Backlog

## Accepted

- M0 — repository, UEFI adapter, Rust workspace, CI, QEMU automation.
- M1 — memory map, `ExitBootServices`, kernel ownership, serial diagnostics.
- M2 — protected stack, GDT/TSS/IDT, exceptions, frame allocator, bootstrap heap.

## M3/M4 — Interactive Runtime — implementation ready for CI

- PIC remapping and interrupt policy.
- 100 Hz PIT timer and tick accounting.
- PS/2 keyboard IRQ and bounded scancode queue.
- Set-1 keyboard decoder.
- Fixed-capacity round-robin kernel scheduler.
- Allocation-free interactive shell.
- Writable RAM filesystem.
- QEMU timer, keyboard-vector, scheduler, shell, and filesystem acceptance flow.

## M5 — Virtual Memory and User Mode

- Own the active page tables.
- Map/unmap APIs, guard pages, NX policy, and address-space layout.
- Reusable kernel allocator.
- Ring-3 GDT entries and controlled user transition.
- Syscall ABI and dispatcher.
- Process object and first embedded user executable.

## Later major epics

VFS and persistent storage; PCI and driver model; networking; USB; graphics and compositor; audio; power management; user SDK; packages, signed updates, recovery, installer, encrypted storage, supported-laptop enablement, and sandboxed AI services.
