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

## M6 storage attack surface

- malformed or cyclic PCI bridge topology;
- concurrent PCI configuration-port access;
- untrusted BAR and capability-list metadata;
- device-controlled DMA addresses and lengths;
- malformed sector geometry, partition tables, FAT metadata, and cluster chains;
- accidental selection of the EFI system partition or a physical user disk.

## M6 controls

- bounded PCI device and bus inventories fail closed on overflow;
- x86 configuration transactions are serialized on the bootstrap CPU;
- M6A performs discovery only and cannot issue block requests;
- M6B requires the dedicated QEMU disk identity before its acceptance I/O;
- M6B uses allocator-owned DMA memory, fixed descriptor chains, sector bounds,
  device status checks, and a finite polling limit;
- the sole M6B write target is a disposable sector whose original bytes are
  restored before the acceptance gate passes;
- filesystem work begins read-only with geometry and bounds validation;
- persistent writes remain blocked until reboot, corruption, and recovery gates
  are implemented.

## Security gates before physical installation

- verified boot artifact provenance;
- signed-boot plan;
- disk-write protection and explicit device selection;
- backup and recovery validation;
- rollback-capable updates;
- storage fuzzing and power-loss tests;
- reference-laptop threat review.
