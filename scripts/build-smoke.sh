#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

cargo build \
  --locked \
  --release \
  --package sanju-boot \
  --target x86_64-unknown-uefi \
  --features qemu-test

rm -rf build/smoke-esp
mkdir -p build/smoke-esp/EFI/BOOT
cp target/x86_64-unknown-uefi/release/sanju-boot.efi \
   build/smoke-esp/EFI/BOOT/BOOTX64.EFI
