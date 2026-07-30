# M6D Read-Only FAT32 Validation

## Purpose

Prove that Soma OS mounts a deterministic FAT32 volume through the accepted
virtio block, cache, and VFS layers; reads persistent data correctly; rejects
corrupt chains; and never creates persistent dirty state.

## Mechanical gates

```bash
python3 scripts/generate-capabilities.py --check
python3 scripts/source-check.py
cargo fmt --all -- --check
cargo test -p sanju-kernel
cargo clippy -p sanju-kernel --all-targets -- -D warnings
cargo clippy -p sanju-boot --target x86_64-unknown-uefi -- -D warnings
cargo build -p sanju-boot --release --target x86_64-unknown-uefi
bash scripts/smoke-test.sh
```

## Host coverage

- valid BPB, FSInfo, complete backup boot, FAT capacity, and root-cluster mount;
- invalid signature and inconsistent backup rejection;
- multi-cluster and offset reads;
- malformed cyclic-chain rejection;
- checksum-validated long-filename lookup;
- secondary VFS mount dispatch;
- streaming shell reads beyond one sector;
- VFS and backend write rejection.

## QEMU acceptance evidence

- M5, FH1, FH2, FH3, M6A, M6B, and M6C gates remain passed;
- the deterministic 131072-sector FAT32 image is the identified virtio device;
- FAT32 reports 512-byte sectors, one sector per cluster, and 129022 data
  clusters;
- FSInfo and backup boot validation pass;
- `/disk` mounts as the second VFS filesystem;
- root, persistent-file, long-name, nested-directory, and multi-cluster reads
  pass;
- FAT32 writes are blocked;
- the cache reports zero dirty entries after all persistent reads;
- shell `mounts`, `fat32`, `ls /disk`, `ls /disk/docs`, and
  `cat /disk/README.TXT` expose the accepted state.

## Boundary

M6D does not provide process-facing persistent file descriptors, executable
loading from FAT32, partition discovery, writable FAT32, recovery, encryption,
or physical-disk installation. The accepted environment remains QEMU-only.
