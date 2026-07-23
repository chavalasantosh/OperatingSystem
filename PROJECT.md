# Project Charter

- Working name: SanjuOS
- Start date: 2026-07-21
- Product owner: Sanju
- Initial architecture: x86-64
- Initial firmware: UEFI 2.x
- Primary implementation language: Rust 2024
- Delivery model: two-week Agile sprints within a gated SDLC
- Current checkpoint: 0.0.2-prealpha / M2-alpha implementation
- Accepted release: M1 kernel-ownership gate
- Deployment policy: QEMU only until physical-install safety gates pass

## Mission

Develop an independent desktop operating system through small, demonstrable, secure increments, ultimately targeting one supported laptop with a polished desktop and isolated AI-native services.

## Immediate objective

Pass all M2 quality and QEMU gates for the protected kernel stack, GDT/TSS/IDT, exception self-test, physical frame allocator, and bootstrap heap. After acceptance, begin M3 timer interrupts, keyboard input, cooperative scheduling, and an interactive kernel shell.
