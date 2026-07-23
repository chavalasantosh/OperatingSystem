#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

./scripts/build-llvm-probe.sh
artifact=build/probe-esp/EFI/BOOT/BOOTX64.EFI

file_output=$(file "$artifact")
headers=$(llvm-objdump -p "$artifact")
strings_output=$(strings "$artifact")

[[ "$file_output" == *"PE32+ executable for EFI (application), x86-64"* ]]
grep -Fq "Subsystem               0000000a" <<<"$headers"
grep -Fq "AddressOfEntryPoint     0000000000001000" <<<"$headers"
grep -Fq "SanjuOS LLVM UEFI verification probe" <<<"$strings_output"
grep -Fq "Kernel ownership gate: passed" <<<"$strings_output"

printf '%s\n' "LLVM UEFI probe verification passed."
file "$artifact"
sha256sum "$artifact"
