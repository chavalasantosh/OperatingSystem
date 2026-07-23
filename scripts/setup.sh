#!/usr/bin/env bash
set -euo pipefail

command -v rustup >/dev/null 2>&1 || {
  echo "error: rustup is required. Install it from https://rustup.rs" >&2
  exit 1
}

rustup show active-toolchain
rustup target add x86_64-unknown-uefi

echo "SanjuOS Rust toolchain is ready."
