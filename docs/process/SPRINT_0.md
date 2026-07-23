# Sprint 0 — Engineering Foundation

## Sprint goal

Create a disciplined repository and prove that an architecture-independent Rust core can execute through a modern x86-64 UEFI boot path in QEMU.

## Completed in the initial scaffold

- [x] product vision and M0 requirements;
- [x] Rust 2024 workspace;
- [x] kernel/firmware separation;
- [x] dependency-free UEFI entry point;
- [x] deterministic boot banner;
- [x] host unit tests;
- [x] QEMU smoke-test design;
- [x] CI workflow;
- [x] ADR, threat model, test strategy, SDLC, and definition of done;
- [x] physical-installation safety restriction.

## Environment-dependent verification remaining

- [ ] run formatting with the installed Rust toolchain;
- [ ] compile host tests;
- [ ] compile `x86_64-unknown-uefi` artifact;
- [ ] execute QEMU/OVMF smoke test;
- [ ] capture the first boot screenshot and CI evidence;
- [ ] initialize the remote Git repository and protect `main`.

## Exit criteria

Sprint 0 closes when CI passes from a clean checkout and the M0 banner is captured from QEMU.
