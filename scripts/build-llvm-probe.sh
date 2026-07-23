#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

CLANG=${CLANG:-clang}
LLD_LINK=${LLD_LINK:-lld-link}

command -v "$CLANG" >/dev/null 2>&1 || {
  echo "error: clang is required" >&2
  exit 1
}
command -v "$LLD_LINK" >/dev/null 2>&1 || {
  echo "error: lld-link is required" >&2
  exit 1
}

rm -rf build/llvm-probe build/probe-esp
mkdir -p build/llvm-probe build/probe-esp/EFI/BOOT

"$CLANG" \
  --target=x86_64-pc-windows-msvc \
  -std=c17 \
  -Wall -Wextra -Werror \
  -ffreestanding \
  -fno-builtin \
  -fno-stack-protector \
  -fshort-wchar \
  -mno-red-zone \
  -O2 \
  -c verification/uefi-probe/main.c \
  -o build/llvm-probe/main.obj

"$LLD_LINK" \
  /machine:x64 \
  /subsystem:efi_application \
  /entry:efi_main \
  /nodefaultlib \
  /opt:ref \
  /out:build/probe-esp/EFI/BOOT/BOOTX64.EFI \
  build/llvm-probe/main.obj

printf 'Built %s\n' "$ROOT_DIR/build/probe-esp/EFI/BOOT/BOOTX64.EFI"
