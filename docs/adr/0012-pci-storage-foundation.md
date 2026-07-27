# ADR 0012: PCI and Storage Foundation

- Status: Accepted; M6B transport accepted and M6C delegated to ADR 0013
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
6. M6B uses the modern virtio PCI capability model and negotiates only
   `VIRTIO_F_VERSION_1`. Until the VM mapper supports explicit uncached device
   mappings, the standardized virtio PCI configuration-access capability is
   used for common, notify, and device-register access.
7. M6B exposes an architecture-independent 512-byte sector contract. Queue
   zero uses one allocator-owned direct-mapped DMA page and a split ring, with
   one synchronous outstanding request and bounded polling.
8. M6B identifies the dedicated disk through `VIRTIO_BLK_T_GET_ID`, reads a
   seeded sector, writes and reads back one disposable sector, then restores
   the original sector before the gate can pass.
9. Interrupt-driven completion, indirect descriptors, multiple outstanding
   requests, MSI-X, and explicit uncached MMIO mappings remain later transport
   work.
10. M6C adds bounded buffer-cache and VFS contracts without disk writes, as
    specified by ADR 0013.
11. M6D will mount a read-only FAT32 volume on the dedicated second disk. The
   EFI system partition is never used for filesystem experiments.
12. Persistent writes require a later gate with device identity, bounds checks,
   checksums, reboot verification, corruption tests, and recovery evidence.

## Consequences

- PCI inventory evidence is independently testable before DMA begins.
- M6B DMA memory has one owner, a fixed layout, bounded request sizes, and no
  lifetime shorter than the live device.
- A failed M6B request cannot be silently treated as successful, and the
  acceptance write is confined to the disposable test image.
- The first transport is optimized for reproducible QEMU validation, while AHCI
  and NVMe remain discoverable future physical-hardware targets.
- Early filesystem work can use standard FAT32 images and host tooling.
- No M6A code can modify a disk.
- The emulator-only installation policy remains in force.
