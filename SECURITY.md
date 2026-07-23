# Security Policy

SanjuOS is pre-alpha research software and must not be used to protect sensitive data or replace a production operating system.

## Reporting

Until a private security channel is established, do not publish exploit details. Record the issue privately with the project owner and include:

- affected commit;
- threat scenario;
- reproduction steps;
- impact;
- suggested mitigation.

## Secure-development baseline

- emulator-first testing;
- deny-by-default permissions in future user space;
- signed update design before physical deployment;
- explicit trust boundaries;
- no unaudited dependencies in the boot path;
- documented `unsafe` invariants;
- fuzzing for parsers before untrusted input is accepted;
- rollback and recovery requirements before installer work.
