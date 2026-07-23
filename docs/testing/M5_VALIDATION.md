# M5 Validation

M5 is accepted only when a clean checkout passes one complete CI run.

## Static and host gates

- source/UEFI ABI checks;
- embedded PNG signature and ELF64 structural checks;
- deterministic user-program source and build script;
- `cargo fmt --check`;
- kernel unit tests for paging policy, guarded stacks, reusable heap, process table, syscall validation, ELF loading, startup output, shell, RAMFS, scheduler, input, and memory;
- Clippy with warnings denied;
- release build for `x86_64-unknown-uefi`.

## QEMU gates

- CR3 root captured;
- GDT/TSS/IDT and timer runtime retained;
- `init.elf` enters Ring 3, prints through `sys_write`, yields, and exits;
- `hello.elf` enters Ring 3 and exits;
- `fault-test.elf` causes a user page fault that is diagnosed and isolated;
- kernel continues into the shell;
- scripted `userspace`, `ls`, `cat`, `tasks`, and `uptime` commands work;
- final log contains `M5 protected user-space gate: passed`.

## Known non-gates

M5 does not claim private activated CR3 roots, hardware-enforced 4 KiB guard-page holes under huge-page firmware mappings, SMP, production preemptive context switching, persistent storage, or a graphical compositor.
