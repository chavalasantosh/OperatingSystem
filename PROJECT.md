# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Current checkpoint: Foundation Hardening Phase 3
- Release candidate: `v0.0.8-fh3`
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Run M5 processes under private CR3 roots with guarded user/Ring 0 stacks, switch complete interrupt frames under timer preemption, reclaim process page-table resources, and preserve M5 through FH2 regressions.

## Next major objective

Begin PCI discovery and the storage-driver/VFS foundation before persistent filesystems or compositor work.
