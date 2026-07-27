#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

command -v qemu-system-x86_64 >/dev/null 2>&1 || {
  echo "error: qemu-system-x86_64 is required" >&2
  exit 1
}

bash ./scripts/build-smoke.sh
OVMF_CODE=$(bash ./scripts/find-ovmf.sh)
OVMF_VARS_TEMPLATE="${OVMF_CODE/OVMF_CODE/OVMF_VARS}"

if [[ ! -f "$OVMF_VARS_TEMPLATE" ]]; then
    echo "error: OVMF variables file not found: $OVMF_VARS_TEMPLATE" >&2
    exit 1
fi

OVMF_VARS_COPY="$(mktemp /tmp/sanjuos-ovmf-vars.XXXXXX.fd)"
STORAGE_IMAGE="$(mktemp /tmp/sanjuos-storage.XXXXXX.img)"
trap 'rm -f "$OVMF_VARS_COPY" "$STORAGE_IMAGE"' EXIT
cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS_COPY"
truncate -s 8M "$STORAGE_IMAGE"
printf '%s' 'SANJUOS-M6B-READ-PATTERN' |
  dd of="$STORAGE_IMAGE" bs=1 seek=$((8 * 512)) conv=notrunc status=none
mkdir -p build
rm -f build/qemu-debug.log

set +e
timeout 20s qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -cpu max \
  -m 256M \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS_COPY" \
  -drive format=raw,file=fat:rw:build/smoke-esp \
  -drive if=none,id=sanju-storage,format=raw,file="$STORAGE_IMAGE" \
  -device virtio-blk-pci,drive=sanju-storage,serial=SANJU-M6B \
  -display none \
  -serial none \
  -monitor none \
  -debugcon file:build/qemu-debug.log \
  -global isa-debugcon.iobase=0xe9 \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
  -no-reboot \
  -no-shutdown
qemu_status=$?
set -e

# isa-debug-exit returns (value << 1) | 1. Success value 0x10 => 33.
if [[ "$qemu_status" -ne 33 ]]; then
  echo "error: QEMU exited with status $qemu_status" >&2
  [[ -f build/qemu-debug.log ]] && cat build/qemu-debug.log >&2
  exit 1
fi

