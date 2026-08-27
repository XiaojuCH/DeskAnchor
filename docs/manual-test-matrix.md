# Manual Windows verification matrix

This is a test plan, not a claim of support. Change `TODO` only after running the guarded destructive verification in the stated environment and retaining its output/evidence record.

| OS | Display | DPI | Scenario | Status | Notes |
| --- | --- | --- | --- | --- | --- |
| Win11 | 1 | 100% | Baseline round-trip | PASS | Windows 11 Pro 23H2 build 22631 VM; virtual 2283×1278; 6 icons; 3 attempts/323 ms; final diff exact; evidence: `verification-20260827T1059529044508Z-10848-verified.json` |
| Win11 | 1 | 100% | After-mutation recovery failpoint | PASS | Windows 11 Pro 23H2 build 22631 VM; mutation 3 attempts/322 ms; recovery 3 attempts/324 ms; final diff exact; active marker `True` → `False`; evidence: `verification-20260827T1152321625746Z-5368-recovered-by-command.json` |
| Win11 | 1 | 125% | Round-trip | TODO | |
| Win11 | 1 | 150% | Round-trip | TODO | |
| Win11 | 1 | 200% | Round-trip | TODO | |
| Win11 | 1 | 100% | Explorer restart before restore | TODO | |
| Win11 | 1 | 100% | Rename fixture | TODO | |
| Win11 | 1 | 100% | Delete fixture | TODO | |
| Win11 | 1 | 100% | Auto Arrange behavior | TODO | |
| Win11 | 1 | 100% | Align to Grid behavior | TODO | |
| Win11 | 2 | same DPI | Cross-monitor icons | TODO | |
| Win11 | 2 | mixed DPI | Cross-monitor icons | TODO | |
| Win10 | 1 | 100% | Baseline round-trip | TODO | |

For every run, record the Windows edition/build, display identities/topology, scale, Explorer settings, command, console summary, and retained recovery evidence path. A failure remains a result; do not rerun until the active recovery guard is resolved.

## Recorded Phase 0.5 baseline evidence

The fixed-fixture harness completed on an isolated Windows 11 Pro 23H2 build 22631 VM with one 2283×1278 virtual display at 100% scale and six desktop items. Fixture A started at `(698, 202)` and fixture B at `(1914, 202)`. Mutation and settle verification passed; recovery settled in three observations over 323 ms; the final six-item diff was exact; completion evidence was archived as `verification-20260827T1059529044508Z-10848-verified.json`; and the active marker no longer existed after the run.

This is evidence for only the first matrix row. It does not imply coverage for other DPI values, Explorer behavior changes, physical/multiple displays, RDP, or Windows 10.

## Recorded Phase 0.5 persistent recovery evidence

The `after-mutation` failpoint and cross-process recovery command completed on the same isolated Windows 11 Pro 23H2 build 22631 VM. The fixture mutation passed immediate readback and settled in three attempts over 322 ms; the full mutated diff was exact; and `active-recovery.json` was confirmed present. The operator also visually confirmed that the two fixture icons remained at their exchanged coordinates after the verification process exited. This visual observation supplements, but does not replace, the automated mutation checks.

`recover-last-verification` then settled in three attempts over 324 ms. Its final six-item diff was exact with six unchanged and zero moved, missing, new, or ambiguous items. Recovery evidence was archived as `verification-20260827T1152321625746Z-5368-recovered-by-command.json`, and the active marker changed from present to absent.

This is evidence only for the second matrix row and for the controlled cross-process recovery workflow. It does not reproduce every possible crash or power-loss timing.

## Historical Phase 0 evidence

Before this matrix existed, an exploratory guarded round-trip succeeded on Windows 11 Pro 24H2 build 26100 with one 100% display. Two ordinary filesystem icons were moved and the 251-item starting layout was recovered. That experiment established feasibility but did not use the Phase 0.5 fixed-fixture harness, so it does not mark any matrix row PASS.
