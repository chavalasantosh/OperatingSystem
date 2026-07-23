# SanjuOS

SanjuOS is an independent, Rust-first desktop operating-system project. It is not a Linux distribution. Development proceeds through emulator-verified kernel milestones before any physical-disk work.

## Current checkpoint: M4-alpha — Interactive Runtime

M1 and M2 are QEMU-verified. This combined M3/M4 batch adds:

- remapped 8259 PIC interrupt controllers;
- a 100 Hz PIT timer and IRQ0 accounting;
- an IRQ1 PS/2 keyboard path with a lock-free scancode queue;
- Set-1 keyboard decoding for an interactive command line;
- a fixed-capacity round-robin kernel-task scheduler;
- an allocation-free interactive kernel shell;
- a writable in-memory filesystem with `ls`, `cat`, and `write`;
- automated timer, keyboard-vector, scheduler, shell, and RAMFS acceptance checks.

Expected smoke output includes:

```text
SanjuOS
Milestone M4: interrupt-driven runtime and interactive kernel environment.
PIT timer interrupts: active
PS/2 keyboard interrupt path: active
Round-robin scheduler: active
Interactive kernel shell: active
RAM filesystem: active
M4 interactive runtime gate: passed
SanjuOS kernel shell ready.
```

## Shell commands

```text
help version uptime memory irq tasks ls cat write echo clear
```

Run interactively with:

```bash
make setup
make run
```

Run all gates with:

```bash
make source-check
make fmt
make lint
make test
make smoke
```

## Repository map

```text
boot/uefi/          UEFI entry, CPU tables, PIC/PIT, IRQ handlers, serial adapter
kernel/             Core models, memory, scheduler, keyboard decoder, shell, RAMFS
scripts/            Build, image, QEMU, source-check, and test automation
docs/               Requirements, architecture, ADRs, testing, security, and process
```

## Safety

M4-alpha remains emulator-only. Do not install it on a physical disk. The next major gate establishes page-table ownership, guarded virtual memory, ring-3 user mode, syscalls, and executable loading.
