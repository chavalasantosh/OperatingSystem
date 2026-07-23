# ADR 0001: Rust-first systems implementation

- Status: Accepted
- Date: 2026-07-21

## Context

The project requires low-level control, predictable performance, explicit memory ownership, and a smaller class of memory-safety defects than traditional C-only kernels.

## Decision

Use stable Rust and the Rust 2024 edition for new boot, kernel, service, and tooling components. C and assembly are permitted only where firmware, hardware, ABI, or mature external code makes them necessary.

## Consequences

- `unsafe` remains necessary at hardware and ABI boundaries.
- every unsafe invariant must be documented and reviewed;
- some hardware ecosystems may require C bindings;
- toolchain upgrades are deliberate, tested changes rather than automatic merges.
