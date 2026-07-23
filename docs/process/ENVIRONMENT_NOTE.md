# Environment Verification Note

The project was advanced to M1-alpha in a workspace without Rust, QEMU, or OVMF. Network restrictions also prevented package installation. Dependency-free source checks validate critical source markers and x86-64 UEFI ABI layout assumptions, but they do not replace compilation or an emulator boot.

The first mandatory action on a configured development machine is:

```bash
make setup
make source-check
make fmt
make lint
make test
make smoke
```

Any compiler, linker, firmware, or emulator issue becomes a release-blocking defect. Neither M0 nor M1 is accepted until the QEMU smoke gate passes from a clean checkout.
