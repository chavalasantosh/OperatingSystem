# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Accepted checkpoint: Foundation Hardening Phase 3
- Immutable release: `v0.0.8-fh3`
- Current development checkpoint: M6A PCI and storage discovery
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Enumerate the QEMU PCI topology through hardware configuration transactions,
retain a bounded device inventory, and match a dedicated virtio block target
without issuing disk I/O.

## Next major objective

Add a polling virtio-blk transport and architecture-independent block-device
API after M6A passes QEMU.
