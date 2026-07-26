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

## FH3 process-runtime attack surface

- process page-table construction and CR3 activation;
- effective `USER`, writable, and execute permissions across four levels;
- user and Ring 0 stack exhaustion;
- interrupt-frame save/restore order;
- timer selection of the next frame and address space;
- TSS `RSP0` and syscall-stack ownership;
- stale page-table frames after process exit.

## FH3 controls

- every inherited user bit is stripped from private clones;
- only declared process ranges are promoted to Ring 3;
- user W^X policy is preserved across identity and direct-map aliases;
- lower and upper hardware guard holes surround both process stacks;
- the interrupt-frame layout is frozen and host-validated;
- register sentinels are checked across live timer preemption;
- CR3 and Ring 0 stack selection change as one scheduler operation;
- live roots cannot be reclaimed;
- exact frame inventories must return the bootstrap pool to its prior count;
- M5 user-fault recovery is rerun under private CR3 roots.

## Security gates before physical installation

- verified boot artifact provenance;
- signed-boot plan;
- disk-write protection and explicit device selection;
- backup and recovery validation;
- rollback-capable updates;
- storage fuzzing and power-loss tests;
- reference-laptop threat review.
