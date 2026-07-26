# M6A PCI Discovery Validation

## Purpose

Prove that SanjuOS discovers the QEMU PCI topology through real x86
configuration-port transactions and matches the dedicated virtio block target.
No block request or disk write is permitted in this phase.

## Static and host checks

- PCI address validation rejects device numbers above 31 and functions above 7.
- Storage classification distinguishes virtio-blk, AHCI, NVMe, IDE, and other
  mass-storage functions.
- The fixed inventory rejects duplicate BDF addresses and overflow.
- Raw `in`/`out` instructions remain in the x86-64 architecture adapter.
- Capability registry version 4 is synchronized.

## QEMU topology

The smoke machine uses `q35` and attaches a disposable 8 MiB second disk through
`virtio-blk-pci`. The disk is blank and is removed after the test.

## Required boot evidence

```text
SanjuOS M6A PCI and Storage Discovery
PCI configuration mechanism #1: active
PCI inventory completeness: active
Virtio block PCI target: active
Storage driver target: virtio-blk-pci
FH3 regression under M6A: passed
M6A PCI discovery gate: passed
```

The shell `pci` command must also report exactly one virtio-blk target in the
smoke topology.

## Failure behavior

- An unavailable configuration mechanism fails M6A without attempting block I/O.
- Inventory or bridge-queue overflow fails the completeness gate.
- Absence of the dedicated virtio target fails driver matching.
- Earlier milestone failures stop boot before M6A can pass.

## Boundary

M6A discovers and classifies hardware only. It does not parse BARs or virtio
capabilities, configure DMA, submit block requests, mount a filesystem, or
write persistent media.
