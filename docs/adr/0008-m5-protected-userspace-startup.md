# ADR 0008: M5 Protected Userspace and Branded Startup

## Status

Proposed for M5-alpha acceptance after the quality and QEMU gates pass.

## Decision

M5 introduces the first protected-user execution path on x86-64. SanjuOS retains the active UEFI-created four-level page-table hierarchy, records its CR3 root, installs Ring 3 GDT selectors, programs `SYSCALL`/`SYSRET`, loads small position-independent ELF64 programs, and recovers from user-mode page faults without terminating the kernel.

The architecture-independent kernel gains fixed-capacity page-mapping policy, W^X validation, a reusable heap, process control blocks, guarded-stack descriptors, user-pointer validation, a syscall ABI model, and an allocation-free ELF loader. The platform layer performs the first real Ring 3 transitions and syscall/exception return paths.

The startup path prints the SanjuOS brand before firmware exit and again in the kernel report. The approved PNG logo is retained under `assets/branding/` for the future framebuffer compositor; M5 uses a deterministic ASCII rendering because no graphics compositor exists yet.

## Security boundary

M5 proves privilege transitions and fault recovery, but it is not a production isolation boundary. The initial user images execute from boot-owned mappings, and per-process root frames are modeled and reserved but are not yet activated with independent CR3 switches. Kernel relocation, private hardware page tables, true unmapped guard pages, copy-on-write, and full preemptive register-context switching remain hardening work.

## Consequences

- Ring 3, syscalls, ELF loading, user-fault recovery, process accounting, and startup branding become continuously testable in QEMU.
- No physical installation is permitted.
- The M5 acceptance report must not be interpreted as desktop readiness or a complete security model.
