# System Architecture Overview

## Initial architecture

```text
UEFI firmware
    |
    v
SanjuOS UEFI boot layer
    - validates firmware tables
    - captures memory and platform information (M1)
    - loads kernel image (later)
    - exits firmware boot services (M1)
    |
    v
Freestanding Rust kernel
    - architecture boundary
    - memory manager
    - interrupts and timers
    - scheduler and processes
    - IPC and object model
    - device and filesystem frameworks
    |
    v
Privileged system services
    - device policy
    - networking
    - storage coordination
    - package/update service
    |
    v
Sandboxed user space
    - compositor and desktop shell
    - applications
    - AI services
```

## Kernel direction

The initial kernel is a modular monolithic kernel: essential mechanisms execute in one protected kernel address space, while subsystems use explicit interfaces and ownership boundaries. This avoids premature microkernel complexity while preserving a migration path for selected services to user space.

## Trust boundaries

1. Firmware is trusted only long enough to obtain required boot services and platform data.
2. The boot layer validates pointers and table signatures before use.
3. The kernel owns memory and interrupt policy after `ExitBootServices`.
4. Drivers are privileged initially, then candidates for isolation according to risk and performance.
5. AI models and agents remain unprivileged and cannot directly access devices or kernel memory.

## Portability

- First architecture: x86-64.
- First firmware: UEFI.
- Second architecture candidate: AArch64 after the x86-64 platform abstraction is stable.
- First physical target: one laptop selected after an inventory and documentation review.

## M5 protected-user execution path

```text
UEFI -> retained memory map -> ExitBootServices -> kernel stack
     -> GDT/TSS/IDT/PIC/PIT -> CR3 capture -> syscall MSRs
     -> ELF64 loader -> Ring 3 IRETQ -> syscalls / user faults
     -> kernel acceptance report -> interactive shell
```

The M5 platform path runs one user program at a time on the bootstrap CPU. The architecture-independent process and paging objects define the intended long-term contract.

## Foundation hardening boundary

```text
UEFI system table and loaded-image protocol
    -> BootInfoV1
       - retained UEFI memory map
       - loaded EFI image range
       - active CR3 root
       - ACPI and SMBIOS entry addresses
       - optional GOP framebuffer metadata
    -> physical ownership map
    -> bitmap frame allocator
    -> reserved page-table bootstrap pool
    -> unchanged M5 execution regression
```

Phase 1 deliberately does not reload `CR3`. The next memory phase builds a fresh
SanjuOS PML4, maps every live kernel and platform range explicitly, validates the
new hierarchy, and only then retires firmware-derived mappings. Full process
preemption and private address spaces remain blocked on that gate.
