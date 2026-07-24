# Foundation Hardening Phase 1

## Goal

Replace implicit M5 bootstrap assumptions with versioned contracts and explicit
physical-memory ownership, while preserving the complete M5 QEMU regression.

## Scope

- `[SYS-TC-001]` pin Rust 1.97.0 and the UEFI target;
- `[SYS-CAP-001]` generate runtime, documentation, and smoke data from one
  capability registry;
- `[SYS-ARCH-001]` move x86-64 implementation details out of `main.rs`;
- `[BOOT-INFO-001]` introduce the versioned `BootInfoV1` handoff;
- `[MEM-OWN-001]` reserve loaded images, retained boot structures, active page
  tables, firmware tables, framebuffer memory, and initrd memory when present;
- `[MEM-PF-001]` provide bitmap-backed frame allocation and release;
- `[MEM-PTB-001]` reserve a page-table-only bootstrap frame pool;
- keep M5 Ring 3, syscall, ELF, fault-isolation, shell, and QEMU behavior intact.

## Explicitly out of scope

- constructing or activating a fresh PML4;
- private process CR3 roots;
- hardware-unmapped guard pages;
- full register-context preemption;
- storage, VFS, graphics rendering, and compositor features.

## Acceptance output

```text
SanjuOS Foundation Hardening
Pinned toolchain: verified
Capability registry: synchronized
Architecture separation: verified
BootInfo version: 1
Physical ownership map: active
Reserved-range overlap test: passed
Frame allocation/free test: passed
Double-free detection: passed
Reserved-frame protection: passed
Page-table bootstrap pool: active
M5 regression boot: passed
Foundation hardening phase 1: passed
```

## Rollback

The accepted `v0.0.5-m5` tag is the rollback point. Phase 1 is delivered as one
commit and must not alter that tag.
