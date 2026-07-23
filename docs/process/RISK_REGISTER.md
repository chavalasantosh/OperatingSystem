# Risk Register

| ID | Risk | Probability | Impact | Current mitigation |
|---|---|---:|---:|---|
| R-001 | Scope expands toward macOS parity too early | High | Critical | milestone contracts and explicit non-goals |
| R-002 | Hardware documentation is unavailable | High | High | choose one documented reference laptop later |
| R-003 | Unsafe Rust introduces memory corruption | Medium | Critical | narrow unsafe boundaries, comments, tests, review |
| R-004 | Toolchain/API churn breaks builds | Medium | Medium | stable Rust, pinned CI policy, controlled upgrades |
| R-005 | Storage bug destroys user data | Medium | Critical | emulator-only policy until recovery and fuzz gates |
| R-006 | Dependency compromise enters trusted base | Medium | Critical | dependency ADR and minimal boot dependencies |
| R-007 | Project owner becomes blocked by low-level complexity | High | High | small demonstrable increments and documented learning path |
| R-008 | Visual work distracts from kernel reliability | High | Medium | graphics begins only after memory/process foundations |
