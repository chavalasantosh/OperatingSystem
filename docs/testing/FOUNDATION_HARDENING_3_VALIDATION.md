# Foundation Hardening Phase 3 Validation

## Purpose

Validate that process isolation, Ring 0 stacks, context switching, and process
page-table reclamation are active hardware paths while preserving every earlier
acceptance gate.

## Static and host checks

- FH3 capability entries remain synchronized in the generated registry;
- the complete interrupt-frame ABI is exactly 160 bytes;
- `RIP`, `CS`, and user `RSP` offsets match the assembly push/pop order;
- private-root cloning removes inherited `USER` bits;
- explicit range promotion and guard-hole clearing are present;
- the timer trampoline passes and consumes saved-frame pointers;
- CR3, TSS `RSP0`, and syscall-stack updates are structurally required;
- process roots retain exact reclaimable frame inventories;
- process block, wake, and reap unit tests pass;
- the source manifest matches every retained source file.

## QEMU acceptance

The smoke boot must report:

```text
Private process CR3 roots: active
Private M5 address spaces: 3
User and Ring 0 guard holes: active
Per-process Ring 0 stacks: active
Ring 3 preemption processes: 2
Complete interrupt-frame switching: active
Process page-table reclamation: passed
M5 regression under private CR3: passed
FH1 allocator regression under FH3: passed
FH2 paging regression under FH3: passed
Foundation hardening phase 3: passed
```

The dynamic gate additionally requires at least two timer preemptions, two
complete context switches, two CR3 switches, and two register-context checks.
Both private counter pages must make forward progress.

## Failure behavior

- A private-root construction error stops boot before Ring 3 entry.
- A scheduler-probe user fault restores the kernel CR3 and fails FH3.
- An invalid next frame or CR3 transition restores the kernel runtime and fails
  the gate.
- A live root cannot be reclaimed.
- A frame-count or pool-restoration mismatch fails resource reclamation.

## Boundary

This phase proves a single-core PIT-driven hardware scheduler and complete
integer interrupt-frame switching. Probe processes do not use floating-point or
SIMD instructions; per-process x87, MMX, SSE, and AVX state remains deferred.
It does not claim SMP safety, local APIC scheduling, PCID optimization,
copy-on-write, demand paging, persistent storage, or a graphics compositor.
