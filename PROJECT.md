# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Accepted checkpoint: M6A PCI and Storage Discovery
- Immutable release: `v0.0.9-m6a`
- Current development checkpoint: M6B virtio block transport
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Activate the dedicated QEMU virtio block target behind an
architecture-independent sector contract. Validate modern PCI capabilities,
feature negotiation, DMA queue ownership, a known-sector read, and a confined
write/readback/restore transaction.

## Next major objective

Add a fixed-capacity block cache and VFS contracts without enabling persistent
filesystem writes.
