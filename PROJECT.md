# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Initial architecture: x86-64
- Initial firmware: UEFI 2.x
- Primary implementation language: Rust 2024
- Delivery model: two-week Agile sprints within a gated SDLC
- Current checkpoint: 0.0.1-prealpha / M1-alpha implementation
- Accepted release: none; emulator boot evidence is still required
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system through small, demonstrable, secure increments, ultimately targeting one supported laptop with a polished desktop and isolated AI-native services.

## Immediate objective

Run all Sprint 0 quality gates and boot M1-alpha in QEMU. Acceptance requires proof that SanjuOS captures the UEFI memory map, exits firmware boot services, and continues logging from kernel-owned code.
