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
- `desktop/restore.rs`: guarded identity matching, `SelectAndPositionItems`, and post-write verification.
- `snapshot/model.rs`: schema v1 and validation.
- `snapshot/diff.rs`: pure identity grouping and change classification.
- `snapshot/storage.rs`: local, pretty JSON with same-directory temporary write and atomic rename.

## Operation flow

Capture reacquires the Explorer desktop view on a dedicated COM STA, enumerates child PIDLs, converts each to an `IShellItem`, reads names and coordinates, then drops all COM/PIDL resources before returning a plain Rust `DesktopState`.

Restore validates the snapshot, reacquires and re-enumerates the current desktop, computes the pure diff, and blocks if the display signature differs. Only moved items with exactly one snapshot identity and one current identity are sent to Explorer. Missing, new, and ambiguous items are counted but not touched. After the supported batch positioning call succeeds, every moved item is read back and counted as restored only when its coordinate matches.

## Threading and unsafe code

Explorer Shell Automation is used from a newly created single-threaded COM apartment per operation. The worker explicitly uses a DPI-unaware context because that physical desktop-view coordinate regime was verified for both capture and write-back; this also isolates the core from Tauri's per-monitor-aware host context. Monitor scale is queried separately with `GetScaleFactorForMonitor`, which does not require changing the Shell coordinate regime. No COM interface or PIDL crosses the worker boundary. `unsafe` is limited to generated Win32/COM calls, callback pointer recovery, tagged union reads after checking their discriminant, DPI-context setup/restore, and task-allocator ownership wrappers. Each site has a local SAFETY justification.

## Privacy

The default store is `%LOCALAPPDATA%\DeskAnchor\snapshots`. Snapshot JSON contains display names and parsing identities and therefore is private local data. The app has no networking, telemetry, account, remote logging, or crash-upload code. UI commands return local results; production code does not log icon names or complete snapshots.
