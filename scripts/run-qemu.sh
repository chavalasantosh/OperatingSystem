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
OVMF_VARS_COPY="$(mktemp /tmp/sanjuos-ovmf-vars.XXXXXX.fd)"

if [[ ! -f "$OVMF_VARS_TEMPLATE" ]]; then
  echo "error: OVMF variables file not found: $OVMF_VARS_TEMPLATE" >&2
  exit 1
fi

cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS_COPY"
trap 'rm -f "$OVMF_VARS_COPY"' EXIT

qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -cpu max \
  -m 256M \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS_COPY" \
  -drive format=raw,file=fat:rw:build/esp \
  -serial stdio \
  -no-reboot \
  -no-shutdown
