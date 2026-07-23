# ADR 0002: UEFI and x86-64 first

- Status: Accepted
- Date: 2026-07-21

## Context

The first milestone needs a modern firmware interface, repeatable emulation, and a path to typical Intel/AMD laptops.

## Decision

Target 64-bit UEFI firmware on x86-64 first. Use QEMU with OVMF as the reference virtual platform.

## Consequences

- legacy BIOS is not supported;
- ARM64 work is deferred;
- the boot layer must isolate UEFI-specific types from the kernel core;
- hardware deployment requires UEFI configuration and a recovery plan.
