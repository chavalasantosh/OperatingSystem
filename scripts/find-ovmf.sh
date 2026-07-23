#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${OVMF_CODE:-}" && -f "${OVMF_CODE}" ]]; then
  printf '%s\n' "$OVMF_CODE"
  exit 0
fi

candidates=(
  /usr/share/OVMF/OVMF_CODE.fd
  /usr/share/OVMF/OVMF_CODE_4M.fd
  /usr/share/edk2/x64/OVMF_CODE.fd
  /usr/share/qemu/OVMF_CODE.fd
  /opt/homebrew/share/qemu/edk2-x86_64-code.fd
  /usr/local/share/qemu/edk2-x86_64-code.fd
)

for candidate in "${candidates[@]}"; do
  if [[ -f "$candidate" ]]; then
    printf '%s\n' "$candidate"
    exit 0
  fi
done

echo "error: OVMF firmware was not found. Install the 'ovmf' package or set OVMF_CODE." >&2
exit 1
