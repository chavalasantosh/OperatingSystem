# ADR 0011: Private Address Spaces and Interrupt-Frame Scheduling

- Status: Accepted for Foundation Hardening Phase 3
- Date: 2026-07-26

## Context

Foundation Hardening Phase 2 replaced the firmware page tables with a
SanjuOS-owned four-level hierarchy, but M5 processes still executed
sequentially under the kernel CR3. Their process roots and register contexts
were software models rather than active hardware isolation boundaries.

The combined EFI-stub kernel still requires its bounded identity window. A
private process root must therefore preserve the complete supervisor execution
environment while exposing only explicitly owned image, data, and stack pages
to Ring 3.

## Decision

1. A process root is a deep clone of the SanjuOS kernel hierarchy allocated
   exclusively from the page-table bootstrap pool.
2. Every `USER` bit is removed during cloning. Only declared process mappings
   are promoted through all four levels.
3. Executable user mappings are read-only; writable user mappings are NX. The
   corresponding physical-direct-map alias remains supervisor-only and follows
   the same W^X policy.
4. Every process receives a private user stack and Ring 0 stack. Both have
   hardware-unmapped lower and upper guard pages in that process root.
5. A timer interrupt saves all 15 general-purpose registers plus the complete
   x86-64 privilege-transition tail. The dispatcher records the current frame,
   selects the next runnable process, changes CR3 and TSS `RSP0`, and returns
   the next saved-frame pointer to the assembly epilogue.
6. The FH3 QEMU probe runs two non-cooperative Ring 3 loops. At least two timer
   switches, two CR3 changes, and two register-sentinel checks are required.
7. An inactive process hierarchy retains an exact page-table-frame inventory.
   Reclamation returns every recorded frame to the bootstrap pool and verifies
   that the pool returns to its pre-process count.

## Consequences

- M5 ELF programs execute under real private CR3 roots.
- Timer preemption is a hardware path rather than a scheduler counter model.
- Kernel and user stack overflows encounter unmapped PTEs before adjacent
  storage.
- Page-table reclamation is deterministic and allocation-free.
- The current implementation remains single-core and uses the legacy PIT/PIC.
  SMP, local APIC timers, PCID, copy-on-write, demand paging, and a relocated
  standalone kernel image remain later work.
