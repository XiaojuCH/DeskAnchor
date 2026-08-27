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
4. An atomic, cross-process, no-replace claim on the fixed active-record path.
5. A complete recovery record that is flushed, read back, integrity-checked, and ownership-checked before mutation.

The optional recovery failpoint adds another exact gate: `DESKANCHOR_VERIFICATION_FAILPOINT` must equal `after-mutation`. It is ignored when absent, unknown values are rejected before capture, and it cannot bypass the destructive opt-in.

Recovery data is local and contains private desktop names/identities:

```text
%LOCALAPPDATA%\DeskAnchor\verification\active-recovery.json
%LOCALAPPDATA%\DeskAnchor\verification\records\*.json
```

The active file is created directly with no-replace semantics. This removes the `exists`/replace-capable-rename race: concurrent runs cannot both own the path. If a process fails while writing the record, the partial file remains as a fail-closed claim, no mutation is allowed, and later runs cannot overwrite it. Do not delete an existing `active-recovery.json` to bypass the guard.

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

This PASS is retained as historical evidence for the pre-remediation implementation. It does not validate the format-v2 atomic-claim and fixture-only recovery implementation at the current PR head.

## Controlled after-mutation recovery test

Only in an isolated VM, run:

```powershell
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
$env:DESKANCHOR_VERIFICATION_FAILPOINT = "after-mutation"
C:\DeskAnchorTest\deskanchor-verify.exe verify-destructive
```

The harness still persists the complete active recovery record, swaps only the two fixtures, waits for settled mutation, and confirms the full mutated diff. It then transitions the RAII guard to an explicit manual-recovery state and exits normally with `RESULT: RECOVERY_REQUIRED`. It does not panic or simulate a process crash. The fixture positions and `active-recovery.json` are intentionally left unchanged so the persistent recovery path can be tested reproducibly.

At this point, do not move either fixture, change the display configuration, create/delete/rename desktop items, start another destructive run, or delete `active-recovery.json`. No new completion evidence should exist yet; the active record remains in status `active`.

Recover with the existing implementation:

```powershell
Remove-Item Env:DESKANCHOR_VERIFICATION_FAILPOINT
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
C:\DeskAnchorTest\deskanchor-verify.exe recover-last-verification
```

Successful recovery verifies record integrity and ownership, captures the full desktop, rejects any non-fixture drift, restores only the stored two-identity allowlist, performs settle verification and a final exact full diff, archives evidence with a `-recovered-by-command.json` suffix, and only then removes the active claim.

The recorded PASS for this scenario is also historical evidence for the pre-remediation implementation. The remediated path must be rerun in an isolated VM before merge.

## Crash recovery

RAII attempts recovery after ordinary errors, early returns, assertion unwinds, and panics compiled with unwinding. It cannot cover process termination, abort, VM crash, power loss, or host failure. In those cases the active record remains and blocks new tests.

After reopening an interactive session with the same display configuration, run:

```powershell
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
cargo run -p deskanchor-core --bin deskanchor-verify -- recover-last-verification
```

The command validates the sealed record and semantic invariants, verifies that both stored fixture identities are uniquely resolvable regular files at the stored/current Desktop fixture paths, and confirms that every non-fixture item still exactly matches the original snapshot. It then uses fixture-subset restore with settle verification, performs a final complete diff, writes a `recovered-by-command` evidence record, revalidates ownership, and only then removes the active claim.

If any non-fixture item moved, disappeared, appeared, or became ambiguous, recovery reports external desktop drift and performs no restore. A display mismatch, unresolved fixture, integrity failure, ownership mismatch, archive failure, active-claim removal failure, or non-exact final diff also fails closed and leaves the active evidence in place. Recovery never moves a non-fixture icon.

The same binary can run the harness directly with `verify-destructive`, but the ignored integration-test command is preferred for recorded matrix runs because it preserves both explicit safety gates in the invocation.

## Required post-remediation VM regression

Previous VM evidence validated the pre-fix implementation. The remediated implementation requires a targeted VM regression run before merge:

1. Baseline fixture round-trip.
2. `after-mutation`, followed by a separate `recover-last-verification` process.

Run both only on an isolated Windows 11 VM, retain the new evidence records and console output, and then return the Draft PR to the independent reviewer.
