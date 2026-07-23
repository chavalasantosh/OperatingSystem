# ADR 0004: Minimal boot-path dependencies

- Status: Accepted
- Date: 2026-07-21

## Context

Boot code executes at the highest privilege and is difficult to recover when broken. Every dependency expands the trusted computing base and supply-chain surface.

## Decision

M0 uses no third-party Rust dependency in the boot path. Dependencies may be introduced only with an ADR covering maintenance, licensing, security, `unsafe` usage, and replacement cost.

## Consequences

- more firmware ABI definitions are maintained locally;
- early progress can be slower;
- the trusted base is small and auditable;
- a mature UEFI crate may be adopted later after review.
