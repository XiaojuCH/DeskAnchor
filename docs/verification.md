# Developer desktop verification

This tooling is intentionally separate from the production UI. It can move desktop icons and must run only in an isolated Windows validation VM.

## Fixtures

DeskAnchor uses operator-created fixtures rather than creating or deleting desktop files. Before a run, create these two empty files and place their icons at distinct positions:

```text
DeskAnchor-Test-A.txt
DeskAnchor-Test-B.txt
```

On a clean validation VM, PowerShell can create them without overwriting an existing same-name file:

```powershell
$desktop = [Environment]::GetFolderPath("Desktop")
New-Item -ItemType File -Path (Join-Path $desktop "DeskAnchor-Test-A.txt") -ErrorAction Stop
New-Item -ItemType File -Path (Join-Path $desktop "DeskAnchor-Test-B.txt") -ErrorAction Stop
```

Move the two fixture icons to visibly distinct positions before starting the test. If either command reports that the file already exists, inspect the existing file instead of using `-Force`.

The harness first locates these basenames in the merged Shell desktop and fails closed on duplicates, including a User Desktop/Public Desktop collision. Each unique candidate must then resolve to the expected current-user Desktop known-folder path, be a filesystem-backed regular file, and not be a directory, symlink, or reparse-point item. It never falls back to another icon and does not overwrite, create, or delete either fixture.

## Safety gates

The destructive integration test has all of these gates:

1. Rust `#[ignore]`, so normal `cargo test` cannot execute it.
2. `DESKANCHOR_DESTRUCTIVE_TESTS=1`, checked before desktop capture or mutation.
3. Two exact, unique fixtures at distinct positions.
4. A transient cross-process operation lease held across the entire verification/recovery lifecycle.
5. An atomic, cross-process, no-replace claim on the fixed active-record path.
6. A complete recovery record that is flushed, read back, integrity-checked, and ownership-checked before mutation.

The optional recovery failpoint adds another exact gate: `DESKANCHOR_VERIFICATION_FAILPOINT` must equal `after-mutation`. It is ignored when absent, unknown values are rejected before capture, and it cannot bypass the destructive opt-in.

Recovery data is local and contains private desktop names/identities:

```text
%LOCALAPPDATA%\DeskAnchor\verification\active-recovery.json
%LOCALAPPDATA%\DeskAnchor\verification\records\*.json
```

The same directory also contains `operation.lock`, but its existence does not mean an operation is active and it is not a recovery record. Each supported `verify-destructive` or `recover-last-verification` invocation opens that path with a live Windows file handle whose share mode permits no competing open. It holds the handle from before recovery-store access through fixture mutation/recovery, evidence archive, and active-claim removal. A contender fails immediately with `VerificationRecoveryError::OperationBusy`; it does not read or modify the active claim. Normal return or unwind releases the handle through RAII; abort, kill, or process crash releases it through Windows process-handle cleanup, allowing a later command to acquire it. No process waits indefinitely.

The operation lease and `active-recovery.json` have different lifetimes. The lease is transient mutual exclusion and is released when a command ends. The active claim is durable recovery evidence and deliberately survives the controlled failpoint or a process failure after it was persisted.

The active file is created directly with no-replace semantics while the operation lease is held. This removes the `exists`/replace-capable-rename race: concurrent runs cannot both own the path. Because every supported command retains the same lease through ownership validation, mutation, archive, and removal, another DeskAnchor process cannot clear or replace the claim inside either check-to-mutation or check-to-removal critical section. If a process fails while writing the record, the partial file remains as a fail-closed claim, no mutation was authorized, and later runs cannot overwrite it. Preserve and investigate a partial record manually; do not delete an existing `active-recovery.json` merely to bypass the guard. A safe operator workflow for archiving and clearing confirmed partial claims remains a future follow-up rather than an automatic deletion command.

The format-v2 active record binds the internally generated verification ID and ownership token to the original snapshot, exact fixture identity allowlist, expected display configuration, fixture Desktop path, and intended swap targets. A SHA-256 digest over this canonical structured payload detects accidental corruption and ordinary manual edits before recovery. The digest is deliberately not presented as protection from a malicious process running with the same Windows-user permissions: such a process can modify the executable, record, and digest together.

Verification IDs accept only ASCII letters, digits, `-`, and `_`, have a fixed maximum length, and reject empty/traversal values. An ID loaded from disk is validated before it can contribute to an archive filename.

## VM verification command

From the repository in an isolated interactive Windows VM:

```powershell
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
cargo test -p deskanchor-core --test windows_desktop destructive_fixture_round_trip_restores_the_complete_layout -- --ignored --nocapture
```

