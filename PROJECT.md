# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Current checkpoint: Foundation Hardening Phase 2 candidate after `v0.0.6-fh1`
- Accepted releases: M1–M5, tagged M5 (`v0.0.5-m5`), and FH1 (`v0.0.6-fh1`)
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Construct and activate a SanjuOS-owned x86-64 page-table hierarchy with explicit W^X permissions, a physical direct map, real hardware map/unmap operations, and unmapped kernel guard holes while preserving M5 and FH1 regressions.

## Next major objective

Build private process CR3 roots, one kernel stack per process, complete interrupt-frame context switching, timer preemption, and resource reclamation before PCI/storage, VFS, persistent filesystems, or compositor work.
