# Sprint 5 — Protected User-Space Foundation

## Goal

Deliver one major M5 batch that moves SanjuOS from a kernel-only shell to the first branded startup and protected Ring 3 programs.

## Scope — 22 development items

1. Permanent virtual-memory layout.
2. Active CR3/page-table ownership capture.
3. Four-level page-table manager model.
4. Page mapping and unmapping APIs.
5. Page protection flags.
6. UEFI boot-service memory reclaim accounting.
7. Guarded kernel/user stack descriptors.
8. W^X policy enforcement.
9. Reusable kernel heap with free and merge.
10. Enhanced page-fault diagnostics.
11. Ring 3 GDT segments.
12. Per-process address-space objects and reserved roots.
13. User-stack construction.
14. Process control blocks.
15. CPU context model and switch accounting.
16. Timer-quantum preemption model plus real user timer interrupts.
17. `SYSCALL`/`SYSRET` entry and return.
18. Safe user-memory validation and copy APIs.
19. ELF64 PIE loader.
20. `init`, `hello`, and fault-isolation user programs with acceptance tests.
21. Branded startup stages and failure codes.
22. SanjuOS wordmark/logo printing and committed graphical logo asset.

## Initial syscall ABI

`write`, `read`, `exit`, `yield`, `getpid`, `open`, `close`, and `spawn` use syscall numbers 0 through 7.

## Exit gate

One clean GitHub Actions run must pass source checks, formatting, unit tests, Clippy, the x86-64 UEFI build, and QEMU/OVMF. The QEMU log must show all three user programs, a recovered user fault, `Ring 3 execution: active`, the SanjuOS brand, and `M5 protected user-space gate: passed`.

## Delivery policy

All M5 code is delivered in one patch and one repository archive. Small corrections are accumulated until the major gate is run; they are not released as separate micro-milestones.
