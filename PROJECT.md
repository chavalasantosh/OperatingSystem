# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Architecture: x86-64
- Firmware: UEFI 2.x
- Primary language: Rust 2024
- Current checkpoint: 0.0.4-prealpha / M4-alpha
- Accepted releases: M1 and M2
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system with a secure Rust-first kernel, modern user environment, and later AI-native services.

## Delivery policy

Development is grouped into major milestone batches. Small formatting or CI corrections are accumulated and shipped with the next substantial batch rather than creating separate delivery cycles.

## Current objective

Pass the combined M3/M4 quality and QEMU gates for timer and keyboard interrupts, scheduler foundations, interactive shell, and RAM filesystem.

## Next major objective

M5 will take ownership of page tables, add guarded virtual address spaces and a reusable kernel allocator, enter ring 3, define the syscall ABI, and load the first user executable.
