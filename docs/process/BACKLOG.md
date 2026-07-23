# Product Backlog

## M0 — Boot Foundation

- S0-001 Repository and Rust workspace — implemented, verification pending.
- S0-002 UEFI firmware adapter — implemented, verification pending.
- S0-003 Freestanding kernel-core boundary — implemented, verification pending.
- S0-004 QEMU smoke test — implemented, verification pending.
- S0-005 CI and quality policy — implemented, verification pending.

## M1 — Firmware Exit and Kernel Ownership

- M1-001 Model UEFI boot services required for memory-map retrieval — implemented, compiler verification pending.
- M1-002 Capture and validate the firmware memory map — implemented, emulator verification pending.
- M1-003 Allocate a dedicated kernel stack — not started.
- M1-004 Introduce an owned boot-information ABI — implemented, compiler verification pending.
- M1-005 Call `ExitBootServices` with retry-safe map-key handling — implemented, emulator verification pending.
- M1-006 Replace firmware console dependence with an early serial logger — implemented for QEMU COM1.
- M1-007 Add fatal-error diagnostics and deterministic QEMU exit behavior — implemented, emulator verification pending.
- M1-008 Test malformed and changing memory-map scenarios — partial host validation; controlled firmware retry test pending.

## M2 — CPU and Exceptions

- M2-001 Architecture module boundary.
- M2-002 GDT and TSS.
- M2-003 IDT and exception stubs.
- M2-004 Page-fault diagnostics.
- M2-005 Local APIC timer investigation.
- M2-006 Double-fault stack and test.

## M3 — Memory Management

- M3-001 Physical-frame allocator.
- M3-002 Virtual address-space manager.
- M3-003 Kernel heap.
- M3-004 Guard pages and non-executable mappings.
- M3-005 Allocator property tests and stress harness.

## M4 — Scheduling and User Mode

- M4-001 Kernel threads.
- M4-002 Preemptive scheduler.
- M4-003 Ring-3 transition.
- M4-004 System-call ABI.
- M4-005 Process and capability object model.

## Later epics

Storage and VFS; PCI and device framework; networking; USB; graphics and compositor; audio; input; power management; user-space runtime; SDK; package/update/recovery; installer; encrypted storage; reference-laptop enablement; sandboxed AI services.
