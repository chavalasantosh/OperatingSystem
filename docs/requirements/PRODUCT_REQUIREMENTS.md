# Product Requirements — Initial Baseline

## Vision

Build an independent, secure, AI-native personal desktop operating system that eventually runs reliably on one explicitly supported laptop model before expanding to broader hardware.

## Target user

The first target user is the project owner: a developer working with AI, ML, MLOps, software engineering, media, documents, and everyday desktop applications.

## Product principles

1. The OS must remain independently bootable and must not use the Linux kernel.
2. The trusted computing base must be understandable and auditable.
3. Safe defaults, recovery, and data protection outrank visual polish.
4. Hardware support expands one validated device at a time.
5. AI functionality belongs in isolated user-space services, never in the privileged kernel.

## M0 acceptance criteria

- [x] Workspace separates firmware adapter from kernel core.
- [x] Boot artifact targets x86-64 UEFI.
- [x] Core contains no standard-library dependency in firmware builds.
- [x] Boot displays a deterministic milestone banner.
- [x] Architecture-independent behavior has host unit tests.
- [x] A headless QEMU smoke-test path exists.
- [x] Physical installation is explicitly blocked by policy.

## V1 product capabilities — long-term

- secure boot and measured boot strategy;
- protected virtual memory and process isolation;
- preemptive multitasking;
- capability-oriented access control;
- modern storage and filesystem reliability;
- networking, Wi-Fi, Bluetooth, audio, graphics, USB, and power management for the reference laptop;
- compositor, desktop shell, settings, file manager, terminal, package manager, browser strategy, and application SDK;
- atomic updates, rollback, recovery environment, and encrypted user data;
- sandboxed AI assistant and automation services.

## Non-goals for early milestones

- broad PC compatibility;
- Windows binary compatibility;
- gaming performance;
- production cryptography implemented from scratch;
- an app store;
- physical-disk installation before recovery gates are complete.
