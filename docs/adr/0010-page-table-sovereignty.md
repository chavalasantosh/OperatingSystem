# ADR 0010: SanjuOS-owned page tables and bounded transition identity map

- Status: Proposed pending the Foundation Hardening Phase 2 QEMU gate
- Date: 2026-07-25

## Context

Foundation Hardening Phase 1 established physical ownership, a bitmap frame
allocator, and a dedicated page-table bootstrap pool. SanjuOS still executes
through OVMF-created page tables, so firmware mapping choices remain part of the
kernel's trusted state.

The current kernel and UEFI loader are linked into one relocatable PE/COFF EFI
image. Moving the executing image to a permanent higher-half address requires a
separate kernel image or a relocation trampoline and is therefore a different
architectural decision from taking page-table ownership.

## Decision

Foundation Hardening Phase 2 constructs a fresh four-level hierarchy from the
dedicated bootstrap pool and reloads CR3 before interrupts or Ring 3 execution
are enabled.

The new hierarchy contains:

1. a bounded identity transition window for the combined EFI-stub kernel;
2. a higher-half physical direct map at `0xffff_8000_0000_0000`;
3. page-granular mappings for every PE/COFF image section;
4. dynamically allocated intermediate page-table nodes;
5. separate high virtual regions for heap probes and guarded kernel stacks.

The identity and direct-map aliases of the loaded image receive matching
permissions. Executable sections are read-only and executable. Writable
sections are writable and NX. Read-only data is read-only and NX. The mapper
rejects writable-plus-executable leaf mappings.

Before the general allocator returns a frame, SanjuOS walks the inherited CR3
hierarchy and reserves every reachable page-table frame. The old hierarchy is
kept reserved after the CR3 transition for diagnosis and rollback analysis; it
is not reclaimed in this phase.

## Security boundary

This phase establishes kernel page-table sovereignty and a real hardware guard-hole
probe. It does not yet provide private process CR3 roots. M5 user programs still
run sequentially in the shared kernel hierarchy, although their executable
pages are read-only and their stack pages are writable and NX.

The identity transition window is retained until SanjuOS gains a separate
higher-half kernel image and loader. Its existence is explicit, bounded, and
covered by the capability registry; it is not treated as the final layout.

## Consequences

- OVMF page tables are no longer active after early bootstrap.
- Page mapping, unmapping, translation, and protection become hardware-backed.
- The physical direct map gives later memory and driver code a stable address
  conversion contract.
- Kernel PE sections are protected with page-granular W^X permissions.
- The acceptance stack probe has absent guard pages in the hardware hierarchy.
- Page-table construction remains independent of the kernel heap.
- Private process address spaces, per-process kernel stacks, CR3 switching, and
  full register-context preemption remain Foundation Hardening Phase 3.
