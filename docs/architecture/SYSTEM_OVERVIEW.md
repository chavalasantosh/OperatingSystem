# System Architecture Overview

## Initial architecture

```text
UEFI firmware
    |
    v
Soma OS UEFI boot layer
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

Phase 2 constructs a fresh PML4 from the dedicated bootstrap pool, reserves the
complete inherited hierarchy, maps a bounded identity transition window plus a
higher-half physical direct map, applies PE-section W^X permissions, and reloads
`CR3` only after validation. Hardware map/translate/protect/unmap probes and real
unmapped kernel guard holes run before interrupts and M5 user space are restored.

The kernel remains a combined EFI-stub image during this epoch, so the identity
transition window is intentional rather than an accidental firmware dependency.

## Foundation Hardening Phase 3 process runtime

```text
Soma OS kernel CR3
    -> sanitized private root per process
       -> explicit Ring 3 image/data/stack pages
       -> supervisor kernel/direct-map pages
       -> unmapped user and Ring 0 stack guards
    -> PIT interrupt
       -> 15 registers + RIP/CS/RFLAGS/RSP/SS
       -> round-robin saved-frame selection
       -> CR3 + TSS RSP0 + syscall-stack switch
       -> IRETQ into the selected Ring 3 process
```

Three M5 ELF programs execute sequentially under distinct private roots for
regression coverage. A separate two-process probe runs non-cooperative Ring 3
loops and can return to the kernel only through timer-driven complete-frame
switching. Inactive private hierarchies are reclaimed from an exact frame
inventory.

The scheduler remains single-core and PIT-driven. SMP, local APIC timers, PCID,
copy-on-write, demand paging, and a separately linked high-half kernel image are
future architecture gates.

## M6 storage layering

```text
x86 PCI configuration mechanism #1
    -> bounded PCI inventory and storage matching (M6A)
    -> virtio-blk transport and block-device API (M6B)
    -> bounded buffer cache and VFS objects (M6C)
    -> read-only FAT32 on a dedicated second disk (M6D)
```

Raw configuration and future device-register access stay in the x86-64
adapter. PCI identity, block contracts, cache policy, VFS types, and filesystem
validation remain architecture-independent. The EFI system partition is not a
storage-development target.

M6B selects the single discovered virtio-blk function, validates its BAR and
vendor-capability topology, and uses the standardized PCI configuration-access
window for modern common, notify, and device registers. One direct-mapped
allocator frame contains the descriptor table, available ring, used ring,
request header, sector buffer, and status byte. The transport permits one
bounded synchronous request at a time; M6C may consume only the block-device
contract, not the x86 PCI adapter.

M6C wraps that contract in a 16-entry read-through LRU cache. Its only
dirty-state policy rejects writes before transport, so no entry can become
dirty. A failed read is staged separately and cannot destroy an existing cache
entry.

The VFS layer defines fixed inode, superblock, mount, path, directory, and
generation-tagged handle contracts. Paths are absolute and canonical with
bounded depth; mount selection observes component boundaries. RAMFS remains the
writable root.

M6D mounts one allocation-free FAT32 backend at `/disk`. It consumes the cached
block-device contract, validates the complete volume geometry before trusting
offsets, and bounds every FAT and directory traversal by the verified cluster
count. The backend decodes short names and bounded checksum-validated UTF-16
long names, supports nested and multi-cluster reads, and cannot issue writes.
Filesystem code imports no PCI or virtio implementation types.
