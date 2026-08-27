# Architecture

## Boundaries

DeskAnchor has three layers:

1. `crates/deskanchor-core` owns domain models, snapshot/diff/storage logic, and all Windows Explorer/display integration.
2. `src-tauri` exposes small async commands and chooses the local snapshot directory. It sends summaries/results across IPC and contains no raw Shell handles.
3. `src` is a minimal React/TypeScript view. It cannot enumerate or move desktop items itself.

The reusable core keeps a future CLI possible without moving Explorer logic into Tauri commands.

## Core layout

- `desktop/model.rs`: platform-neutral desktop, icon, monitor, coordinate, and signature types.
- `desktop/discovery.rs`: short-lived COM STA and acquisition of the current Explorer `IFolderView`/`IShellFolder`.
- `desktop/icons.rs`: current PIDL enumeration, parsing/display names, and positions.
- `desktop/monitors.rs`: CCD/GDI monitor configuration, work area, and display-scale capture.
- `desktop/restore.rs`: guarded identity matching, `SelectAndPositionItems`, immediate readback, and bounded settle verification.
- `desktop/settle.rs`: pure polling/deadline state machine and settle policy.
- `snapshot/model.rs`: schema v1 and validation.
- `snapshot/diff.rs`: pure identity grouping and change classification.
- `snapshot/storage.rs`: local, pretty JSON with same-directory temporary write and atomic rename.

## Operation flow

Capture reacquires the Explorer desktop view on a dedicated COM STA, enumerates child PIDLs, converts each to an `IShellItem`, reads names and coordinates, then drops all COM/PIDL resources before returning a plain Rust `DesktopState`.

Restore validates the snapshot, reacquires and re-enumerates the current desktop, computes the pure diff, and blocks if the display signature differs. Only moved items with exactly one snapshot identity and one current identity are sent to Explorer. Missing, new, and ambiguous items are counted but not touched.

After the supported batch positioning call succeeds, every moved PIDL is read back immediately. If that passes, restore repeatedly reacquires a fresh desktop view and performs a complete capture/diff until the layout is exactly equal for the configured number of consecutive observations or the polling deadline expires. The default is a 150 ms polling interval, a 2 second observation window, and three consecutive exact observations. An observation completed at or after the deadline cannot settle. Missing, new, ambiguous, moved, or display-mismatched state resets stability. `RestoreOutcome::Settled` is the only successful moved-layout outcome; callers must not infer success from the `restored` count.

The observation deadline is evaluated between completed synchronous Shell captures. The current implementation does not hard-cancel an in-progress COM capture, so it is not a strict total wall-clock bound.

## Developer verification boundary

`verification.rs` and the `deskanchor-verify` binary are developer-only tooling and are not reachable from Tauri IPC or the production UI. The destructive integration test is ignored, requires `DESKANCHOR_DESTRUCTIVE_TESTS=1`, and only swaps the positions of two pre-created, uniquely identified fixture files. Every supported verification/recovery command first opens the same `operation.lock` with a live Windows handle that permits no sharing; contention fails closed rather than waiting. The handle is the transient lifecycle mutex and remains held across active-claim access, fixture mutation/recovery, archive, active removal, and RAII recovery. The lock file may persist and is not recovery evidence; Windows releases ownership when the handle closes or the process terminates.

Separately, `create_new` atomically claims the fixed active-record path across processes; the complete sealed record is then flushed and read back before mutation. A partial record left by a crash remains a fail-closed claim. RAII attempts recovery on unwind or ordinary error; a durable active marker covers process termination and blocks later runs until the recovery command succeeds.

The record binds the verification ID and ownership token to the original snapshot, exact two-identity fixture allowlist, expected display, and swap metadata. Mutation, recovery, archive, and active-claim removal revalidate ownership. Recovery captures the full desktop first and rejects any non-fixture drift, grants write capability only for the two stored fixture identities, captures/diffs the full desktop afterward, archives evidence, and only then removes the active claim.

The single supported recovery failpoint, `DESKANCHOR_VERIFICATION_FAILPOINT=after-mutation`, is parsed before desktop capture and also requires the destructive opt-in. After the fixture mutation has settled and passed a complete diff, it transitions the recovery guard from `Armed` to `ManualRecoveryRequired`, returns a dedicated `RECOVERY_REQUIRED` result, and exits normally. The guard's Drop implementation deliberately leaves the active record untouched only in that explicit state. It does not panic, abort, terminate the process, or affect production restore. See `docs/verification.md`.

## Threading and unsafe code

Explorer Shell Automation is used from a newly created single-threaded COM apartment per operation. The worker explicitly uses a DPI-unaware context because that physical desktop-view coordinate regime was verified for both capture and write-back; this also isolates the core from Tauri's per-monitor-aware host context. Monitor scale is queried separately with `GetScaleFactorForMonitor`, which does not require changing the Shell coordinate regime. No COM interface or PIDL crosses the worker boundary. `unsafe` is limited to generated Win32/COM calls, callback pointer recovery, tagged union reads after checking their discriminant, DPI-context setup/restore, and task-allocator ownership wrappers. Each site has a local SAFETY justification.

## Privacy

The default store is `%LOCALAPPDATA%\DeskAnchor\snapshots`. Snapshot JSON contains display names and parsing identities and therefore is private local data. The app has no networking, telemetry, account, remote logging, or crash-upload code. UI commands return local results; production code does not log icon names or complete snapshots.
