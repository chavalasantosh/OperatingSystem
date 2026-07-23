# Combined Sprint 3/4 — Interactive Runtime

## Goal

Move from a protected boot kernel to an interrupt-driven, testable interactive environment in one major delivery.

## Scope

PIC/PIT, timer IRQ, keyboard IRQ and queue, keyboard decoder, scheduler foundation, kernel shell, RAM filesystem, QEMU acceptance script, tests, and documentation.

## Exit gate

Both GitHub Actions jobs pass from a clean checkout and the QEMU log contains `M4 interactive runtime gate: passed` plus successful scripted shell and RAMFS output.
