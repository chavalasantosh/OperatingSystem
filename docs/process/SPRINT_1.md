# Sprint 1 — Firmware Exit and Kernel Ownership

## Sprint goal

Transfer machine ownership from UEFI firmware to the SanjuOS kernel while preserving deterministic diagnostics and validated platform information.

## Implemented in M1-alpha

- [x] model the UEFI boot-services prefix required by `GetMemoryMap` and `ExitBootServices`;
- [x] reserve an aligned, kernel-retained memory-map buffer;
- [x] capture descriptor size, version, map key, map size, and descriptor count;
- [x] validate memory-map metadata before firmware exit;
- [x] retry `ExitBootServices` when firmware invalidates the map key;
- [x] add an early COM1 serial logger independent of firmware services;
- [x] transfer an owned `BootInfo` and `MemoryMapInfo` ABI to the kernel core;
- [x] add host tests for boot information and memory-map invariants;
- [x] add source-level UEFI ABI checks.

## Remaining before Sprint 1 acceptance

- [ ] compile and format with the pinned/current stable Rust toolchain;
- [ ] pass Clippy with warnings denied;
- [ ] pass host-side kernel tests;
- [ ] boot M0/M1 in QEMU with OVMF;
- [ ] prove `ExitBootServices` succeeded through post-exit serial output;
- [ ] capture CI logs and an interactive boot screenshot;
- [ ] implement and switch to a dedicated protected kernel stack;
- [ ] test the invalid-map-key retry path under a controlled harness.

## Exit criteria

Sprint 1 closes only when QEMU proves post-firmware execution, CI is green, and the kernel is running on a dedicated stack with documented bounds and alignment.
