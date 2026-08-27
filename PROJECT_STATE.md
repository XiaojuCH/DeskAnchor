# Project state

## Current phase

Phase 0.5C — Close the final cross-process verification lifecycle blocker in Draft PR #1. No product feature expansion.

## Completed

- Phase 0 reusable Rust core, supported Shell desktop discovery, versioned local snapshots, exact diff/matching, guarded restore, thin Tauri commands, and minimal React UI.
- Bounded settle verification with configurable polling interval, polling deadline, and required consecutive exact full-desktop observations.
- Explicit restore outcomes for Shell positioning failure, immediate verification failure, later settle failure, settled success, display blocking, unresolved items, and nothing-to-restore.
- Developer-only fixed-fixture destructive verification harness with an identity allowlist and no random-item fallback.
- Persistent pre-mutation recovery guard, retained completion evidence, RAII recovery for unwind/ordinary error, and `recover-last-verification` crash-recovery command.
- Every supported verification/recovery command acquires the same transient Windows operation lease before entering recovery-store state. A live exclusive `operation.lock` handle, not the lock file's existence, serializes capture, active-claim creation/load, fixture mutation/recovery, archive, and active removal. Contention fails immediately with `VerificationRecoveryError::OperationBusy`; Windows releases the handle if the process exits or crashes.
- Cross-process recovery ownership is separately persisted with an atomic no-replace active-claim create. The complete format-v2 record is flushed and read back before mutation; every mutation and completion also revalidates the verification ID, ownership token, and sealed record.
- Recovery records persist the exact two-identity fixture allowlist, expected display configuration, original snapshot, and swap metadata under a SHA-256 corruption-detection digest. Recovery preflight rejects display changes and all non-fixture drift, and every recovery write uses the fixture-only subset restore path.
- Fixture validation requires uniquely named, filesystem-backed regular files at the current user's Desktop known-folder paths and rejects directories, symlink/reparse items, wrong paths, and merged-desktop basename duplicates.
- Developer-only `after-mutation` controlled recovery failpoint. It requires both explicit environment gates, disarms RAII only after mutation settles, leaves the active recovery record in place, and returns `RECOVERY_REQUIRED` without crashing the process.
- Manual Windows verification matrix, VM instructions, the first Phase 0.5 baseline evidence record, and isolated-VM persistent recovery evidence.
- Targeted post-remediation baseline and persistent-recovery regression evidence for the binary built from commit `28097628997e4183cbda7b0c2d8c3eab774437a7`.

## Verified

- Historical pre-remediation baseline evidence: the fixed-fixture harness passed on an isolated Windows 11 Pro 23H2 build 22631 AMD64 VM with one 2283×1278 virtual display at 100% scale and six desktop items. Mutation and settle passed; recovery used three observations over 323 ms; the final full diff was exact; evidence was archived as `verification-20260827T1059529044508Z-10848-verified.json`; and the active marker was absent.
- Historical pre-remediation persistent-recovery evidence: the `after-mutation` mutation passed immediate readback and settled in three attempts over 322 ms, the operator visually confirmed the exchanged fixtures after `RECOVERY_REQUIRED`, and a separate recovery invocation settled in three attempts over 324 ms. Its final full diff was exact, evidence was archived as `verification-20260827T1152321625746Z-5368-recovered-by-command.json`, and the active marker was absent after `RESULT: RECOVERED`.
- Post-remediation binary provenance: the host-built and VM-executed `deskanchor-verify.exe` files were confirmed to share SHA-256 `7870B9ADAEFB08A3CD1C5389934C778C1053096F019D5218727FDE471999C926` and correspond to commit `28097628997e4183cbda7b0c2d8c3eab774437a7`.
- Post-remediation baseline regression PASS: mutation, immediate restore, and settle verification passed in three attempts over 323 ms; the final full diff contained six unchanged and zero moved/missing/new/ambiguous items; evidence was archived as `verification-20260827T1703128240159Z-10580-b1c14ad04f2d4070b99df1c36b850556-verified.json`; and `active-recovery.json` was absent afterward.
- Post-remediation persistent-recovery regression PASS: the controlled mutation passed immediate readback and settle verification in three attempts over 323 ms with an exact mutated diff, then returned `RECOVERY_REQUIRED`. A separate `recover-last-verification` invocation returned `Settled`, passed settle verification in three attempts over 324 ms, produced an exact six-item final diff, archived `verification-20260827T1703397294356Z-6904-fb42c270bed24db19aef7719b51a4ae7-recovered-by-command.json`, cleared the active claim, and returned `RESULT: RECOVERED`.
- Pure settle-state behavior, snapshot/diff/matching/storage, recovery-record persistence/archive, failpoint parsing, fail-closed unknown values, opt-in enforcement, and recovery-guard state transitions pass non-destructive tests.
- Remediation regression coverage includes operation-lease exclusion at the pre-mutation and active-removal boundaries, failpoint lease release, RecoveryGuard Drop ordering, atomic concurrent claim, ownership mismatch, archive/removal failure ordering, record tampering and unsafe IDs, fixture-only recovery authorization, non-fixture drift, strict fixture file validation, exact settle-deadline boundaries, and capture-error UI precedence.
- Current-host non-destructive validation passes Rust formatting, clippy with warnings denied, 56 Rust tests, 2 frontend tests, frontend lint/typecheck/build, and the no-bundle Tauri production build. Both real-Explorer integration tests remain ignored on the host.

