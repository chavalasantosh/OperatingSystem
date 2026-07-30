# ADR 0014: Read-Only FAT32 Behind the VFS

- Status: Accepted for M6D implementation
- Date: 2026-07-30

## Context

M6B established a bounded virtio block transport and M6C added a fixed-capacity
read-through cache plus VFS contracts. The next dependency is a real,
persistent filesystem that can be tested without authorizing general-purpose
disk writes or coupling filesystem code to x86 PCI details.

FAT32 is suitable for this gate because its on-disk structures are documented,
the fixture can be generated deterministically, and the implementation can be
kept allocation-free. The format is also untrusted input: malformed geometry,
cluster chains, directory entries, and long filenames must fail closed.

## Decision

1. M6D supports 512-byte-sector FAT32 volumes only.
2. Mounting validates the boot signature, BPB geometry, total device bounds,
   FAT capacity, root cluster, FSInfo signatures, and complete backup boot
   sector.
3. The filesystem depends only on the architecture-independent `BlockDevice`
   contract. M6D wraps the accepted virtio device in the M6C read-only cache.
4. RAMFS remains the writable root and FAT32 mounts at `/disk` as the one
   secondary VFS backend.
5. FAT and directory traversals are bounded by the validated data-cluster
   count. Free, reserved, bad, out-of-range, and cyclic chains are rejected.
6. File reads advance sequentially through the chain. When a read reaches the
   declared file size, the current cluster must terminate the chain.
7. FAT 8.3 names are ASCII-bounded. Long filenames require a complete ordinal
   sequence, matching short-name checksum, valid UTF-16, and a VFS-bounded
   component.
8. The VFS and cache reject all persistent writes. The M6D gate requires zero
   dirty cache entries after every acceptance read.
9. QEMU uses a deterministic generated image containing a root file, a long
   filename, a nested directory, and a multi-cluster file.

## Consequences

- Persistent data is now read through the real virtio/cache/VFS stack.
- Shell `ls` and `cat` can address `/disk` without knowing the backend type.
- Corrupt media produces bounded errors instead of unbounded traversal.
- The early VFS intentionally supports one root and one secondary concrete
  backend; a dynamic backend registry is deferred.
- OEM short-name code pages, partitions, writable FAT32, journaling, recovery,
  and physical-disk installation are outside M6D.

## Rollback

`v0.0.11-m6c` remains the rollback point. Removing the `/disk` mount and M6D
acceptance path restores the cache/VFS baseline without changing the accepted
M6B transport.
