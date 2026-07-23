# M2 Validation Record

## Completed in the restricted build workspace

- Python source and ABI invariants pass.
- UEFI table, memory-descriptor, x86-64 TSS, and IDT-entry sizes are validated.
- All shell scripts pass `bash -n`.
- TOML and GitHub Actions YAML parse successfully.
- Rust source delimiters are balanced.
- The exact x86-64 exception-stub assembly syntax was assembled successfully as a COFF object with Clang/LLVM.
- Git reports no whitespace errors in the M2 patch.

## Required acceptance evidence

The following remain CI gates because this workspace has no Rust compiler, QEMU, or OVMF:

- `cargo fmt --all -- --check`;
- host unit tests;
- Clippy with warnings denied;
- release build for `x86_64-unknown-uefi`;
- QEMU/OVMF smoke boot ending in `M2 core kernel gate: passed`.

M2 must not be marked accepted until both GitHub Actions jobs are green.
