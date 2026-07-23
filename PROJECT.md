# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Current checkpoint: 0.0.5-prealpha / M5-alpha
- Accepted releases: M1, M2, M3, and M4
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Pass the single M5 quality and QEMU gate for paging policy, reusable allocation, Ring 3 execution, syscalls, ELF loading, process/fault handling, and branded startup.

## Next major objective

After M5 acceptance, build hardware-owned process page tables and context switching, then move into PCI/storage discovery, a VFS, persistent filesystem, framebuffer graphics, and the first compositor.
