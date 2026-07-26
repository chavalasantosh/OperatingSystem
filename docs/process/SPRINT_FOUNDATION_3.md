# Foundation Hardening Phase 3

## Goal

Replace modeled M5 process isolation and scheduling with active private CR3
roots, guarded per-process kernel stacks, complete interrupt-frame switching,
timer preemption, and deterministic page-table reclamation.

## Delivered scope

1. Deep-cloned private four-level process hierarchies.
2. Removal of inherited Ring 3 permissions during cloning.
3. Explicit image, data, and user-stack promotion.
4. Cross-alias W^X preservation in each private root.
5. Lower and upper user-stack guard holes.
6. One lower/upper-guarded Ring 0 stack per M5 process.
7. TSS `RSP0` changes on every process activation.
8. Per-process syscall-stack selection.
9. A 160-byte x86-64 interrupt-frame ABI.
10. Save/restore of all 15 general-purpose registers.
11. Timer-dispatch handoff of the next saved frame.
12. CR3 switching during timer scheduling.
13. Two non-cooperative Ring 3 scheduling probes.
14. Per-process private counter pages for isolation evidence.
15. Register-sentinel preservation checks.
16. M5 execution under three real private roots.
17. Process block/wakeup/reap lifecycle operations.
18. Exact page-table-frame inventories per process.
19. Reverse-order page-table reclamation.
20. M5, FH1, and FH2 regression gates under FH3.

## Deferred

- SMP and per-CPU scheduler state.
- Local APIC and deadline timers.
- PCID/INVPCID optimization.
- Per-process x87, MMX, SSE, and AVX state.
- Copy-on-write and demand paging.
- A separately linked high-half kernel image.
- User VFS handles and executable spawning.
- PCI, storage drivers, persistent VFS, and graphics.

## Exit criteria

- Three M5 programs run with distinct private CR3 roots.
- Non-owned user pages fail effective user-access checks.
- User and Ring 0 guard pages are absent from each process root.
- Two Ring 3 probes make progress under timer preemption.
- At least two complete frame switches and CR3 changes occur.
- Register sentinels survive interrupt save/restore.
- Every private page-table frame is reclaimed.
- M5, FH1, and FH2 acceptance reports remain passed.
