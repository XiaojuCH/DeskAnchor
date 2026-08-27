# Project state

## Current phase

Phase 0.5B — Remediate the independent validation blockers in Draft PR #1. No product feature expansion.

## Completed

- Phase 0 reusable Rust core, supported Shell desktop discovery, versioned local snapshots, exact diff/matching, guarded restore, thin Tauri commands, and minimal React UI.
- Bounded settle verification with configurable polling interval, polling deadline, and required consecutive exact full-desktop observations.
- Explicit restore outcomes for Shell positioning failure, immediate verification failure, later settle failure, settled success, display blocking, unresolved items, and nothing-to-restore.
- Developer-only fixed-fixture destructive verification harness with an identity allowlist and no random-item fallback.
- Persistent pre-mutation recovery guard, retained completion evidence, RAII recovery for unwind/ordinary error, and `recover-last-verification` crash-recovery command.
- Cross-process recovery ownership is claimed with an atomic no-replace file create. The complete format-v2 record is flushed and read back before mutation; every mutation and completion revalidates the verification ID, ownership token, and sealed record.
- Recovery records persist the exact two-identity fixture allowlist, expected display configuration, original snapshot, and swap metadata under a SHA-256 corruption-detection digest. Recovery preflight rejects display changes and all non-fixture drift, and every recovery write uses the fixture-only subset restore path.
- Fixture validation requires uniquely named, filesystem-backed regular files at the current user's Desktop known-folder paths and rejects directories, symlink/reparse items, wrong paths, and merged-desktop basename duplicates.
- Developer-only `after-mutation` controlled recovery failpoint. It requires both explicit environment gates, disarms RAII only after mutation settles, leaves the active recovery record in place, and returns `RECOVERY_REQUIRED` without crashing the process.
- Manual Windows verification matrix, VM instructions, the first Phase 0.5 baseline evidence record, and isolated-VM persistent recovery evidence.

## Verified

- The fixed-fixture destructive harness passed on an isolated Windows 11 Pro 23H2 build 22631 AMD64 VM with one 2283×1278 virtual display at 100% scale and six desktop items.
- Mutation passed, settle passed, recovery used three observations over 323 ms, and the final full diff was exact: six unchanged, zero moved/missing/new/ambiguous.
- Normal recovery completion was confirmed: the active marker was removed only after verified evidence was archived as `verification-20260827T1059529044508Z-10848-verified.json`.
- The `after-mutation` persistent recovery workflow passed on the same isolated Windows 11 Pro 23H2 build 22631 VM. Mutation passed immediate readback and settled in three attempts over 322 ms; the full mutated diff was exact; and the active marker was confirmed present after the process returned `RECOVERY_REQUIRED`.
- The operator visually confirmed that the two dedicated fixtures remained exchanged after the failpoint. This is a manual observation in addition to, not a substitute for, the automated mutation verification.
- A separate `recover-last-verification` process settled in three attempts over 324 ms. The final full diff was exact with six unchanged and zero moved/missing/new/ambiguous; evidence was archived as `verification-20260827T1152321625746Z-5368-recovered-by-command.json`; and the active marker was confirmed absent after `RESULT: RECOVERED`.
- Pure settle-state behavior, snapshot/diff/matching/storage, recovery-record persistence/archive, failpoint parsing, fail-closed unknown values, opt-in enforcement, and recovery-guard state transitions pass non-destructive tests.
- Remediation regression coverage includes atomic concurrent claim, ownership mismatch, archive/removal failure ordering, record tampering and unsafe IDs, fixture-only recovery authorization, non-fixture drift, strict fixture file validation, exact settle-deadline boundaries, and capture-error UI precedence.
- Current-host non-destructive validation passes Rust formatting, clippy with warnings denied, 53 Rust tests, 2 frontend tests, frontend lint/typecheck/build, and the no-bundle Tauri production build. Both real-Explorer integration tests remain ignored on the host.

The VM's legacy `Get-ComputerInfo` product-name string reported `Windows 10 Pro`, but build 22631 and the actual installed system are Windows 11 Pro 23H2. The evidence is recorded as Windows 11, not Windows 10.

This is baseline evidence from one isolated VM configuration. It is not a general Windows 11 or display-configuration support claim.

The two recorded VM PASS results validated the pre-remediation implementation. They remain historical evidence, but they do not validate the remediated recovery implementation at the current PR head. A targeted isolated-VM regression is required before merge.

## Known limitations

- The controlled `after-mutation` path and subsequent cross-process recovery are verified in one isolated VM configuration only.
- Controlled orphan mode validates the persistent recovery workflow without using panic, abort, or process termination. Its successful VM result does not reproduce every real crash timing or failure mode.
- A process kill, abort, VM crash, or power loss cannot execute RAII recovery. Recovery still depends on the persisted active record and an environment compatible with the original snapshot.
- The settle deadline bounds the polling/observation loop between completed synchronous Shell captures. A single in-progress Shell/COM capture has no hard-cancellation guarantee and can exceed that observation window.
- Recovery fails closed on a changed display signature, unresolved fixture identity, or any moved/missing/new/ambiguous non-fixture item. The active recovery claim remains for operator investigation.
- The SHA-256 record digest detects accidental corruption or manual edits when the digest is not recomputed. It is not a security boundary against a malicious process running as the same Windows user that can modify both program and local files.
- Fixture preparation is manual. Duplicate fixture basenames across merged desktop locations are rejected.
- 125%/150%/200% DPI, physical and multiple displays, mixed DPI, Explorer restart, RDP, Auto Arrange, Align to Grid, and Windows 10 remain unverified.

## Next step

On an isolated Windows 11 VM, rerun the remediated binary through (1) the baseline round-trip and (2) `after-mutation` followed by `recover-last-verification`, then return Draft PR #1 to the independent reviewer.
