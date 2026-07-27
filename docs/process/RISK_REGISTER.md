# Risk Register

| ID | Risk | Probability | Impact | Current mitigation |
|---|---|---:|---:|---|
| R-001 | Scope expands toward macOS parity too early | High | Critical | milestone contracts and explicit non-goals |
| R-002 | Hardware documentation is unavailable | High | High | choose one documented reference laptop later |
| R-003 | Unsafe Rust introduces memory corruption | Medium | Critical | narrow unsafe boundaries, comments, tests, review |
| R-004 | Toolchain/API churn breaks builds | Medium | Medium | stable Rust, pinned CI policy, controlled upgrades |
| R-005 | Storage bug destroys user data | Medium | Critical | emulator-only policy until recovery and fuzz gates |
| R-006 | Dependency compromise enters trusted base | Medium | Critical | dependency ADR and minimal boot dependencies |
| R-007 | Project owner becomes blocked by low-level complexity | High | High | small demonstrable increments and documented learning path |
| R-008 | Visual work distracts from kernel reliability | High | Medium | graphics begins only after memory/process foundations |
| R-009 | Incorrect interrupt-frame layout corrupts a resumed process | Medium | Critical | frozen 160-byte ABI, offset checks, register sentinels, QEMU preemption gate |
| R-010 | CR3 and TSS stack selection diverge | Medium | Critical | single scheduler transition function and private-root smoke probe |
| R-011 | Process teardown leaks or double-frees page tables | Medium | High | exact frame inventory, inactive-root guard, reverse reclamation, pool-count check |
| R-012 | Legacy PIT/PIC results do not generalize to SMP | High | High | explicit single-core boundary; APIC/SMP remains a separate milestone |
| R-013 | PCI enumeration races configuration access | Low | High | single-CPU ownership and interrupt serialization during M6A |
| R-014 | Malformed PCI topology overflows fixed discovery state | Medium | High | bounded inventory/bridge queue and fail-closed completeness gate |
| R-015 | DMA or block writes corrupt memory or persistent media | Medium | Critical | owned DMA page, fixed chains, identity/bounds/status/timeouts, disposable sector restoration |
| R-016 | Untrusted virtio PCI capabilities redirect register access | Medium | Critical | bounded capability traversal, validated BAR references and region offsets, exact-width access |
