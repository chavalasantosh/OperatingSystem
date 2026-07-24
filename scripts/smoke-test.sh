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

grep -Fq "SanjuOS M5 boot transition" build/qemu-debug.log
grep -Fq "Milestone M5: protected user-space foundation and branded startup." build/qemu-debug.log
grep -Fq "init: SanjuOS protected userspace online" build/qemu-debug.log
grep -Fq "hello: running from SanjuOS Ring 3" build/qemu-debug.log
grep -Fq "SanjuOS: isolated user exception" build/qemu-debug.log
grep -Fq "Inherited page-table root captured: active" build/qemu-debug.log
grep -Fq "Kernel heap: active" build/qemu-debug.log
grep -Fq "Ring 3 execution: active" build/qemu-debug.log
grep -Fq "User address-space model: active" build/qemu-debug.log
grep -Fq "System-call interface: active" build/qemu-debug.log
grep -Fq "ELF64 loader: active" build/qemu-debug.log
grep -Fq "User processes launched: 3" build/qemu-debug.log
grep -Fq "User fault isolation: passed" build/qemu-debug.log
grep -Fq "SanjuOS logo print: active" build/qemu-debug.log
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
grep -Fq "SanjuOS kernel shell ready." build/qemu-debug.log
grep -Fq "M5 protected userspace, syscalls, and ELF loader are active." build/qemu-debug.log

echo "QEMU smoke test passed."
cat build/qemu-debug.log
