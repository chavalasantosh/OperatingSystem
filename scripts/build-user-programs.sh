#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

CLANG=${CLANG:-clang}
LD_LLD=${LD_LLD:-ld.lld}

command -v "$CLANG" >/dev/null 2>&1 || {
  echo "error: clang is required to build M5 user programs" >&2
  exit 1
}
command -v "$LD_LLD" >/dev/null 2>&1 || {
  echo "error: ld.lld is required to build M5 user programs" >&2
  exit 1
}

mkdir -p build/user-programs user/programs/bin

for program in init hello fault-test; do
  "$CLANG" \
    -target x86_64-unknown-none \
    -fPIC \
    -c "user/programs/src/${program}.S" \
    -o "build/user-programs/${program}.o"

  "$LD_LLD" \
    -pie \
    --no-dynamic-linker \
    -nostdlib \
    -e _start \
    --build-id=none \
    -z noseparate-code \
    "build/user-programs/${program}.o" \
    -o "user/programs/bin/${program}.elf"
done

echo "Built SanjuOS M5 user programs."
