# ADR 0007: Interrupt Runtime, Kernel Scheduler, Shell, and RAMFS

## Status

Accepted for M4-alpha.

## Decision

Use the legacy 8259 PIC and PIT for the first x86-64 interrupt runtime because they are deterministic in QEMU and require no ACPI/APIC discovery. IRQ0 provides a 100 Hz time base. IRQ1 feeds a bounded single-producer/single-consumer scancode queue. The kernel uses a fixed-capacity round-robin work scheduler, an allocation-free shell, and a fixed-capacity RAM filesystem.

## Boundaries

This is a bootstrap runtime, not the final desktop architecture. Local APIC timers, SMP, register-context switching, production virtual memory, user processes, a VFS, and persistent storage remain later milestones.
