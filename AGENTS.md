# DeskAnchor agent notes

DeskAnchor is a Windows desktop icon layout capture/restore tool. The immediate goal is a reliable manual Save → Move → Restore path on Windows 10/11, not a broad desktop customization product.

Before changing code, read `PROJECT_STATE.md`, `docs/architecture.md`, `docs/windows-desktop-research.md`, and `docs/snapshot-format.md`. Read `docs/verification.md` before touching destructive developer verification.

Rules:

- Keep Explorer/Win32 code in the Rust core; UI code must not know HWND, COM, or PIDL details.
- Prefer pure Rust for models, identity matching, diffing, validation, and storage decisions.
- Use the supported Shell `IFolderView` APIs. Do not manipulate `SysListView32`, use cross-process memory, or match by view index.
- Isolate `unsafe`, add a concrete SAFETY comment, and attach context to Windows errors.
- Do not use casual `unwrap()`/`expect()` in production code.
- Snapshots are local-only and may contain private filenames. No telemetry, uploads, network calls, or production logging of icon names/identities.
- Do not require elevation, add accounts/cloud sync, window restoration, auto-restore daemons, updaters, or Fences-like features without an explicit scope change.
- Do not present planned or unverified behavior as complete. Update `PROJECT_STATE.md` after material changes or verification.
- Never run destructive desktop verification on a user's normal host. It must remain ignored, require `DESKANCHOR_DESTRUCTIVE_TESTS=1`, operate only on the two named fixtures, and persist the complete recovery record before mutation.
- An existing verification `active-recovery.json` blocks every new destructive run. Recover and archive it; do not delete recovery evidence to bypass the guard.

Before handoff, run formatting, clippy with warnings denied, Rust tests, frontend lint/typecheck/tests/build, and a Tauri production build. Real Explorer tests are Windows-only and must be reported separately from unit tests. Never report the destructive harness as verified until it has run in an isolated test machine and the matrix has been updated from recorded output.
