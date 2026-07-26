#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

command -v qemu-system-x86_64 >/dev/null 2>&1 || {
  echo "error: qemu-system-x86_64 is required" >&2
  exit 1
}

bash ./scripts/build.sh
OVMF_CODE=$(bash ./scripts/find-ovmf.sh)
OVMF_VARS_TEMPLATE="${OVMF_CODE/OVMF_CODE/OVMF_VARS}"

if [[ ! -f "$OVMF_VARS_TEMPLATE" ]]; then
  echo "error: OVMF variables file not found: $OVMF_VARS_TEMPLATE" >&2
  exit 1
fi

OVMF_VARS_COPY="$(mktemp /tmp/sanjuos-ovmf-vars.XXXXXX.fd)"
trap 'rm -f "$OVMF_VARS_COPY"' EXIT
cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS_COPY"
mkdir -p build
if [[ ! -f build/sanju-storage.img ]]; then
  truncate -s 64M build/sanju-storage.img
fi

qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -cpu max \
  -m 256M \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS_COPY" \
  -drive format=raw,file=fat:rw:build/esp \
  -drive if=none,id=sanju-storage,format=raw,file=build/sanju-storage.img \
  -device virtio-blk-pci,drive=sanju-storage,serial=SANJU-M6A \
  -serial stdio \
  -no-reboot \
  -no-shutdown
