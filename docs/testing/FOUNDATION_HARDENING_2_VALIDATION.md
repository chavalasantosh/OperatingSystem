# Foundation Hardening Phase 2 Validation

## Purpose

Prove that SanjuOS replaces the inherited OVMF page-table hierarchy with a
fresh kernel-owned PML4 without regressing firmware ownership, interrupts,
Ring 3 execution, syscalls, ELF loading, or Foundation Hardening Phase 1.

## Host gates

- pinned Rust 1.97.0 formatting;
- Clippy with warnings denied;
- kernel unit tests;
- generated capability registry synchronization;
- source-manifest verification;
- source checks for hardware mapper, CR3 transition, W^X, and direct-map code;
- UEFI release build and user-ELF W^X checks.

## QEMU gates

The smoke run must prove:

- a fresh PML4 root differs from the inherited root;
- every inherited page-table frame is reserved before allocation;
- CR3 points to the SanjuOS root after transition;
- the physical direct map translates to the expected physical frame;
- a high virtual page can be mapped, written, translated, protected, unmapped,
  and returned to the physical allocator;
- writable-plus-executable mappings are rejected;
- both guard pages around the kernel guard-stack probe are unmapped;
- PIT and keyboard interrupts operate after the CR3 switch;
- M5 Ring 3, syscall, ELF, and user-fault tests still pass;
- FH1 allocator and bootstrap-pool tests still pass.

## Failure policy

Any exception before the Phase 2 gate is a failed milestone. The inherited
root and all of its table frames remain reserved for post-mortem analysis. No
storage, VFS, graphics, DMA, or compositor development may begin until this
validation is green.
