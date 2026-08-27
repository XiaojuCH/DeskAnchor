# Manual Windows verification matrix

This is a test plan, not a claim of support. Change `TODO` only after running the guarded destructive verification in the stated environment and retaining its output/evidence record.

| OS | Display | DPI | Scenario | Status | Notes |
| --- | --- | --- | --- | --- | --- |
| Win11 | 1 | 100% | Baseline round-trip | PASS | Windows 11 Pro 23H2 build 22631 VM; virtual 2283×1278; 6 icons; 3 attempts/323 ms; final diff exact; evidence: `verification-20260827T1059529044508Z-10848-verified.json` |
| Win11 | 1 | 100% | After-mutation recovery failpoint | TODO | Controlled orphan state followed by `recover-last-verification` |
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

## Historical Phase 0 evidence

Before this matrix existed, an exploratory guarded round-trip succeeded on Windows 11 Pro 24H2 build 26100 with one 100% display. Two ordinary filesystem icons were moved and the 251-item starting layout was recovered. That experiment established feasibility but did not use the Phase 0.5 fixed-fixture harness, so it does not mark any matrix row PASS.
