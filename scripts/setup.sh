#!/usr/bin/env bash
set -euo pipefail

command -v rustup >/dev/null 2>&1 || {
  echo "error: rustup is required. Install it from https://rustup.rs" >&2
  exit 1
}

rustup toolchain install 1.97.0 \
  --profile minimal \
  --component rustfmt \
  --component clippy \
  --target x86_64-unknown-uefi
rustup override set 1.97.0
rustup show active-toolchain

printf '%s\n' "SanjuOS Rust 1.97.0 toolchain is ready."
