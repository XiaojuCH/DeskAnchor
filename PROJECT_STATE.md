# Project state

## Current phase

Phase 0.5B — Record isolated VM evidence and harden persistent recovery validation. No product feature expansion.

## Completed

- Phase 0 reusable Rust core, supported Shell desktop discovery, versioned local snapshots, exact diff/matching, guarded restore, thin Tauri commands, and minimal React UI.
- Bounded settle verification with configurable polling interval, total deadline, and required consecutive exact full-desktop observations.
- Explicit restore outcomes for Shell positioning failure, immediate verification failure, later settle failure, settled success, display blocking, unresolved items, and nothing-to-restore.
- Developer-only fixed-fixture destructive verification harness with an identity allowlist and no random-item fallback.
- Persistent pre-mutation recovery guard, retained completion evidence, RAII recovery for unwind/ordinary error, and `recover-last-verification` crash-recovery command.
- Developer-only `after-mutation` controlled recovery failpoint. It requires both explicit environment gates, disarms RAII only after mutation settles, leaves the active recovery record in place, and returns `RECOVERY_REQUIRED` without crashing the process.
- Manual Windows verification matrix, VM instructions, and the first Phase 0.5 baseline evidence record.

## Verified

- The fixed-fixture destructive harness passed on an isolated Windows 11 Pro 23H2 build 22631 AMD64 VM with one 2283×1278 virtual display at 100% scale and six desktop items.
- Mutation passed, settle passed, recovery used three observations over 323 ms, and the final full diff was exact: six unchanged, zero moved/missing/new/ambiguous.
- Normal recovery completion was confirmed: the active marker was removed only after verified evidence was archived as `verification-20260827T1059529044508Z-10848-verified.json`.
- Pure settle-state behavior, snapshot/diff/matching/storage, recovery-record persistence/archive, failpoint parsing, fail-closed unknown values, opt-in enforcement, and recovery-guard state transitions pass non-destructive tests.
- Host validation passes Rust formatting, clippy with warnings denied, 25 Rust tests, frontend lint/typecheck/test/build, and the no-bundle Tauri production build. Both real-Explorer integration tests remain ignored on the host.

The VM's legacy `Get-ComputerInfo` product-name string reported `Windows 10 Pro`, but build 22631 and the actual installed system are Windows 11 Pro 23H2. The evidence is recorded as Windows 11, not Windows 10.

This is baseline evidence from one isolated VM configuration. It is not a general Windows 11 or display-configuration support claim.

## Known limitations

- The new `after-mutation` controlled recovery state and subsequent `recover-last-verification` path have not yet been executed in the VM.
- Controlled orphan mode validates the persistent recovery workflow without using panic, abort, or process termination. It does not reproduce every real crash timing or failure mode.
- A process kill, abort, VM crash, or power loss cannot execute RAII recovery. Recovery still depends on the persisted active record and an environment compatible with the original snapshot.
- Settle success is bounded evidence, not proof that Explorer can never rearrange after the deadline.
- Recovery blocks on a changed display signature or any missing, new, ambiguous, or moved item that prevents an exact final diff.
- Fixture preparation is manual. Duplicate fixture basenames across merged desktop locations are rejected.
- 125%/150%/200% DPI, physical and multiple displays, mixed DPI, Explorer restart, RDP, Auto Arrange, Align to Grid, and Windows 10 remain unverified.

## Next step

Run the `after-mutation` controlled recovery failpoint in the existing isolated Windows 11 VM, confirm the swapped fixture state and active marker, then execute `recover-last-verification` and record the final exact recovery evidence.
