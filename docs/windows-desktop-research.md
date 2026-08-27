# Windows desktop icon research

## Decision

Use the documented Shell view interfaces, specifically `IFolderView::GetItemPosition` and `IFolderView::SelectAndPositionItems`. Do not locate or message the desktop `SysListView32` control.

Microsoft documents `SWC_DESKTOP` as the Windows desktop Shell window and `IShellWindows::FindWindowSW` as the route to its `IDispatch`. The implementation follows the Microsoft Shell pattern:

1. `CoCreateInstance(CLSID_ShellWindows)`.
2. `IShellWindows::FindWindowSW(CSIDL_DESKTOP, SWC_DESKTOP, SWFO_NEEDDISPATCH)`.
3. Query the dispatch for `IServiceProvider`.
4. `QueryService(SID_STopLevelBrowser)` for `IShellBrowser`.
5. `IShellBrowser::QueryActiveShellView`, then query `IFolderView`.
6. `IFolderView::GetFolder` for the parent `IShellFolder`.

References: [IShellWindows](https://learn.microsoft.com/en-us/windows/win32/api/exdisp/nn-exdisp-ishellwindows), [FindWindowSW](https://learn.microsoft.com/en-us/windows/win32/api/exdisp/nf-exdisp-ishellwindows-findwindowsw), [SWC_DESKTOP](https://learn.microsoft.com/en-us/windows/win32/api/exdisp/ne-exdisp-shellwindowtypeconstants), and Microsoft's [desktop icon positioning sample](https://devblogs.microsoft.com/oldnewthing/20130318-00/?p=4933).

## Progman, WorkerW, SHELLDLL_DefView, and SysListView32

Explorer commonly exposes a desktop window tree involving `Progman` or `WorkerW`, `SHELLDLL_DefView`, and a `SysListView32` child. The exact parent can vary across Explorer versions and desktop composition states. This hierarchy is useful for diagnostics, but it is an implementation detail and is not used by DeskAnchor.

Old utilities locate those class names, allocate memory in Explorer, and send `LVM_GETITEMPOSITION`/`LVM_SETITEMPOSITION`. Besides architecture, integrity-level, pointer-size, and lifetime hazards, this bypasses Explorer's icon-position management. Microsoft states that starting in Windows 10 version 1809, positions sent directly to the ListView may be lost, and identifies `IFolderView::SelectAndPositionItems` as the supported API. See [Microsoft's 2021 compatibility note](https://devblogs.microsoft.com/oldnewthing/20211122-00/?p=105948).

## Enumeration, identity, and coordinates

`IFolderView::ItemCount(SVGIO_ALLVIEW)` and `Item(index)` enumerate child PIDLs. The index is used only to enumerate the current view; it is never persisted or used for matching. Each child PIDL is:

- passed back to the same view for `GetItemPosition`, which returns the top-left position in Explorer's desktop-view coordinate space;
- combined logically with its parent `IShellFolder` through `SHCreateItemWithParent` to obtain `IShellItem`;
- read with `SIGDN_NORMALDISPLAY` for UI text and `SIGDN_DESKTOPABSOLUTEPARSING` for identity.

The parsing identity normally becomes a full filesystem path for user/public/redirected desktop files and a canonical `::{CLSID}`-style name for virtual items such as Recycle Bin or This PC. This avoids same-display-name collisions and supports shortcuts as distinct `.lnk` shell items. It also covers the merged virtual desktop rather than only the physical user Desktop directory. References: [SIGDN](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/ne-shobjidl_core-sigdn), [SHCreateItemWithParent](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-shcreateitemwithparent), and the distinction between the [virtual and physical desktop](https://devblogs.microsoft.com/oldnewthing/20090730-00/?p=17293).

Limitations are intentional and visible: a filesystem rename/move changes the parsing identity; deleted items are missing; new items are untouched; duplicate identities are ambiguous and skipped. Some third-party namespace extensions can return parsing names that are not durable or round-trippable. Restore never parses a stored name into a PIDL: it matches the string against a fresh enumeration and positions the current PIDL, reducing exposure to that Shell edge case.

## Restore

The supported write is `IFolderView::SelectAndPositionItems` with `SVSI_POSITIONITEM`. DeskAnchor first validates the snapshot, captures current state, groups both sides by exact identity, computes a diff, checks the display signature, and supplies only unique moved matches. A successful HRESULT is not considered enough: the positions are queried again and mismatches are returned as immediate-verification failures. See [SelectAndPositionItems](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifolderview-selectandpositionitems) and [GetItemPosition](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifolderview-getitemposition).

An immediate readback still cannot rule out later grid snapping, refresh, or asynchronous rearrangement. The formal restore path therefore uses bounded settle verification: it periodically reacquires the Shell view, captures the complete desktop, and compares it to the expected snapshot. The default requires three consecutive exact full diffs at 150 ms intervals before a 2 second polling deadline; an observation completed at or after the deadline cannot settle. A display mismatch or any moved, missing, new, or ambiguous item prevents settled success. The deadline governs the observation loop between completed synchronous captures. It does not hard-cancel an in-progress Shell/COM call and is not a strict total wall-clock bound. These defaults bound normal polling latency rather than claiming Explorer can never drift after the observation window; the manual matrix must test that assumption across Explorer settings and Windows configurations.

No administrator privilege is required or requested. The operation targets the current interactive user's Explorer through Shell Automation. Session isolation, a missing Explorer desktop, or COM disconnection is reported as an error rather than bypassed with elevation.

## Display configuration and multi-monitor behavior

Active paths come from `GetDisplayConfigBufferSizes`/`QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)`. Source and target packets from `DisplayConfigGetDeviceInfo` provide the GDI source name, monitor device path, friendly name, EDID manufacturer/product identifiers, and connector instance. GDI monitor enumeration adds work area, primary status, and a monitor handle used with `GetScaleFactorForMonitor`. CCD geometry is in physical pixels and does not participate in DPI virtualization. References: [QueryDisplayConfig](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-querydisplayconfig), [target device identity fields](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_target_device_name), and [GetScaleFactorForMonitor](https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/nf-shellscalingapi-getscalefactorformonitor).

The signature sorts monitors, normalizes device-path case, and includes stable identity, bounds, resolution, primary status, and display scale when available. Device path is preferred; EDID plus connector is a fallback; the GDI source name is the last fallback. Device paths and EDID are not guaranteed stable through every dock, driver, KVM, virtual display, or identical-monitor scenario. Remote sessions can return `ERROR_ACCESS_DENIED` from CCD; the code then retains GDI geometry with a less stable source-name identity.

`IFolderView` coordinates proved DPI-context-sensitive even though CCD geometry itself is not virtualized. A rejected per-monitor-v2 worker experiment changed the desktop view from a 2560×1440 coordinate regime to 1600×900 logical coordinates and caused broad rearrangement when those values were written back. The final worker therefore pins only Shell operations to `DPI_AWARENESS_CONTEXT_UNAWARE`, the physical coordinate regime verified on this machine, regardless of the host executable's manifest. Actual monitor scaling is stored separately from `GetScaleFactorForMonitor`; `dpi` remains null when it cannot be obtained without context-dependent behavior. The guarded round trip is tested both from a default probe and a simulated per-monitor-v2 host.

Icon coordinates are stored exactly as returned by Explorer and labeled `explorerDesktopView`. Phase 0 does not claim they are interchangeable with CCD virtual-screen coordinates under every DPI/topology transition. Restore is therefore blocked when the display signature differs. Coordinate remapping belongs to a later, separately verified phase.

## Explorer restart and Windows compatibility

Every operation reacquires the current desktop Shell view, so no HWND, COM interface, or PIDL is cached across commands. If Explorer restarts during an active enumeration/write, COM can fail or the view can disappear; the error is returned and automatic retry is not yet implemented. A snapshot remains readable, but whether each identity still matches depends on the recreated Shell namespace.

Read-only enumeration was verified on Windows 11 24H2 build 26100. The selected interfaces are documented desktop APIs available well before Windows 10, but Windows 10 coverage and broader Windows 11 builds still require a compatibility matrix. Auto Arrange and Align to Grid can override or quantize positions and must be included in that matrix.

## Alternatives considered

- Direct `SysListView32` messages: rejected as unsupported and known to lose state on modern Windows.
- Reading only `%USERPROFILE%\Desktop`: rejected because it misses the merged public desktop and virtual Shell items and provides no view coordinates.
- Persisting ListView indices: rejected because view order changes.
- Persisting raw PIDL bytes: rejected as the primary identity because PIDLs are opaque namespace-provider data and are not generally a portable long-term serialization format.
- Display name matching: rejected as primary identity because names are localized and can collide across physical/virtual sources.
