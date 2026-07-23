# LLVM UEFI verification probe

This directory contains a freestanding C probe used only to verify the SanjuOS
UEFI ABI and packaging path when the Rust compiler is unavailable.

It validates that the available LLVM/LLD toolchain can produce an x86-64
PE32+ EFI application with:

- the Microsoft x64 / UEFI calling convention;
- UEFI system-table and boot-services validation;
- memory-map capture;
- retry-safe `ExitBootServices` handling;
- post-firmware serial and QEMU debug output;
- a QEMU debug-exit success/failure contract.

It does **not** replace the Rust boot application or kernel. Product code
remains Rust-first.

Build and validate:

```bash
make verify-probe
```

Output:

```text
build/probe-esp/EFI/BOOT/BOOTX64.EFI
```
