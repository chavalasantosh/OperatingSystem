# ADR 0009: Freeze platform contracts before storage and graphics

- Status: Accepted
- Date: 2026-07-24

## Context

M0 through M5 proved firmware exit, protected kernel execution, interrupt delivery,
Ring 3 entry, syscalls, ELF loading, and recoverable user faults. Several M5
memory and scheduling capabilities remain acceptance models rather than final
hardware-owned implementations. Building storage, a VFS, or a compositor on top
of those models would create avoidable coupling and rework.

## Decision

SanjuOS enters Foundation Hardening Phase 1 with no new user-facing subsystem.
This phase freezes the contracts that the later page-table, scheduler, storage,
and graphics work will consume.

1. Rust is pinned to 1.97.0, including rustfmt, Clippy, and the
   `x86_64-unknown-uefi` target.
2. `capabilities/capabilities.toml` is the single source of truth for capability
   names, maturity states, generated Rust data, documentation, and smoke-test
   expectations.
3. Architecture-specific assembly, register access, port I/O, and QEMU exit
   handling live under `boot/uefi/src/arch/x86_64/`; `main.rs` remains the UEFI
   orchestration layer.
4. The UEFI-to-kernel handoff is a versioned, C-compatible `BootInfoV1` with no
   Rust references or heap-backed collections. It carries the memory map, loaded
   image range, retained handoff range, active CR3 root, ACPI/SMBIOS addresses,
   and optional GOP framebuffer metadata.
5. A physical ownership map reserves every known live physical range before the
   general allocator becomes available.
6. Conventional-memory allocation uses a fixed-capacity bitmap supporting
   allocation, contiguous allocation, release, arbitrary reservations, and
   misuse detection.
7. A dedicated 256-frame bootstrap pool is reserved for future page-table nodes,
   preventing page-table construction from depending on the dynamic heap.
8. This phase does not construct a new PML4 and does not reload CR3. The current
   M5 execution path remains the regression baseline.

## Consequences

- Toolchain upgrades become deliberate changes rather than accidental CI events.
- Runtime claims can no longer drift independently across code, documentation,
  and smoke tests.
- Future physical and virtual memory work starts from an explicit ownership map.
- The bitmap currently tracks up to 8 GiB of conventional memory. Larger-memory
  and sparse-memory policies are deferred until real hardware requirements are
  known.
- The boot layer and kernel remain one EFI image during this phase. A separate
  kernel image and loader remain a later architectural decision.
- Storage, VFS, graphics, and compositor work remain blocked until fresh SanjuOS
  page tables and real process contexts are verified.