The test captures the full layout, claims and persists recovery, swaps only the two fixture positions, verifies the mutated layout through settle capture/diff, and restores only those same two identities to their original coordinates. Recovery first captures the full desktop and requires every non-fixture item to match the original snapshot exactly. It then performs a final full capture/diff; only an exact result may be archived under `records` and followed by active-claim removal.

## Settle observation window

The settle deadline bounds polling decisions between completed synchronous Shell captures. An observation completed at or after the deadline is a timeout even when its diff is exact. DeskAnchor does not currently hard-cancel a blocking Shell/COM capture, so this setting is not a strict total wall-clock limit.

## Recorded Windows 11 VM baseline

The first fixed-fixture run passed on an isolated Windows 11 Pro 23H2 build 22631 AMD64 VM with one 2283×1278 virtual display at 100% scale and six desktop items. Fixture A started at `(698, 202)` and fixture B at `(1914, 202)`. Mutation passed, settle passed, recovery used three observations over 323 ms, and the final complete diff contained six unchanged items with no moved, missing, new, or ambiguous items. The active marker was absent afterward, and verified evidence was retained as:

```text
verification-20260827T1059529044508Z-10848-verified.json
```

No unrelated desktop item names or user-specific path are recorded here. This single baseline does not establish broader Windows/display compatibility.

This PASS is retained as historical evidence for the pre-remediation implementation. It does not by itself validate the format-v2 atomic-claim and fixture-only recovery implementation; the separate post-remediation regression evidence is recorded below.

## Controlled after-mutation recovery test

Only in an isolated VM, run:

```powershell
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
$env:DESKANCHOR_VERIFICATION_FAILPOINT = "after-mutation"
C:\DeskAnchorTest\deskanchor-verify.exe verify-destructive
```

The harness still persists the complete active recovery record, swaps only the two fixtures, waits for settled mutation, and confirms the full mutated diff. It then transitions the RAII guard to an explicit manual-recovery state and exits normally with `RESULT: RECOVERY_REQUIRED`. It does not panic or simulate a process crash. The fixture positions and `active-recovery.json` are intentionally left unchanged so the persistent recovery path can be tested reproducibly.

The verification operation lease remains live through that transition and through `RecoveryGuard::drop`. Because `ManualRecoveryRequired` intentionally disables automatic recovery, the guard leaves the active claim untouched and then releases the transient lease as the command returns. The later recovery command acquires a new lease before loading the same durable claim and retains it until recovery, archive, and active removal finish.

At this point, do not move either fixture, change the display configuration, create/delete/rename desktop items, start another destructive run, or delete `active-recovery.json`. No new completion evidence should exist yet; the active record remains in status `active`.

Recover with the existing implementation:

```powershell
Remove-Item Env:DESKANCHOR_VERIFICATION_FAILPOINT
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
C:\DeskAnchorTest\deskanchor-verify.exe recover-last-verification
```

Successful recovery verifies record integrity and ownership, captures the full desktop, rejects any non-fixture drift, restores only the stored two-identity allowlist, performs settle verification and a final exact full diff, archives evidence with a `-recovered-by-command.json` suffix, and only then removes the active claim.

The recorded PASS for this scenario is also historical evidence for the pre-remediation implementation. The separate post-remediation regression evidence is recorded below.

## Crash recovery

RAII attempts recovery after ordinary errors, early returns, assertion unwinds, and panics compiled with unwinding. It cannot cover process termination, abort, VM crash, power loss, or host failure. In those cases the active record remains and blocks new tests.

After reopening an interactive session with the same display configuration, run:

```powershell
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
cargo run -p deskanchor-core --bin deskanchor-verify -- recover-last-verification
```

The command validates the sealed record and semantic invariants, verifies that both stored fixture identities are uniquely resolvable regular files at the stored/current Desktop fixture paths, and confirms that every non-fixture item still exactly matches the original snapshot. It then uses fixture-subset restore with settle verification, performs a final complete diff, writes a `recovered-by-command` evidence record, revalidates ownership, and only then removes the active claim.

`NothingToRestore` remains a valid recovery-command result when a complete active record survived a crash after persistence but before fixture mutation, or when an ordinary pre-mutation failure left the original exact layout. The lifecycle lease guarantees that this no-op recovery cannot run concurrently with a still-live verifier; after exact full-diff verification it may safely archive and clear the durable claim.