grep -Fq "Soma OS M5 boot transition" build/qemu-debug.log
grep -Fq "Milestone M5: protected user-space foundation and branded startup." build/qemu-debug.log
grep -Fq "init: Soma OS protected userspace online" build/qemu-debug.log
grep -Fq "hello: running from Soma OS Ring 3" build/qemu-debug.log
grep -Fq "Soma OS: isolated user exception" build/qemu-debug.log
grep -Fq "Soma OS page-table ownership: active" build/qemu-debug.log
grep -Fq "Kernel heap: active" build/qemu-debug.log
grep -Fq "Ring 3 execution: active" build/qemu-debug.log
grep -Fq "User address-space model: active" build/qemu-debug.log
grep -Fq "System-call interface: active" build/qemu-debug.log
grep -Fq "ELF64 loader: active" build/qemu-debug.log
grep -Fq "User processes launched: 3" build/qemu-debug.log
grep -Fq "User fault isolation: passed" build/qemu-debug.log
grep -Fq "Soma OS logo print: active" build/qemu-debug.log
grep -Fq "M5 protected user-space gate: passed" build/qemu-debug.log
while IFS= read -r expected_line; do
  [[ -z "$expected_line" || "$expected_line" == \#* ]] && continue
  grep -Fq "$expected_line" build/qemu-debug.log
done < capabilities/smoke-expectations.txt
grep -Fq "Reserved-range overlap test: passed" build/qemu-debug.log
grep -Fq "Double-free detection: passed" build/qemu-debug.log
grep -Fq "Reserved-frame protection: passed" build/qemu-debug.log
grep -Fq "M5 regression boot: passed" build/qemu-debug.log
grep -Fq "Foundation hardening phase 1: passed" build/qemu-debug.log
grep -Fq "Inherited firmware page tables: retired" build/qemu-debug.log
grep -Fq "Page map/unmap test: passed" build/qemu-debug.log
grep -Fq "Page translation test: passed" build/qemu-debug.log
grep -Fq "Page protection test: passed" build/qemu-debug.log
grep -Fq "CR3 transition checkpoint: passed" build/qemu-debug.log
grep -Fq "Interrupts after CR3 switch: passed" build/qemu-debug.log
grep -Fq "Foundation hardening phase 2: passed" build/qemu-debug.log
grep -Fq "Private M5 address spaces: 3" build/qemu-debug.log
grep -Fq "Ring 3 preemption processes: 2" build/qemu-debug.log
grep -Fq "M5 regression under private CR3: passed" build/qemu-debug.log
grep -Fq "FH2 paging regression under FH3: passed" build/qemu-debug.log
grep -Fq "Foundation hardening phase 3: passed" build/qemu-debug.log
grep -Fq "Soma OS M6A PCI and Storage Discovery" build/qemu-debug.log
grep -Fq "PCI configuration mechanism #1: active" build/qemu-debug.log
grep -Fq "PCI inventory completeness: active" build/qemu-debug.log
grep -Fq "Virtio block PCI target: active" build/qemu-debug.log
grep -Fq "Storage driver target: virtio-blk-pci" build/qemu-debug.log
grep -Fq "FH3 regression under M6A: passed" build/qemu-debug.log
grep -Fq "M6A PCI discovery gate: passed" build/qemu-debug.log
grep -Fq "Soma OS M6B Virtio Block Transport" build/qemu-debug.log
grep -Fq "Architecture-independent block-device API: active" build/qemu-debug.log
grep -Fq "Modern virtio PCI capabilities: active" build/qemu-debug.log
grep -Fq "PCI bus mastering: active" build/qemu-debug.log
grep -Fq "Virtio feature negotiation: active" build/qemu-debug.log
grep -Fq "DMA-safe split virtqueue: active" build/qemu-debug.log
grep -Fq "Dedicated storage identity: verified" build/qemu-debug.log
grep -Fq "Known sector read test: passed" build/qemu-debug.log
grep -Fq "Disposable sector write/readback test: passed" build/qemu-debug.log
grep -Fq "Disposable sector restoration: passed" build/qemu-debug.log
grep -Fq "Block bounds rejection test: passed" build/qemu-debug.log
grep -Fq "Block request timeout protection: active" build/qemu-debug.log
grep -Fq "M6A regression under M6B: passed" build/qemu-debug.log
grep -Fq "M6B block transport gate: passed" build/qemu-debug.log
grep -Fq "Soma OS M6C Cache and Virtual Filesystem" build/qemu-debug.log
grep -Fq "Fixed-capacity block cache: active" build/qemu-debug.log
grep -Fq "Cache first-read miss test: passed" build/qemu-debug.log
grep -Fq "Cache repeat-read hit test: passed" build/qemu-debug.log
grep -Fq "Cached data consistency test: passed" build/qemu-debug.log
grep -Fq "Read-only dirty-state policy: active" build/qemu-debug.log
grep -Fq "Dirty cache entries: 0" build/qemu-debug.log
grep -Fq "VFS contracts: active" build/qemu-debug.log
grep -Fq "Bounded mount table: active" build/qemu-debug.log
grep -Fq "RAMFS VFS adapter: active" build/qemu-debug.log
grep -Fq "Absolute-path normalization test: passed" build/qemu-debug.log
grep -Fq "Path traversal bounds test: passed" build/qemu-debug.log
grep -Fq "Generation-protected user handle table: active" build/qemu-debug.log
grep -Fq "Stale file-handle rejection test: passed" build/qemu-debug.log
grep -Fq "Persistent storage writes: disabled" build/qemu-debug.log
grep -Fq "M6B regression under M6C: passed" build/qemu-debug.log
grep -Fq "M6C cache and VFS gate: passed" build/qemu-debug.log
grep -Fq "Soma OS kernel shell ready." build/qemu-debug.log
grep -Fq "M5 protected userspace, syscalls, and ELF loader are active." build/qemu-debug.log
grep -Fq "virtio-blk targets: 1" build/qemu-debug.log
grep -Fq "write/readback passed" build/qemu-debug.log
grep -Fq "Block cache: 16 sectors, hits 1, misses 1, device reads 1, dirty 0, policy read-only" build/qemu-debug.log
grep -Fq "VFS mounts: 1, handle capacity: 32, normalized paths: active" build/qemu-debug.log
grep -Fq "/ ramfs read-write" build/qemu-debug.log
grep -Fq "Welcome to Soma OS." build/qemu-debug.log

echo "QEMU smoke test passed."
cat build/qemu-debug.log
