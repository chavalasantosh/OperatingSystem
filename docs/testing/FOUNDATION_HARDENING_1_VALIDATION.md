# Foundation Hardening Phase 1 Validation

## Purpose

Prove that the frozen build, capability, boot-handoff, physical-ownership, and
frame-allocation contracts work without regressing the accepted M5 runtime.

## Host quality gates

```text
python3 scripts/generate-capabilities.py --check
python3 scripts/source-check.py
cargo fmt --all -- --check
cargo test -p sanju-kernel
cargo clippy -p sanju-kernel --all-targets -- -D warnings
cargo clippy -p sanju-boot --target x86_64-unknown-uefi -- -D warnings
cargo build -p sanju-boot --release --target x86_64-unknown-uefi
```

## Required allocator tests

```text
allocates_unique_frames
reuses_freed_frames
rejects_double_free
rejects_reserved_frame_free
reserves_unaligned_ranges_correctly
handles_allocator_exhaustion
bootstrap_pool_does_not_use_heap
```

The ownership suite additionally verifies overlap rejection and preservation of
the loaded image, active page-table root, and framebuffer range.

## QEMU acceptance

Run:

```text
bash ./scripts/smoke-test.sh
```

The run must preserve all M5 Ring 3, syscall, ELF, recoverable user-fault, and
shell assertions and print:

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

## Safety boundary

This validation does not claim a fresh PML4, private process CR3 roots,
hardware-unmapped guard pages, or full register-context preemption. The active
firmware-derived hierarchy remains installed until Hardening Phase 2.
