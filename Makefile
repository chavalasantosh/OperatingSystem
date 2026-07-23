SHELL := /usr/bin/env bash

.PHONY: help setup source-check fmt lint test build probe verify-probe image run smoke clean

help:
	@printf '%s\n' \
	  'SanjuOS developer commands:' \
	  '  make setup   Install the Rust target used by the UEFI loader' \
	  '  make source-check  Validate critical source and UEFI ABI invariants' \
	  '  make fmt     Check formatting' \
	  '  make lint    Run Clippy quality gates' \
	  '  make test    Run host-side kernel tests' \
	  '  make build   Build the Rust UEFI boot artifact' \
	  '  make probe   Build the LLVM UEFI verification probe' \
	  '  make verify-probe  Validate the probe PE/COFF contract' \
	  '  make image   Create a bootable FAT disk image' \
	  '  make run     Boot interactively in QEMU + OVMF' \
	  '  make smoke   Headless QEMU boot test' \
	  '  make clean   Remove generated artifacts'

setup:
	./scripts/setup.sh

source-check:
	python3 ./scripts/source-check.py

fmt:
	cargo fmt --all -- --check

lint:
	cargo clippy -p sanju-kernel --all-targets -- -D warnings
	cargo clippy -p sanju-boot --target x86_64-unknown-uefi -- -D warnings

test:
	cargo test -p sanju-kernel

build:
	./scripts/build.sh

probe:
	./scripts/build-llvm-probe.sh

verify-probe:
	./scripts/verify-llvm-probe.sh

image:
	./scripts/image.sh

run:
	./scripts/run-qemu.sh

smoke:
	./scripts/smoke-test.sh

clean:
	rm -rf build target
