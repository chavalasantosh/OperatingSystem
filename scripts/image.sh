#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

for command_name in dd mkfs.vfat mmd mcopy; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "error: '$command_name' is required (install dosfstools and mtools)" >&2
    exit 1
  }
done

./scripts/build.sh
rm -f build/sanju-os-m1-alpha.img

dd if=/dev/zero of=build/sanju-os-m1-alpha.img bs=1M count=64 status=none
mkfs.vfat -n SANJUOS build/sanju-os-m1-alpha.img >/dev/null
mmd -i build/sanju-os-m1-alpha.img ::/EFI ::/EFI/BOOT
mcopy -i build/sanju-os-m1-alpha.img \
  build/esp/EFI/BOOT/BOOTX64.EFI \
  ::/EFI/BOOT/BOOTX64.EFI

echo "Created build/sanju-os-m1-alpha.img"
