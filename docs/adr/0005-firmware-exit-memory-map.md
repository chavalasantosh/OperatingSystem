# ADR 0005: Firmware exit and retained memory map

- Status: Proposed pending QEMU verification
- Date: 2026-07-21

## Context

The kernel cannot claim ownership of the platform while it depends on UEFI boot services. `ExitBootServices` requires a current memory-map key, and firmware may change that key between map retrieval and exit.

## Decision

M1-alpha reserves a 256 KiB, 16-byte-aligned static buffer inside the boot image. The loader retrieves the memory map into that buffer, validates the returned descriptor metadata, and calls `ExitBootServices` without invoking any map-mutating firmware service in between. An `EFI_INVALID_PARAMETER` result triggers a bounded retry with a fresh map key.

After successful exit, the firmware console is never used again. A minimal 16550-compatible COM1 writer provides deterministic diagnostics in QEMU. The kernel receives the retained buffer address and descriptor metadata through an owned boot-information structure.

## Consequences

- the transition avoids allocator activity in its critical window;
- the map remains available for the future physical-frame allocator;
- the fixed buffer is simple and auditable but may be insufficient on unusual firmware;
- COM1 is an emulator/debug transport, not the final hardware logging design;
- a dedicated kernel stack remains required before M1 acceptance;
- physical deployment remains prohibited.
