# ADR 0013: Bounded Cache and VFS Contracts

- Status: Accepted for M6C implementation
- Date: 2026-07-27

## Context

M6B established one synchronous 512-byte block device and proved a confined
write/readback/restore transaction on a dedicated QEMU disk. Filesystem parsing
must not depend directly on the virtio transport, and persistent writes cannot
be enabled before recovery and power-loss behavior exists.

The kernel remains `no_std`, single-core, allocation-conscious, and subject to
deterministic memory limits. Its existing RAMFS predates mount, path, inode, and
user file-handle contracts.

## Decision

1. M6C adds a 16-entry, allocation-free, read-through sector cache.
2. Cache replacement is least-recently-used among clean entries.
3. A failed device read does not evict or alter an existing cache entry.
4. The only M6C dirty-state policy is `RejectWrites`. An in-range write returns
   an error before reaching the block device and cannot create dirty data.
5. The live acceptance probe reads sector 8 twice. The first request must
   produce one miss and one device read; the second must produce one hit and no
   additional device read.
6. VFS paths are canonical, absolute, limited to 256 bytes and 16 components,
   collapse repeated separators and `.`, and bound `..` at root.
7. The mount table has four fixed slots and resolves only on component
   boundaries using the longest matching prefix.
8. User file handles have 32 fixed slots. Every identifier contains a slot and
   generation so a closed identifier cannot access a reused slot.
9. RAMFS implements the common inode, superblock, lookup, read, create/replace,
   and directory-visit contracts. RAMFS may remain volatile and writable.
10. M6D may add a read-only FAT32 backend behind these contracts. It must not
    bypass the cache or expose transport-specific state through the VFS.

## Consequences

- Filesystem code no longer depends on PCI or virtio types.
- Cache memory and handle memory have compile-time upper bounds.
- A repeat read has hardware-backed cache evidence rather than only a unit
  model.
- Persistent storage remains unable to receive general-purpose writes.
- RAMFS shell behavior is preserved while exercising the future FAT32 boundary.
- Multiple live filesystem backend types require a later mount-dispatch design;
  M6C records mount contracts but activates only the RAMFS root backend.
