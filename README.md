# SanjuOS

SanjuOS is a long-term project to build an independent, secure, AI-native desktop operating system. It is **not** a Linux distribution. The project begins with a minimal x86-64 UEFI boot path and a freestanding Rust core, then grows through tested increments toward processes, drivers, graphics, user space, applications, installation, updates, and laptop-specific hardware support.

## Current checkpoint: M2-alpha — Protected Core Kernel

M1 is verified in GitHub Actions/QEMU. The M2 implementation adds:

- a dedicated 64 KiB kernel stack and one-way firmware-stack transition;
- a long-mode GDT and TSS;
- a dedicated double-fault IST stack;
- an IDT for breakpoint, double fault, general protection, and page fault;
- a recoverable breakpoint exception self-test;
- fatal page-fault diagnostics including CR2;
- a physical frame allocator over UEFI conventional memory;
- a 256 KiB aligned bootstrap bump heap;
- host tests, structural ABI checks, and a deterministic QEMU gate.

Expected M2 smoke output:

```text
SanjuOS
Milestone M2: CPU protection and early memory management.
Architecture: x86_64
Firmware: UEFI
Firmware boot services: exited
Protected kernel stack: active
GDT: active
TSS: active
IDT exception handling: active
Breakpoint exception self-test: active
Usable physical frames: <firmware-dependent count>
Allocated test frames: 3
Bootstrap heap allocations: 1
M2 core kernel gate: passed
Next gate: timer interrupts, keyboard, scheduler, and shell
```

## Engineering principles

1. Correctness before feature count.
2. Memory-safe Rust by default; every `unsafe` block requires a documented invariant.
3. Minimal dependencies in the trusted boot path.
4. Emulator-first validation before physical-hardware experiments.
5. Reproducible builds and automated quality gates.
6. Architecture decisions recorded as ADRs.
7. Two-week Agile sprints inside a gated SDLC.
8. Security and recovery are product features, not final-stage additions.

## Prerequisites

The reference environment is Ubuntu or Ubuntu under WSL2.

```bash
sudo apt update
sudo apt install -y build-essential make qemu-system-x86 ovmf mtools dosfstools
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Then:

```bash
make setup
make source-check
make fmt
make lint
make test
make build
make smoke
```

For an interactive emulator boot:

```bash
make run
```

## Repository map

```text
boot/uefi/          UEFI entry, x86-64 CPU setup, and early serial adapter
kernel/             Freestanding core, memory-map model, frame and heap allocators
scripts/            Build, image, QEMU, source-check, and test automation
docs/requirements/  Product and system requirements
docs/architecture/  System boundaries and technical design
docs/adr/           Architecture Decision Records
docs/security/      Threat model and secure-development rules
docs/testing/       Test strategy and release gates
docs/process/       Agile, SDLC, backlog, and definition of done
```

## Safety

Do not install this project onto a physical disk. M2-alpha is emulator-only. Physical-hardware deployment begins only after storage safety, recovery, boot fallback, page-table ownership, and device-specific validation gates exist.

## Verification status

M1 has passed the QEMU smoke gate. M2 source and ABI checks can run without Rust; M2 acceptance still requires the complete GitHub Actions quality and QEMU jobs to pass from a clean checkout.
