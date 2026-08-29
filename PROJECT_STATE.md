# Project state

## Current phase

Phase 1A — Single canonical Saved Layout plus non-destructive Compare. Phase 0.5 is complete, and PR #1 was squash-merged to `main` as `f6cff9852a446df3fd66d30e2c8c400e294331b4`.

## Completed

- Phase 1A canonical `%LOCALAPPDATA%\DeskAnchor\snapshots\saved-layout.json` storage contract. A complete validated snapshot is written and flushed to a same-directory temporary file, then published on Windows with `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` without a delete-first gap. A failed publication leaves the previous saved layout valid.
- Phase 1A ID-less `get_saved_layout`, `save_saved_layout`, and `compare_saved_layout` Tauri commands. Legacy timestamp snapshot APIs remain for internal compatibility, but the Phase 1 product workflow does not enumerate or depend on them.
- Phase 1A single-layout React workflow with automatic startup Compare, explicit Save/Replace behavior, exact/moved/missing/new/ambiguous/display-mismatch states, unavailable/corrupt feedback, and request-generation protection against stale async responses. The production UI exposes no Restore action in Phase 1A.
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
- Operation-lease baseline and persistent-recovery smoke evidence for the binary built from commit `9a312c6649369b6dad559b0c4c2fa312dc4c3109`.

## Verified

- Phase 1A current-host automatic validation passes Rust formatting, clippy with warnings denied, 67 non-destructive Rust tests, 11 frontend tests, frontend lint/typecheck/build, and the no-bundle Tauri production build. Both real-Explorer integration tests remain ignored.
- The Phase 1A normal-host read-only Explorer capture smoke passed when the specific ignored capture test was selected by name. The destructive fixture round-trip remained filtered out and was not run.
- Phase 1A replacement regression coverage includes no canonical layout, first publication, successful replacement, controlled publication failure, an actual Windows sharing-conflict failure, corrupt/unsupported canonical data, and isolation from legacy timestamp snapshots. Both failure tests confirm that the prior valid canonical snapshot remains loadable.
- Phase 1A frontend coverage includes no saved layout, exact and changed comparisons, all five diff counts, display mismatch without remapping, corrupt/unavailable saved data, current capture failure, Save and Replace success, Replace failure without a false success state, and deterministic stale-response suppression.
- Historical pre-remediation baseline evidence: the fixed-fixture harness passed on an isolated Windows 11 Pro 23H2 build 22631 AMD64 VM with one 2283×1278 virtual display at 100% scale and six desktop items. Mutation and settle passed; recovery used three observations over 323 ms; the final full diff was exact; evidence was archived as `verification-20260827T1059529044508Z-10848-verified.json`; and the active marker was absent.
- Historical pre-remediation persistent-recovery evidence: the `after-mutation` mutation passed immediate readback and settled in three attempts over 322 ms, the operator visually confirmed the exchanged fixtures after `RECOVERY_REQUIRED`, and a separate recovery invocation settled in three attempts over 324 ms. Its final full diff was exact, evidence was archived as `verification-20260827T1152321625746Z-5368-recovered-by-command.json`, and the active marker was absent after `RESULT: RECOVERED`.
- Post-remediation binary provenance: the host-built and VM-executed `deskanchor-verify.exe` files were confirmed to share SHA-256 `7870B9ADAEFB08A3CD1C5389934C778C1053096F019D5218727FDE471999C926` and correspond to commit `28097628997e4183cbda7b0c2d8c3eab774437a7`.
- Post-remediation baseline regression PASS: mutation, immediate restore, and settle verification passed in three attempts over 323 ms; the final full diff contained six unchanged and zero moved/missing/new/ambiguous items; evidence was archived as `verification-20260827T1703128240159Z-10580-b1c14ad04f2d4070b99df1c36b850556-verified.json`; and `active-recovery.json` was absent afterward.
- Post-remediation persistent-recovery regression PASS: the controlled mutation passed immediate readback and settle verification in three attempts over 323 ms with an exact mutated diff, then returned `RECOVERY_REQUIRED`. A separate `recover-last-verification` invocation returned `Settled`, passed settle verification in three attempts over 324 ms, produced an exact six-item final diff, archived `verification-20260827T1703397294356Z-6904-fb42c270bed24db19aef7719b51a4ae7-recovered-by-command.json`, cleared the active claim, and returned `RESULT: RECOVERED`.
- Operation-lease binary provenance: the host-built and VM-executed `deskanchor-verify.exe` files were confirmed to share SHA-256 `4FBB65542A05B4B3DDDF9A5A2B96F5FA7B516D110D208DD9F925A0491A5413CD` and correspond to commit `9a312c6649369b6dad559b0c4c2fa312dc4c3109`.
- Operation-lease baseline smoke PASS: mutation, immediate restore, and settle verification passed in three attempts over 323 ms; the final full diff contained six unchanged and zero moved/missing/new/ambiguous items; evidence was archived as `verification-20260828T0133379303343Z-292-f07d2f17b2ae451b87e03d161c5094d7-verified.json`; and `active-recovery.json` was absent before and after the run.
- Operation-lease persistent-recovery smoke PASS: the controlled mutation passed immediate readback and settle verification in three attempts over 325 ms with an exact mutated diff, then returned `RESULT: RECOVERY_REQUIRED` with `active-recovery.json` present. A separate `recover-last-verification` invocation returned `Settled`, passed settle verification in three attempts over 326 ms, produced an exact six-item final diff, archived `verification-20260828T0133544395307Z-8584-ab1ba3a514374ea3b538d803afa7ec11-recovered-by-command.json`, cleared the active claim, and returned `RESULT: RECOVERED`.
- Pure settle-state behavior, snapshot/diff/matching/storage, recovery-record persistence/archive, failpoint parsing, fail-closed unknown values, opt-in enforcement, and recovery-guard state transitions pass non-destructive tests.
- Remediation regression coverage includes operation-lease exclusion at the pre-mutation and active-removal boundaries, failpoint lease release, RecoveryGuard Drop ordering, atomic concurrent claim, ownership mismatch, archive/removal failure ordering, record tampering and unsafe IDs, fixture-only recovery authorization, non-fixture drift, strict fixture file validation, exact settle-deadline boundaries, and capture-error UI precedence.
- Current-host non-destructive validation passes Rust formatting, clippy with warnings denied, 56 Rust tests, 2 frontend tests, frontend lint/typecheck/build, and the no-bundle Tauri production build. Both real-Explorer integration tests remain ignored on the host.

