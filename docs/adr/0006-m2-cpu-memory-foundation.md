# ADR 0006 — M2 CPU and Memory Foundation

## Status

Proposed implementation; acceptance requires green CI and QEMU smoke execution.

## Decision

SanjuOS M2 uses static, image-owned storage for its first kernel stack, double-fault stack, descriptor tables, retained UEFI memory map, and bootstrap heap. The boot layer abandons the firmware stack after `ExitBootServices`, loads a kernel GDT/TSS/IDT, runs a breakpoint exception self-test, and exposes only UEFI conventional-memory frames through the first physical allocator.

The bootstrap heap is an explicit allocation-only bump allocator. It is not yet Rust's global allocator and performs no deallocation. This keeps M2 deterministic while page-table ownership and production heap design remain future work.

## Consequences

- exception handling is available before enabling hardware interrupts;
- double faults have a protected IST stack;
- page faults produce vector, error-code, and CR2 diagnostics;
- loader and boot-service memory are intentionally not reclaimed yet;
- virtual-memory remapping, guard pages, and a reusable heap remain later gates;
- M2 remains emulator-only and single-core.
