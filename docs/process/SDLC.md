# Delivery Lifecycle

SanjuOS uses Agile delivery inside a gated systems-engineering SDLC.

## Lifecycle stages

1. Discovery and product requirements.
2. Architecture and threat modeling.
3. Sprint planning.
4. Implementation with local verification.
5. Automated quality and emulator integration tests.
6. Architecture/security review for affected boundaries.
7. Milestone demonstration.
8. Release candidate, recovery validation, and retrospective.
9. Maintenance and incident learning.

## Sprint cadence

- Two-week sprints.
- One demonstrable technical increment per sprint.
- Backlog refinement once per sprint.
- Architecture Decision Records whenever a durable technical choice changes.
- No velocity target may override safety or correctness gates.

## Milestone gates

A milestone cannot close until acceptance criteria, tests, documentation, known risks, and rollback/recovery implications are recorded.
