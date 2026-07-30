# Changelog

## M6D Read-Only FAT32 candidate — 2026-07-30

- Added an allocation-free FAT32 backend over the architecture-independent
  block-device contract.
- Validated BPB geometry, FAT capacity, device bounds, FSInfo signatures, and
  the complete backup boot sector before mounting.
- Added bounded cluster traversal with free, reserved, bad, out-of-range,
  premature-end, overlong, and cyclic-chain rejection.
- Added FAT 8.3 and checksum-validated, bounded UTF-16 long-name decoding.
- Added root, nested-directory, offset, and multi-cluster persistent reads.
- Mounted FAT32 read-only at `/disk` behind the M6C VFS.
- Added streaming shell `cat`, path-aware `ls`, and `fat32` diagnostics.
- Added a deterministic 64 MiB FAT32 QEMU fixture preserving the M6B device
  identity sector.
- Advanced the capability registry to version 7 and added exact M6D smoke
  evidence while preserving M5 through M6C regression gates.

M6D does not expose persistent files to Ring 3, load executables from FAT32,
write persistent media, discover partitions, or support physical installation.

## M6C Bounded Cache and VFS candidate — 2026-07-27

- Adopted Soma OS as the temporary user-facing working identity while retaining
  historical tags and internal crate identifiers.
- Added a 16-sector, allocation-free, read-through LRU block cache.
- Made the M6C dirty-state policy explicit: every persistent write is rejected
  before transport and the cache must retain zero dirty entries.
- Added live first-miss/repeat-hit evidence against the accepted virtio device.
- Added fixed inode, superblock, mount, canonical path, directory, and backend
  contracts.
- Added bounded absolute-path normalization with component and depth limits.
- Added a four-entry mount table with component-boundary longest-prefix lookup.
- Added a 32-entry generation-protected user file-handle table.
- Adapted RAMFS and shell file operations behind the VFS contract.
- Added `cache` and `mounts` diagnostics.
- Added capability registry version 6 and mandatory M6B regression evidence.

M6C does not mount a persistent filesystem and cannot leave persistent dirty
data. Read-only FAT32 remains the M6D gate; persistent writes remain deferred
until corruption, recovery, and power-loss behavior are designed and tested.

## M6B Virtio Block Transport candidate — 2026-07-26

- Architecture-independent 512-byte sector block-device contract.
- Validated I/O, 32-bit, and 64-bit PCI BAR decoding.
- Modern virtio PCI common, notify, device, and configuration-window parsing.
- `VIRTIO_F_VERSION_1` negotiation and fail-closed device initialization.
- Allocator-owned, direct-mapped split virtqueue with one outstanding request.
- Polling read, write, and device-ID requests with status/reset/timeout errors.
- Dedicated disk identity verification and known-sector read evidence.
- Disposable-sector write/readback followed by restoration of original bytes.
- Sector-boundary rejection and `block` shell diagnostics.
- Capability registry version 5 and full M5/FH1/FH2/FH3/M6A regressions.

M6B does not mount a filesystem or expose general-purpose persistent writes.
Buffer caching and VFS contracts remain M6C; read-only FAT32 remains M6D.

## M6A PCI and Storage Discovery candidate — 2026-07-26

- Architecture-independent PCI identity and storage classification.
- x86 PCI configuration-mechanism #1 presence probe and enumeration.
- Multifunction and PCI bridge traversal with fixed-capacity fail-closed state.
- QEMU dedicated virtio-blk second-disk topology.
- Hardware virtio block target matching without block I/O.
- PCI diagnostics in the kernel shell.
- Capability registry version 4 and M6A acceptance evidence.
- FH3 preserved as a mandatory regression gate.

M6A cannot issue sector reads or writes. Virtio capability parsing, DMA,
request queues, and the block-device API remain the M6B gate.

## Foundation Hardening Phase 3 (`v0.0.8-fh3`) — 2026-07-26

- Deep-cloned private four-level roots for all M5 processes.
- Sanitized inherited user permissions and explicit process-owned promotion.
- Lower and upper hardware guard holes for user and Ring 0 stacks.
- Per-process TSS `RSP0` and syscall-stack activation.
- A 160-byte complete x86-64 interrupt-frame ABI.
- Timer-driven saved-frame, CR3, and Ring 0 stack switching.
- Two non-cooperative Ring 3 forward-progress probes.
- Register-sentinel preservation across preemption.
- Blocking, wakeup, and terminal-process reap operations.
- Exact private page-table inventories and deterministic reclamation.
- Capability registry version 3 and FH3 QEMU acceptance gates.
- M5, FH1, and FH2 regressions preserved as hard gates.

The runtime remains single-core and uses the legacy PIT/PIC. Per-process
floating-point and SIMD state, SMP, local APIC timers, PCID, copy-on-write,
demand paging, PCI, and persistent storage remain future work.

## Foundation Hardening Phase 2 candidate — 2026-07-25

### Added

- Frozen x86-64 virtual-memory layout and higher-half physical direct map.
- Complete inherited page-table hierarchy discovery and frame reservation.
- Fresh SanjuOS-owned PML4 built from the dedicated bootstrap pool.
- Hardware 4 KiB/2 MiB map, split, translate, protect, and unmap operations.
- Safe CR3 transition with NX, supervisor write protection, and global-TLB flush.
- PE/COFF section parsing with page-granular kernel W^X permissions.
- Cross-alias W^X hardening for Ring 3 images and kernel direct mappings.
- Real unmapped kernel guard holes and post-transition hardware probes.
- FH2 capability evidence, ADR, sprint plan, validation plan, and regressions.

