#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

cargo build \
  --locked \
  --release \
  --package sanju-boot \
  --target x86_64-unknown-uefi

mkdir -p build/esp/EFI/BOOT
cp target/x86_64-unknown-uefi/release/sanju-boot.efi \
   build/esp/EFI/BOOT/BOOTX64.EFI

printf 'Built %s\n' "$ROOT_DIR/build/esp/EFI/BOOT/BOOTX64.EFI"
