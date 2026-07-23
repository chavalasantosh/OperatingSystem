# Threat Model — Foundation

## Assets

- boot integrity;
- kernel control flow and memory isolation;
- user data;
- cryptographic keys;
- update authenticity;
- recovery capability;
- audit records.

## Initial adversaries

- malformed firmware data;
- corrupted boot media;
- malicious packages or updates;
- compromised user-space applications;
- hostile network input;
- unsafe driver behavior;
- supply-chain compromise.

## M0 attack surface

- UEFI system-table pointer;
- UEFI console protocol pointer and function table;
- generated EFI executable;
- build toolchain and CI workflow.

## M0 controls

- null and signature validation before firmware-table use;
- no dynamic allocation;
- no parsing of external files;
- no network stack;
- dependency-free boot path;
- documented unsafe invariants;
- QEMU-only deployment policy.

## Security gates before physical installation

- verified boot artifact provenance;
- signed-boot plan;
- disk-write protection and explicit device selection;
- backup and recovery validation;
- rollback-capable updates;
- storage fuzzing and power-loss tests;
- reference-laptop threat review.
