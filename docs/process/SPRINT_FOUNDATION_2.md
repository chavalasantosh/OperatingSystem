# Foundation Hardening Phase 2

## Epoch goal

Replace firmware-derived virtual-memory authority with a SanjuOS-owned hardware
hierarchy in one major development batch.

## Scope

- frozen x86-64 virtual-memory layout;
- inherited hierarchy reservation;
- fresh PML4 construction from the bootstrap pool;
- bounded identity transition window;
- higher-half physical direct map;
- PE/COFF section parsing and page-granular W^X;
- hardware map, unmap, protect, translate, and TLB invalidation;
- a real unmapped guard-hole probe for future kernel stacks;
- controlled CR3 replacement;
- interrupt, M5, and FH1 regressions;
- capability, ADR, validation, and smoke-test updates.

## Explicitly deferred

- private process roots and CR3 switching;
- one Ring 0 stack per process;
- live timer-driven register-context switching;
- old hierarchy frame reclamation;
- separate higher-half ELF kernel image;
- storage, VFS, DMA, PCI, graphics, and compositor work.