The VM's legacy `Get-ComputerInfo` product-name string reported `Windows 10 Pro`, but build 22631 and the actual installed system are Windows 11 Pro 23H2. The evidence is recorded as Windows 11, not Windows 10.

Both evidence generations come from one isolated VM configuration. The post-remediation runs show that the normal baseline and persistent-recovery workflows did not regress after the atomic-claim, format-v2 integrity, strict-fixture, and fixture-only recovery changes. They do not independently construct or prove the B1/B2/B3 boundary cases; those properties remain primarily supported by the dedicated unit and non-destructive regression tests. This is not a general Windows 11 or display-configuration support claim.

The recorded post-remediation VM binary came from commit `28097628997e4183cbda7b0c2d8c3eab774437a7`, before the later lifecycle operation lease was added. That evidence remains valid for its recorded commit but does not validate the current execution path. A small isolated-VM baseline plus `after-mutation`/`recover-last-verification` smoke should be rerun for the operation-lease HEAD before readiness; concurrency safety itself is covered by deterministic host tests and code review rather than a timing-based VM experiment.

## Known limitations

- The controlled `after-mutation` path and subsequent cross-process recovery are verified in one isolated VM configuration only.
- Controlled orphan mode validates the persistent recovery workflow without using panic, abort, or process termination. Its successful VM result does not reproduce every real crash timing or failure mode.
- A process kill, abort, VM crash, or power loss cannot execute RAII recovery. Recovery still depends on the persisted active record and an environment compatible with the original snapshot.
- The settle deadline bounds the polling/observation loop between completed synchronous Shell captures. A single in-progress Shell/COM capture has no hard-cancellation guarantee and can exceed that observation window.
- Recovery fails closed on a changed display signature, unresolved fixture identity, or any moved/missing/new/ambiguous non-fixture item. The active recovery claim remains for operator investigation.
- A partial active claim means record persistence did not complete and fixture mutation was never authorized. It intentionally blocks automated recovery and new verification until an operator preserves and investigates the file; no automatic partial-claim deletion command exists.
- The SHA-256 record digest detects accidental corruption or manual edits when the digest is not recomputed. It is not a security boundary against a malicious process running as the same Windows user that can modify both program and local files.
- Fixture preparation is manual. Duplicate fixture basenames across merged desktop locations are rejected.
- 125%/150%/200% DPI, physical and multiple displays, mixed DPI, Explorer restart, RDP, Auto Arrange, Align to Grid, and Windows 10 remain unverified.

## Next step

Push the operation-lease remediation to Draft PR #1, run the small isolated-VM smoke, and return the updated HEAD to the independent reviewer. Do not mark the PR ready or merge it as part of this remediation.
