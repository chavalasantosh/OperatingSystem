# M6B Virtio Block Transport Validation

## Purpose

Prove that SanjuOS can initialize the one dedicated QEMU virtio block target
and complete bounded sector requests without selecting the EFI system
partition or leaving acceptance-test data on the disk.

## Host checks

- Block geometry reports 512-byte sectors and checked byte capacity.
- Zero-length, end-of-device, and overflowing ranges are rejected.
- Read-only media rejects writes through the block contract.
- I/O, 32-bit memory, and 64-bit memory BARs decode without truncation.
- The M6B report cannot pass without every transport and M6A regression fact.

## QEMU fixture

The smoke test creates a temporary 8 MiB raw second disk, writes
`SANJUOS-M6B-READ-PATTERN` at sector 8, and attaches it with
`serial=SANJU-M6B`. The firmware ESP is a separate drive.

Sector 16 is the sole acceptance write target. SanjuOS reads and retains its
original contents, writes a deterministic pattern, reads the pattern back, and
restores the original contents.

## Required boot evidence

```text
SanjuOS M6B Virtio Block Transport
Architecture-independent block-device API: active
Modern virtio PCI capabilities: active
PCI bus mastering: active
Virtio feature negotiation: active
DMA-safe split virtqueue: active
Dedicated storage identity: verified
Known sector read test: passed
Disposable sector write/readback test: passed
Disposable sector restoration: passed
Block bounds rejection test: passed
Block request timeout protection: active
M6A regression under M6B: passed
M6B block transport gate: passed
```

The scripted shell must also run `block` and report the capacity, queue size,
read result, and write/readback result.

## Fail-closed cases

- Missing, duplicate, cyclic, truncated, or out-of-range capabilities stop M6B.
- An unassigned or malformed BAR stops M6B.
- Failure to negotiate `VIRTIO_F_VERSION_1` stops M6B.
- Read-only media, zero capacity, an unavailable queue, or a wrong device ID
  stops M6B.
- Device-reset requests, non-success status bytes, malformed used entries, and
  polling timeouts return errors instead of advancing the gate.

## Boundary

M6B has no partition parser, block cache, VFS, or filesystem. Its write probe
does not authorize persistent filesystem writes. Physical disks remain
unsupported.
