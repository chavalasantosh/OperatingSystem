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
OVMF_VARS_COPY="$(mktemp /tmp/sanjuos-ovmf-vars.XXXXXX.fd)"

if [[ ! -f "$OVMF_VARS_TEMPLATE" ]]; then
    echo "error: OVMF variables file not found: $OVMF_VARS_TEMPLATE" >&2
    exit 1
fi

cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS_COPY"
trap 'rm -f "$OVMF_VARS_COPY"' EXIT
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

grep -Fq "Milestone M4: interrupt-driven runtime and interactive kernel environment." build/qemu-debug.log
grep -Fq "Protected kernel stack: active" build/qemu-debug.log
grep -Fq "IDT exception handling: active" build/qemu-debug.log
grep -Fq "PIT timer interrupts: active" build/qemu-debug.log
grep -Fq "PS/2 keyboard interrupt path: active" build/qemu-debug.log
grep -Fq "Round-robin scheduler: active" build/qemu-debug.log
grep -Fq "Interactive kernel shell: active" build/qemu-debug.log
grep -Fq "RAM filesystem: active" build/qemu-debug.log
grep -Fq "M4 interactive runtime gate: passed" build/qemu-debug.log
grep -Fq "SanjuOS kernel shell ready." build/qemu-debug.log
grep -Fq "welcome.txt" build/qemu-debug.log
grep -Fq "runtime-ok" build/qemu-debug.log

echo "QEMU smoke test passed."
cat build/qemu-debug.log
