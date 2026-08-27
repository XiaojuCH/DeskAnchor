# Manual Windows verification matrix

This is a test plan, not a claim of support. Change `TODO` only after running the guarded destructive verification in the stated environment and retaining its output/evidence record.

| OS | Display | DPI | Scenario | Status | Notes |
| --- | --- | --- | --- | --- | --- |
| Win11 | 1 | 100% | Baseline round-trip | TODO | |
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

## Historical Phase 0 evidence

Before this matrix existed, an exploratory guarded round-trip succeeded on Windows 11 Pro 24H2 build 26100 with one 100% display. Two ordinary filesystem icons were moved and the 251-item starting layout was recovered. That experiment established feasibility but did not use the Phase 0.5 fixed-fixture harness, so it does not mark any matrix row PASS.
