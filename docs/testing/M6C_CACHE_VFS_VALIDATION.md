# M6C Cache and VFS Validation

## Acceptance target

Prove that the accepted virtio block device can be read through a bounded cache
and that RAMFS operates through bounded VFS path, mount, inode, directory, and
user-handle contracts without enabling persistent writes.

## Automated evidence

The pinned CI and local validation sequence is:

```bash
python3 scripts/generate-capabilities.py --check
python3 scripts/source-check.py
cargo fmt --all -- --check
cargo clippy -p sanju-kernel --all-targets -- -D warnings
cargo clippy -p sanju-boot --target x86_64-unknown-uefi -- -D warnings
cargo test -p sanju-kernel
bash scripts/smoke-test.sh
```

The headless QEMU gate must prove:

- the first cache-backed sector read records one miss and one device request;
- the repeat read records one hit without another device request;
- both returned sector images are identical;
- one in-range cache write is rejected;
- zero dirty entries remain;
- the root RAMFS mount is active behind VFS;
- canonical path normalization and traversal limits pass;
- a RAMFS file resolves and reads through an inode and user handle;
- a closed handle is rejected as stale;
- the complete M6B and earlier boot regressions remain passed;
- the shell exposes `cache`, `mounts`, `ls`, and `cat` through the accepted
  runtime.

## Unit boundaries

Kernel tests cover:

- cache hit, miss, device-read, and LRU-eviction accounting;
- write rejection before the backing device;
- zero-capacity and out-of-range failures;
- absolute path normalization, root-bounded `..`, and depth rejection;
- component-boundary longest-prefix mount resolution;
- RAMFS lookup and read through VFS;
- fixed handle capacity and stale-generation rejection.

## Safety boundary

M6C does not mount FAT32 and does not expose persistent writes. The only
writeable backend is volatile RAMFS. The dedicated QEMU storage image remains
disposable, and M6B still restores its one acceptance sector before M6C starts.
