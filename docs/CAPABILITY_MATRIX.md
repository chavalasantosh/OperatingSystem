# SanjuOS Capability Matrix

Registry version: **1**

This file is generated from `capabilities/capabilities.toml`. Do not edit it manually.

| ID | Capability | Status | Milestone | Evidence |
|---|---|---|---|---|
| `SYS-TC-001` | Pinned Rust toolchain | `verified` | `foundation-hardening-1` | `rust-toolchain.toml`<br>`.github/workflows/ci.yml` |
| `SYS-CAP-001` | Generated capability registry | `verified` | `foundation-hardening-1` | `capabilities/capabilities.toml`<br>`scripts/generate-capabilities.py` |
| `SYS-ARCH-001` | Architecture-specific code separated from boot orchestration | `verified` | `foundation-hardening-1` | `boot/uefi/src/arch/x86_64/`<br>`scripts/source-check.py` |
| `BOOT-INFO-001` | Versioned BootInfo v1 handoff | `verified` | `foundation-hardening-1` | `kernel/src/boot_info.rs`<br>`boot/uefi/src/main.rs` |
| `MEM-OWN-001` | Physical memory ownership map | `hardware_active` | `foundation-hardening-1` | `kernel/src/ownership.rs`<br>`kernel/src/memory.rs` |
| `MEM-PF-001` | Bitmap physical frame allocator with alloc and free | `hardware_active` | `foundation-hardening-1` | `kernel/src/memory.rs` |
| `MEM-PTB-001` | Dedicated page-table bootstrap pool | `hardware_active` | `foundation-hardening-1` | `kernel/src/memory.rs` |
| `MEM-VM-001` | Hardware four-level page-table manager | `acceptance_prototype` | `m5` | `kernel/src/paging.rs`<br>`boot/uefi/src/arch/x86_64/mod.rs` |
| `MEM-GUARD-001` | Hardware-unmapped guard pages | `software_model` | `m5` | `kernel/src/paging.rs` |
| `MEM-RECLAIM-001` | Boot-service memory reclaim | `software_model` | `m5` | `kernel/src/memory.rs` |
| `PROC-R3-001` | Ring 3 privilege transition | `hardware_active` | `m5` | `boot/uefi/src/arch/x86_64/mod.rs`<br>`user/programs/src/` |
| `PROC-AS-001` | Private process CR3 isolation | `software_model` | `m5` | `kernel/src/process.rs` |
| `SCHED-PRE-001` | Full register-context preemptive scheduling | `software_model` | `m5` | `kernel/src/process.rs`<br>`kernel/src/scheduler.rs` |
| `SYS-SYSCALL-001` | x86-64 syscall entry and return | `hardware_active` | `m5` | `boot/uefi/src/arch/x86_64/mod.rs`<br>`kernel/src/syscall.rs` |
| `EXEC-ELF-001` | ELF64 position-independent loader | `hardware_active` | `m5` | `kernel/src/elf.rs`<br>`user/programs/bin/` |
| `GFX-BOOT-001` | Graphical framebuffer startup splash | `planned` | `m6` | `assets/branding/sanjuos-logo.png` |