The VM's legacy `Get-ComputerInfo` product-name string reported `Windows 10 Pro`, but build 22631 and the actual installed system are Windows 11 Pro 23H2. The evidence is recorded as Windows 11, not Windows 10.

All three evidence generations come from one isolated VM configuration. The first is historical pre-remediation evidence, the second covers the recovery-safety remediation at `2809762`, and the third covers the operation-lease implementation at `9a312c6`. The newest smoke runs show that adding the lifecycle operation lease did not regress the normal baseline or `after-mutation` → separate `recover-last-verification` Explorer workflows; the active marker remained present after the failpoint and was cleared only after exact recovery.

The operation-lease smoke is not a constructive VM proof that the B1 cross-process concurrency race is impossible. That safety boundary remains primarily supported by the operation-lease code invariant, deterministic contention/regression tests, and independent code review. Likewise, the earlier B2/B3 boundary cases remain primarily supported by focused non-destructive tests and code review. None of these runs is a general Windows 11 or display-configuration support claim.

The persistent `%LOCALAPPDATA%\DeskAnchor\verification\operation.lock` carrier file still existed after the smoke runs. This is expected and is not evidence of an active or unreleased lock: transient mutual exclusion is represented by the live exclusive Windows file handle, which closes when the process exits, while the carrier file may remain on disk.

## Known limitations

- Phase 1A intentionally has no product Restore entry. The known moved-plus-missing/new/ambiguous settle-contract issue remains for Phase 1B; restore outcomes, settle behavior, Shell positioning, and the developer verifier were not changed.
- Legacy timestamp-based Phase 0 snapshot JSON files are neither migrated nor deleted. They are ignored by canonical saved-layout loading and are not exposed as product history.
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

Review the Phase 1A single Saved Layout plus Compare implementation as a normal feature PR. Do not merge it as part of the implementation pass. Phase 1B should address the restore/partial-result contract separately after Phase 1A is accepted.
