#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

command -v qemu-system-x86_64 >/dev/null 2>&1 || {
  echo "error: qemu-system-x86_64 is required" >&2
  exit 1
}

./scripts/build-smoke.sh
OVMF=$(./scripts/find-ovmf.sh)
mkdir -p build
rm -f build/qemu-debug.log

set +e
qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -cpu max \
  -m 256M \
  -bios "$OVMF" \
  -drive format=raw,file=fat:rw:build/smoke-esp \
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

grep -Fq "Milestone M1: firmware exit and kernel ownership." build/qemu-debug.log
grep -Fq "Architecture: x86_64" build/qemu-debug.log
grep -Fq "Kernel ownership gate: passed" build/qemu-debug.log

echo "QEMU smoke test passed."
cat build/qemu-debug.log
