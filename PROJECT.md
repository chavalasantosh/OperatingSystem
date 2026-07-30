# Project Charter

- Working name: Soma OS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Accepted checkpoint: M6C Bounded Cache and VFS
- Immutable release: `v0.0.11-m6c`
- Current development checkpoint: M6D read-only FAT32
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Validate and mount a deterministic FAT32 volume over the accepted cache and
virtio block stack. Expose bounded persistent directory and file reads through
the VFS and shell while preserving an enforceable zero-dirty-data boundary.

## Next major objective

Add process-facing persistent file syscalls and executable reads without
weakening path, handle, address-space, or read-only storage isolation.
