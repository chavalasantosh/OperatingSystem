# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Current checkpoint: Foundation Hardening Phase 1 after `v0.0.5-m5`
- Accepted releases: M1, M2, M3, M4, and tagged M5 (`v0.0.5-m5`)
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Freeze the build, capability, architecture, boot-handoff, physical-ownership, frame-allocation, and page-table-bootstrap contracts while preserving the complete M5 regression boot.

## Next major objective

Construct and activate a fresh SanjuOS PML4 with a hardware-backed mapper and real guard holes. Private process CR3 roots and complete saved-register context switching follow before PCI/storage, VFS, persistent filesystems, or compositor work.
