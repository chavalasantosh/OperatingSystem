# Project Charter

- Working name: Soma OS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Accepted checkpoint: M6B Virtio Block Transport
- Immutable release: `v0.0.10-m6b`
- Current development checkpoint: M6C bounded cache and VFS
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Add an allocation-free, fixed-capacity read-through cache over the accepted
virtio block transport. Establish bounded inode, superblock, mount, canonical
path, directory, and generation-protected user file-handle contracts. Adapt
RAMFS behind that VFS boundary without enabling persistent writes.

## Next major objective

Validate and mount a dedicated FAT32 image read-only, then expose bounded
persistent directory listing and file reads through the accepted VFS.
