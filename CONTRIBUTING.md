# Contributing

## Branch and review policy

- `main` must remain bootable and pass all quality gates.
- Work occurs on short-lived branches named `type/issue-summary`.
- Every behavior change requires an issue, acceptance criteria, tests, and documentation updates.
- At least one review is required before merge once a second maintainer exists.
- Force-pushes to `main` are forbidden.

## Commit convention

Use Conventional Commit prefixes:

- `feat:` new behavior;
- `fix:` defect correction;
- `refactor:` structure without behavior change;
- `test:` test-only change;
- `docs:` documentation-only change;
- `build:` build or dependency change;
- `ci:` automation change;
- `security:` hardening or vulnerability correction.

## Rust rules

- Stable Rust is the default toolchain.
- Rust 2024 edition is used for new crates.
- `unsafe` is permitted only at hardware, ABI, or ownership boundaries.
- Every `unsafe` block must have a nearby `SAFETY:` comment describing the invariant.
- Panics are forbidden as ordinary error handling in kernel paths.
- Hidden allocation is not permitted before the allocator milestone.
- New third-party dependencies require an ADR and supply-chain review.

## Required local checks

```bash
make fmt
make lint
make test
make smoke
```