### Boundary

The kernel now owns its active page-table root. Private process CR3 roots,
per-process Ring 0 stacks, and full timer-driven register-context switching remain
FH3 work and are not claimed by this phase.

## Foundation Hardening Phase 1 — 2026-07-24

### Added

- Exact Rust 1.97.0 toolchain and x86-64 UEFI target pinning in local and CI configuration.
- Canonical capability registry generating Rust data, the capability matrix, and smoke-test expectations.
- x86-64 architecture boundary for assembly, CPU state, interrupts, syscalls, serial, and QEMU control.
- Versioned `BootInfoV1` containing retained memory-map, loaded-image, active-CR3, ACPI, SMBIOS, and optional GOP framebuffer metadata.
- Explicit physical-memory ownership map with overlap rejection.
- Bitmap frame allocator with allocation, contiguous allocation, release, reservations, exhaustion handling, double-free detection, and reserved-frame protection.
- Dedicated 256-frame bootstrap pool reserved for future page-table structures.
- Foundation acceptance report and M5 regression gate.

### Boundary

This phase intentionally keeps the firmware-derived active page tables. It does not activate a fresh PML4, private process CR3 roots, hardware guard holes, or full register-context preemption.

### Rollback

`v0.0.5-m5` remains the immutable rollback point.

## M5-alpha protected userspace and startup — 2026-07-24

### Added

- Active CR3 capture, virtual-memory layout, map/unmap policy, page flags, W^X checks, reclaim accounting, and guarded-stack descriptors.
- Reusable first-fit kernel heap with deallocation and region merging.
- Ring 3 GDT entries, controlled `IRETQ` entry, `SYSCALL`/`SYSRET`, user-pointer validation, and user-fault recovery.
- Process control blocks, address-space/context models, and timer-quantum preemption evidence.
- Allocation-free ELF64 PIE loader and reproducible `init`, `hello`, and `fault-test` programs.
- Branded startup stages, stable failure codes, SanjuOS ASCII output, and approved PNG logo asset.
- One combined source, host-test, Clippy, UEFI-build, and QEMU acceptance flow.

### Boundary

M5 is a protected-userspace foundation, not the final security architecture. Private activated process page tables, hardware guard holes, and full process register switching remain M6 work.

### Safety status

Emulator-only. Physical installation remains unsupported.

## M4-alpha combined runtime implementation — 2026-07-24

### Added

- Legacy PIC remapping with IRQ0/IRQ1 policy and end-of-interrupt handling.
- 100 Hz PIT timer with observable interrupt ticks.
- PS/2 keyboard IRQ path, bounded lock-free scancode queue, and Set-1 decoder.
- Fixed-capacity round-robin kernel task scheduler.
- Allocation-free interactive shell with runtime diagnostics.
- Writable RAM filesystem and shell commands for listing, reading, and writing files.
- Scripted QEMU acceptance flow covering timer, keyboard vector, scheduler, shell, and RAMFS.

### Delivery model

M3 and M4 were intentionally combined into one major batch to reduce micro-commit and CI overhead.

### Safety status

Emulator-only. Physical installation remains unsupported.

## M2-alpha implementation — 2026-07-24

### Added

- Dedicated 64 KiB kernel stack and one-way post-firmware stack transition.
- x86-64 GDT, TSS, ring-0 stack, and double-fault IST stack.
- IDT handlers for breakpoint, double fault, general protection, and page fault.
- Recoverable breakpoint exception self-test and fatal CR2 diagnostics.
- Physical frame allocator restricted to UEFI conventional memory.
- 256 KiB allocation-only bootstrap heap.
- M2 host tests, ABI checks, timeout-protected QEMU gate, ADR, and Sprint 2 plan.

### Verification status

M1 is QEMU-verified. M2 source checks pass locally; Rust formatting, Clippy, unit tests, UEFI build, and QEMU execution must pass in CI before M2 is accepted.

### Safety status

Emulator-only. Physical installation remains unsupported.

## M1-alpha checkpoint — 2026-07-21

### Added

- x86-64 UEFI boot-services ABI through `ExitBootServices`.
- Aligned, retained 256 KiB firmware memory-map storage.
- Memory-map metadata validation and bounded map-key retry logic.
- Early 16550/COM1 serial diagnostics independent of UEFI console services.
- Owned `BootInfo` and `MemoryMapInfo` kernel handoff.
- Kernel tests for memory-map invariants and allocation-free integer output.
- Dependency-free source and UEFI ABI verification script.
- Sprint 1 plan and firmware-exit ADR.

### Verification status

Source and shell checks pass. Rust compilation, linting, and QEMU/OVMF execution remain pending because the current workspace cannot install the required toolchain.

### Safety status

Emulator-only. Physical installation remains intentionally unsupported.

## 0.0.1-prealpha / M0 scaffold — 2026-07-21

### Added

- Sprint 0 project charter, requirements, architecture, security, testing, SDLC, backlog, definition of done, and risk register.
- Dependency-free Rust UEFI boot layer for x86-64.
- Freestanding architecture-independent kernel core.
- Deterministic M0 boot banner.
- Host unit tests and QEMU/OVMF smoke-test automation.
- CI workflow and coding standards.

## M1 verification checkpoint

- Added a dependency-free LLVM/LLD UEFI verification probe.
- Produced a real x86-64 PE32+ `BOOTX64.EFI` artifact.
- Verified EFI application subsystem, entry point, embedded boot messages, and checksum.
- Kept the probe separate from the Rust-first product implementation.
- QEMU execution remains blocked in the current restricted container because QEMU/OVMF cannot be installed.
