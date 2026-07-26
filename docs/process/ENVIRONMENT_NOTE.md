# Environment Verification Note

GitHub Actions is the authoritative clean Ubuntu validation environment. It
installs the pinned Rust toolchain, QEMU, and OVMF, then runs the complete
quality and smoke workflows from a fresh checkout.

Dependency-free source checks validate critical source markers and x86-64 UEFI
ABI layout assumptions in constrained workspaces, but they do not replace the
compiler or emulator gates.

The mandatory validation sequence on a configured development machine is:

```bash
make setup
make source-check
make fmt
make lint
make test
make smoke
```

Any compiler, linker, firmware, or emulator issue is release-blocking. A
milestone remains a candidate until the matching QEMU smoke gate passes from a
clean checkout.