If any non-fixture item moved, disappeared, appeared, or became ambiguous, recovery reports external desktop drift and performs no restore. A display mismatch, unresolved fixture, integrity failure, ownership mismatch, archive failure, active-claim removal failure, or non-exact final diff also fails closed and leaves the active evidence in place. Recovery never moves a non-fixture icon.

The same binary can run the harness directly with `verify-destructive`, but the ignored integration-test command is preferred for recorded matrix runs because it preserves both explicit safety gates in the invocation.

## Recorded post-remediation VM regression

The targeted regression was completed using the binary from commit `28097628997e4183cbda7b0c2d8c3eab774437a7`. The host-built and VM-executed binaries were confirmed to share SHA-256 `7870B9ADAEFB08A3CD1C5389934C778C1053096F019D5218727FDE471999C926`.

On the same isolated Windows 11 Pro 23H2 build 22631 AMD64 VM with one 2283×1278 virtual display at 100% scale and six desktop items:

- The post-remediation baseline round-trip passed mutation, immediate restore, and settle verification in three attempts over 323 ms. The final full diff was exact, evidence was archived as `verification-20260827T1703128240159Z-10580-b1c14ad04f2d4070b99df1c36b850556-verified.json`, and the active claim was absent afterward.
- The post-remediation `after-mutation` run passed mutation, immediate readback, and settle verification in three attempts over 323 ms with an exact mutated diff, then returned `RECOVERY_REQUIRED`. A separate recovery invocation returned `Settled`, passed settle verification in three attempts over 324 ms, produced an exact final full diff, archived `verification-20260827T1703397294356Z-6904-fb42c270bed24db19aef7719b51a4ae7-recovered-by-command.json`, cleared the active claim, and returned `RECOVERED`.

These post-remediation regression PASS results confirm that the normal baseline and persistent-recovery workflows still function after the atomic-claim, format-v2 integrity, strict-fixture, and fixture-only recovery changes. They do not independently construct or prove every B1/B2/B3 boundary; those safety properties remain primarily covered by the dedicated unit and non-destructive regression tests. No support inference is made for Windows 10, other DPI values, physical or multiple displays, or other untested Explorer configurations.

These binaries predate the later cross-process operation lease, so the PASS results remain evidence for commit `28097628997e4183cbda7b0c2d8c3eab774437a7` rather than the operation-lease implementation. The separate operation-lease smoke is recorded below.

## Recorded operation-lease remediation VM smoke

The operation-lease smoke used the binary from commit `9a312c6649369b6dad559b0c4c2fa312dc4c3109`. The host-built and VM-executed binaries were confirmed to share SHA-256 `4FBB65542A05B4B3DDDF9A5A2B96F5FA7B516D110D208DD9F925A0491A5413CD`.

On the same isolated Windows 11 Pro 23H2 build 22631 AMD64 VM with one 2283×1278 virtual display at 100% scale and six desktop items:

- The operation-lease baseline round-trip passed mutation, immediate restore, and settle verification in three attempts over 323 ms. Its final full diff contained six unchanged and zero moved, missing, new, or ambiguous items. Recovery was confirmed, evidence was archived as `verification-20260828T0133379303343Z-292-f07d2f17b2ae451b87e03d161c5094d7-verified.json`, and `active-recovery.json` was absent before and after the run.
- The operation-lease `after-mutation` run passed mutation, immediate readback, and settle verification in three attempts over 325 ms with an exact mutated diff, then triggered the controlled failpoint and returned `RESULT: RECOVERY_REQUIRED`. The active marker was present afterward. After the failpoint environment variable was removed, a separate `recover-last-verification` invocation returned `Settled`, passed settle verification in three attempts over 326 ms, produced an exact final full diff with six unchanged and zero moved, missing, new, or ambiguous items, archived `verification-20260828T0133544395307Z-8584-ab1ba3a514374ea3b538d803afa7ec11-recovered-by-command.json`, cleared the active claim, and returned `RESULT: RECOVERED`.

After all commands completed, `%LOCALAPPDATA%\DeskAnchor\verification\operation.lock` remained present and `Test-Path` returned `True`. This is expected carrier-file behavior, not evidence of an active or unreleased lock. The transient mutual exclusion is the live exclusive Windows file handle; process exit closes that handle even though the carrier path remains on disk.

The smoke demonstrates that adding the lifecycle operation lease did not regress the real baseline round-trip or `after-mutation` → separate `recover-last-verification` Explorer workflow, and that the durable active marker remained after the failpoint and was cleared after exact recovery. It is not a constructive VM proof that the B1 cross-process race is impossible. That safety boundary remains primarily supported by the operation-lease code invariant, deterministic contention/regression tests, and independent code review. No broader Windows or display-support inference is made.
