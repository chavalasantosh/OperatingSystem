# Product Backlog

## M0 — Boot Foundation — accepted

- Repository, Rust workspace, UEFI adapter, freestanding kernel boundary, CI, and QEMU automation.

## M1 — Firmware Exit and Kernel Ownership — accepted

- UEFI memory-map capture and validation.
- Retry-safe `ExitBootServices`.
- Owned boot-information ABI.
- Firmware-independent serial/debug logging.
- Deterministic QEMU smoke exit.

## M2 — Protected Core Kernel — implemented; CI acceptance pending

- M2-001 Dedicated kernel stack and one-way stack transition.
- M2-002 Long-mode GDT and TSS.
- M2-003 Double-fault IST stack.
- M2-004 IDT and breakpoint self-test.
- M2-005 General-protection and page-fault diagnostics.
- M2-006 Physical frame allocator over conventional memory.
- M2-007 Bootstrap bump heap.
- M2-008 Host tests and deterministic QEMU assertions.

## M3 — Interrupts, Scheduling, and Shell

- M3-001 Local APIC/PIT timer strategy.
- M3-002 Interrupt-controller initialization.
- M3-003 PS/2 keyboard input for QEMU.
- M3-004 Kernel task model and cooperative scheduler.
- M3-005 Interactive command shell.
- M3-006 QEMU input and scheduling integration tests.

## M4 — Virtual Memory and User Mode

- Page-table ownership and address-space manager.
- Guard pages and non-executable mappings.
- Production kernel heap.
- Ring-3 transition and syscall ABI.
- Process and capability object model.

## Later epics

Storage and VFS; PCI and device framework; networking; USB; graphics and compositor; audio; input; power management; user-space runtime; SDK; package/update/recovery; installer; encrypted storage; reference-laptop enablement; sandboxed AI services.
