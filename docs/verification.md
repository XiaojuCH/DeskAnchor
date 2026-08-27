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

The harness matches their filesystem parsing-name basenames, requires exactly one match for each, and never falls back to another icon. It does not overwrite, create, or delete either file. A missing or ambiguous fixture stops before mutation.

## Safety gates

The destructive integration test has all of these gates:

1. Rust `#[ignore]`, so normal `cargo test` cannot execute it.
2. `DESKANCHOR_DESTRUCTIVE_TESTS=1`, checked before desktop capture or mutation.
3. Two exact, unique fixtures at distinct positions.
4. An atomically persisted complete recovery record before mutation.

Recovery data is local and contains private desktop names/identities:

```text
%LOCALAPPDATA%\DeskAnchor\verification\active-recovery.json
%LOCALAPPDATA%\DeskAnchor\verification\records\*.json
```

An existing `active-recovery.json` refuses every new destructive run. Do not delete it to bypass the guard.

## VM verification command

From the repository in an isolated interactive Windows VM:

```powershell
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
cargo test -p deskanchor-core --test windows_desktop destructive_fixture_round_trip_restores_the_complete_layout -- --ignored --nocapture
```

The test captures the full layout, persists recovery, swaps only the two fixture positions, verifies the mutated layout through settle capture/diff, restores the original complete snapshot, and performs a final exact full diff. Completion evidence is retained under `records`.

## Crash recovery

RAII attempts recovery after ordinary errors, early returns, assertion unwinds, and panics compiled with unwinding. It cannot cover process termination, abort, VM crash, power loss, or host failure. In those cases the active record remains and blocks new tests.

After reopening an interactive session with the same display configuration, run:

```powershell
$env:DESKANCHOR_DESTRUCTIVE_TESTS = "1"
cargo run -p deskanchor-core --bin deskanchor-verify -- recover-last-verification
```

The command validates the stored snapshot, uses guarded restore with settle verification, performs a final complete diff, writes a `recovered-by-command` evidence record, and only then removes the active marker. A display mismatch, unresolved item, or settle failure leaves the active record in place.

The same binary can run the harness directly with `verify-destructive`, but the ignored integration-test command is preferred for recorded matrix runs because it preserves both explicit safety gates in the invocation.
