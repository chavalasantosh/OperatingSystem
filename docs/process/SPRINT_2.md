# Sprint 2 — CPU Protection and Early Memory

## Goal

Move SanjuOS from firmware ownership into a protected x86-64 kernel runtime with its own stack, exception tables, physical-frame allocator, and bootstrap heap.

## Implemented

- dedicated 64 KiB kernel stack and one-way stack transition;
- GDT with long-mode code/data descriptors;
- TSS with ring-0 stack and a dedicated double-fault IST stack;
- IDT entries for breakpoint, double fault, general protection, and page fault;
- recoverable breakpoint self-test and fatal exception diagnostics;
- allocation-only physical frame allocator over UEFI conventional memory;
- 256 KiB aligned bootstrap bump heap;
- host tests and deterministic QEMU M2 smoke assertions.

## Acceptance gates

- `cargo fmt --all -- --check`;
- kernel unit tests;
- Clippy with warnings denied for host and UEFI targets;
- release UEFI build;
- QEMU output contains `M2 core kernel gate: passed`;
- QEMU exits through the deterministic success port.
