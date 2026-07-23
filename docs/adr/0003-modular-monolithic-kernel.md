# ADR 0003: Modular monolithic kernel for the first usable system

- Status: Accepted
- Date: 2026-07-21

## Context

A small team needs a realistic path to scheduling, storage, graphics, and device support without the IPC and debugging cost of an immediate full microkernel.

## Decision

Begin with a modular monolithic kernel. Subsystems must communicate through explicit interfaces, avoid global mutable state, and preserve ownership boundaries. High-risk or policy-heavy services may migrate to user space later.

## Consequences

- early driver failures can still compromise the kernel;
- performance and implementation speed are favorable;
- module boundaries must be enforced through code structure and review;
- isolation remains an explicit roadmap item, not an assumed property.
