# ADR 0012: PCI and Storage Foundation

- Status: Accepted for M6 implementation
- Date: 2026-07-26

## Context

FH3 established isolated processes and timer-driven context switching. The next
product dependency is persistent storage, but disk writes are the first kernel
feature capable of destroying durable user data. PCI discovery, block
transport, caching, VFS policy, and filesystem parsing therefore need separate
acceptance boundaries.

The current supported machine is QEMU `q35`. SanjuOS has no ACPI PCI routing,
MSI/MSI-X, DMA allocator, block layer, or persistent filesystem yet.

## Decision

1. M6A uses x86 PCI configuration mechanism #1 through `0xCF8`/`0xCFC`.
2. Enumeration starts at bus zero, follows discovered PCI-to-PCI secondary
   buses, handles multifunction devices, and retains a bounded inventory.
3. Configuration-port access is serialized by disabling interrupts on the
   single bootstrap CPU for the discovery pass.
4. PCI identity and class decoding live in the architecture-independent kernel;
   raw port I/O remains inside the x86-64 adapter.
5. QEMU exposes a dedicated second `virtio-blk-pci` disk. M6A only proves that
   the exact controller is discovered and matched; it does not claim sector I/O.
6. M6B will introduce a polling virtio-blk transport behind a block-device API.
   Interrupt-driven completion is deferred until the polling contract passes.
7. M6C will add bounded buffer-cache and VFS contracts without disk writes.
8. M6D will mount a read-only FAT32 volume on the dedicated second disk. The
   EFI system partition is never used for filesystem experiments.
9. Persistent writes require a later gate with device identity, bounds checks,
   checksums, reboot verification, corruption tests, and recovery evidence.

## Consequences

- PCI inventory evidence is independently testable before DMA begins.
- The first transport is optimized for reproducible QEMU validation, while AHCI
  and NVMe remain discoverable future physical-hardware targets.
- Early filesystem work can use standard FAT32 images and host tooling.
- No M6A code can modify a disk.
- The emulator-only installation policy remains in force.
