#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

command -v qemu-system-x86_64 >/dev/null 2>&1 || {
  echo "error: qemu-system-x86_64 is required" >&2
  exit 1
}

./scripts/build.sh
OVMF=$(./scripts/find-ovmf.sh)

exec qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -cpu max \
  -m 256M \
  -bios "$OVMF" \
  -drive format=raw,file=fat:rw:build/esp \
  -serial stdio \
  -no-reboot
