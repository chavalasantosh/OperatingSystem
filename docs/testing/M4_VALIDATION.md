# M4 Validation

M4 is accepted only when one clean CI run passes:

- source and ABI checks;
- `cargo fmt`;
- host unit tests for decoder, scheduler, shell, RAMFS, memory, and reports;
- Clippy with warnings denied;
- x86-64 UEFI release build;
- QEMU/OVMF boot observing real PIT ticks;
- keyboard interrupt-vector self-test;
- scheduler dispatch evidence;
- scripted shell and RAMFS commands;
- `M4 interactive runtime gate: passed`.
