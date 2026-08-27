# Project state

## Current phase

Phase 0.5 — Validation hardening. No product feature expansion.

## Completed

- Phase 0 reusable Rust core, Shell desktop discovery, versioned local snapshots, diff/matching, guarded restore, thin Tauri commands, and minimal React UI.
- Bounded settle verification with configurable polling interval, total deadline, and required consecutive exact full-desktop observations.
- Explicit restore outcomes for Shell positioning failure, immediate verification failure, later settle failure, settled success, display blocking, unresolved items, and nothing-to-restore.
- Developer-only destructive verification harness using two fixed, pre-created fixtures and no random-item fallback.
- Persistent recovery guard written before mutation, RAII recovery for unwind/ordinary error, durable crash marker, retained completion evidence, and `recover-last-verification` command.
- Manual Windows verification matrix and VM operating instructions.

## Verified

- Pure settle-state behavior: immediate success, retry success, deadline failure, later drift, display mismatch, and missing/new items.
- Snapshot/diff/matching/storage and recovery-record persistence/archive logic through non-destructive tests.
- Host validation passed: Rust formatting, workspace clippy with warnings denied, 20 Rust tests, frontend lint/typecheck/test/build, and the no-bundle Tauri production build.
- `cargo test --workspace` reported both real-Explorer integration tests as ignored. A separate non-destructive guard preflight confirmed that opt-in value `0` is rejected before capture or mutation.
- Phase 0 historical Windows 11 feasibility evidence remains documented separately.

The Phase 0.5 destructive verification harness is implemented but awaits execution in an isolated Windows 11 VM. It was not run on the development host.

## Known limitations

- The new harness has not yet been exercised against a real Explorer desktop; its destructive path and RAII recovery behavior await VM validation.
- A process kill, abort, VM crash, or power loss cannot execute RAII recovery. The persistent active record enables a later explicit recovery attempt but cannot guarantee the external environment is unchanged.
- Settle success is bounded evidence, not proof that Explorer can never rearrange after the deadline. Default timings require matrix validation across DPI, Explorer settings, and Windows builds.
- Recovery blocks on a changed display signature or any missing, new, ambiguous, or moved item that prevents an exact final diff.
- Fixture preparation is manual by design. Duplicate fixture basenames across merged desktop locations are rejected.
- Windows 10, multi-display, mixed-DPI, Explorer restart, RDP, Auto Arrange, and Align to Grid remain unverified.

## Next step

Run the destructive fixed-fixture round-trip in the existing isolated Windows 11 VM, retain its console/recovery evidence, and update only the corresponding manual-test-matrix row from the observed result.
