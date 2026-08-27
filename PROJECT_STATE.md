# Project state

## Current phase

Phase 0 — Desktop API feasibility and project bootstrap.

## Completed

- Rust workspace with a reusable `deskanchor-core` crate and a thin Tauri v2 application.
- Supported Shell COM desktop discovery and icon enumeration using `IShellWindows` and `IFolderView`.
- Version 1 JSON snapshot model, validation, local atomic storage, display signature, pure diff/matching, guarded restore, and minimal React UI.
- Windows display capture through CCD (`QueryDisplayConfig`) with a GDI fallback.
- Unit tests, an ignored real-Windows capture test, developer probe, internal documentation, and Windows CI definition.

## Verified

- Real Explorer read-only capture on Windows 11 24H2 (build 26100): identity and position were obtained for every visible desktop item in the test environment.
- Guarded real Save → Move → Restore on the same machine: two ordinary filesystem icons were swapped through the supported Shell API, the changed positions survived recapture, and the original snapshot restored every icon to its starting coordinate. The recovery snapshot was written before mutation and retained locally. The final DPI-unaware Shell worker passed from both a default host and a simulated Per-Monitor V2 host.
- Pure Rust snapshot, validation, diff, matching, storage, and monitor-signature tests.
- Frontend checks and an optimized Tauri application build without an installer bundle.

## Known limitations

- Identity uses the Shell desktop-absolute parsing name. Rename/move changes filesystem identity, and exceptional third-party namespace extensions may not provide a durable parsing name.
- v0.1 blocks restore when the normalized display signature differs; it does not remap coordinates across monitor changes.
- Explorer restart during an operation can invalidate COM interfaces. Each operation reacquires the desktop view, but automatic retry is not implemented.
- Automatic Arrange or Align to Grid settings may change/ignore requested positions.
- CCD is unavailable in some remote/non-console sessions; fallback monitor identity then uses the less stable GDI source name.
- The Tauri window was production-built, but its visible button flow was not manually exercised; the equivalent core restore path was exercised under a simulated Per-Monitor V2 host process.

## Next step

Repeat the guarded two-icon round-trip verification on a small set of representative Windows 10/11 machines and record the behavior of Explorer settings (Auto Arrange/Align to Grid), DPI combinations, Explorer restart, and multi-monitor layouts before expanding Phase 1 UX.
