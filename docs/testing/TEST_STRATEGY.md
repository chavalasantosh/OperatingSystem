# Test Strategy

## Test pyramid

1. Pure host unit tests for architecture-independent code.
2. Compile-time checks for firmware and architecture targets.
3. QEMU integration tests for boot, faults, memory, interrupts, and devices.
4. Deterministic image and artifact validation.
5. Reference-laptop tests only after safety gates.

## Current gates

- `cargo fmt --check`;
- Clippy with warnings denied;
- host unit tests for the kernel-core output contract;
- UEFI release build;
- headless QEMU boot;
- expected milestone strings observed through debug I/O;
- explicit QEMU success exit code.
- exact attached-device topology for hardware-driver milestones;
- every previously accepted milestone rerun as a regression gate.

## Future gates

- property tests for allocators and object tables;
- fuzzing for filesystems, protocols, image formats, and syscalls;
- fault injection and out-of-memory testing;
- race and lock-order testing;
- boot-time and memory-footprint budgets;
- suspend/resume and power-loss testing;
- update rollback and recovery drills.
