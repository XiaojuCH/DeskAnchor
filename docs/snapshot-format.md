# Snapshot format v1

Snapshots are UTF-8, pretty-printed JSON stored by default in `%LOCALAPPDATA%\DeskAnchor\snapshots`. Writes use a temporary file in that directory, flush it, and rename it into place. The application has no upload or telemetry path.

Representative shape:

```json
{
  "schemaVersion": 1,
  "createdAt": "2026-08-27T07:30:00Z",
  "display": {
    "signature": "display-v1|...",
    "coordinateSpace": "explorerDesktopView",
    "monitors": [
      {
        "identity": {
          "devicePath": "\\\\?\\DISPLAY#...",
          "edidManufacturerId": 1234,
          "edidProductCodeId": 5678,
          "connectorInstance": 0
        },
        "deviceName": "\\\\.\\DISPLAY1",
        "friendlyName": "Example Monitor",
        "bounds": { "left": 0, "top": 0, "right": 2560, "bottom": 1440 },
        "workArea": { "left": 0, "top": 0, "right": 2560, "bottom": 1400 },
        "primary": true,
        "scalePercent": 150,
        "dpi": null
      }
    ]
  },
  "icons": [
    {
      "identity": {
        "kind": "shellDesktopAbsoluteParsingName",
        "value": "C:\\Users\\user\\Desktop\\example.txt"
      },
      "displayName": "example.txt",
      "x": 13,
      "y": 100
    }
  ]
}
```

`schemaVersion` is mandatory and checked before decoding the body. Unknown versions are rejected; unknown fields in version 1 are tolerated to allow additive tooling metadata. `createdAt` must be RFC 3339. Monitor geometry and DPI are validated, the stored signature must equal the normalized monitor data, and icon identities cannot be empty.

`identity.value` and `displayName` can expose private filenames or paths. They must not be sent to logs, crash reporters, telemetry, or remote services. The human-readable form is intentional for local diagnosis.

Matching is exact on the `(kind, value)` pair. Duplicate identity on either side is ambiguous. Deletion becomes `missing`; a new current item becomes `new`; rename/move currently appears as one missing plus one new. Virtual Shell items use their parsing identity. No fallback to display name or view index is allowed.

Positions are raw `IFolderView` desktop-view coordinates. Version 1 restores only when the current normalized display signature exactly matches the snapshot. It does not scale or translate coordinates when a monitor is absent or DPI/resolution/topology changes.
