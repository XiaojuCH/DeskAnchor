use std::mem::size_of;

use anyhow::{Result, bail};
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    DISPLAYCONFIG_PATH_MODE_IDX_INVALID, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
    MONITORINFOEXW,
};
use windows::Win32::UI::Shell::GetScaleFactorForMonitor;
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

use super::model::{Monitor, MonitorIdentity, Rect};

pub(crate) fn capture_monitors() -> Result<Vec<Monitor>> {
    let gdi_monitors = enumerate_gdi_monitors()?;
    match query_display_config(&gdi_monitors) {
        Ok(monitors) if !monitors.is_empty() => Ok(monitors),
        _ => Ok(gdi_monitors
            .into_iter()
            .map(GdiMonitor::into_fallback)
            .collect()),
    }
}

#[derive(Clone)]
struct GdiMonitor {
    device_name: String,
    bounds: Rect,
    work_area: Rect,
    primary: bool,
    scale_percent: Option<u32>,
}

impl GdiMonitor {
    fn into_fallback(self) -> Monitor {
        Monitor {
            identity: MonitorIdentity {
                device_path: None,
                edid_manufacturer_id: None,
                edid_product_code_id: None,
                connector_instance: None,
            },
            device_name: self.device_name,
            friendly_name: None,
            bounds: self.bounds,
            work_area: self.work_area,
            primary: self.primary,
            scale_percent: self.scale_percent,
            dpi: None,
        }
    }
}

fn enumerate_gdi_monitors() -> Result<Vec<GdiMonitor>> {
    let mut monitors = Vec::<GdiMonitor>::new();
    let success = unsafe {
        // SAFETY: the callback receives a pointer to `monitors`, which lives for the
        // synchronous enumeration call and is used only on this thread.
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor_callback),
            LPARAM((&mut monitors as *mut Vec<GdiMonitor>) as isize),
        )
    };
    if !success.as_bool() {
        bail!("EnumDisplayMonitors failed")
    }
    if monitors.is_empty() {
        bail!("Windows reported no active desktop monitors")
    }
    Ok(monitors)
}

unsafe extern "system" fn monitor_callback(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = unsafe {
        // SAFETY: `data` is the live Vec pointer supplied by enumerate_gdi_monitors.
        &mut *(data.0 as *mut Vec<GdiMonitor>)
    };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let info_ok = unsafe {
        // SAFETY: MONITORINFOEXW begins with MONITORINFO and cbSize declares the
        // full allocation, so GetMonitorInfoW can fill the trailing device name.
        GetMonitorInfoW(monitor, &mut info.monitorInfo)
    };
    if info_ok.as_bool() {
        let scale_percent = unsafe {
            // SAFETY: `monitor` is the live handle provided by EnumDisplayMonitors.
            GetScaleFactorForMonitor(monitor)
        }
        .ok()
        .and_then(|scale| u32::try_from(scale.0).ok())
        .filter(|scale| *scale > 0);
        monitors.push(GdiMonitor {
            device_name: utf16z(&info.szDevice),
            bounds: rect(info.monitorInfo.rcMonitor),
            work_area: rect(info.monitorInfo.rcWork),
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            scale_percent,
        });
    }
    true.into()
}

fn query_display_config(gdi_monitors: &[GdiMonitor]) -> Result<Vec<Monitor>> {
    let (paths, modes) = active_display_paths()?;
    let mut monitors = Vec::with_capacity(paths.len());
    for path in paths {
        let source_name = source_device_name(&path)?;
        let target = target_device_name(&path).ok();
        let source_mode_index = unsafe {
            // SAFETY: QDC_ONLY_ACTIVE_PATHS without virtual-mode flags populates the
            // modeInfoIdx member of this tagged union.
            path.sourceInfo.Anonymous.modeInfoIdx
        };
        if source_mode_index == DISPLAYCONFIG_PATH_MODE_IDX_INVALID {
            continue;
        }
        let Some(mode) = modes.get(source_mode_index as usize) else {
            bail!("QueryDisplayConfig returned an out-of-range source mode index")
        };
        if mode.infoType != DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
            bail!("QueryDisplayConfig source path referenced a non-source mode")
        }
        let source_mode = unsafe {
            // SAFETY: infoType was checked immediately above.
            mode.Anonymous.sourceMode
        };
        let bounds = Rect {
            left: source_mode.position.x,
            top: source_mode.position.y,
            right: source_mode.position.x + source_mode.width as i32,
            bottom: source_mode.position.y + source_mode.height as i32,
        };
        let gdi = gdi_monitors
            .iter()
            .find(|monitor| monitor.device_name.eq_ignore_ascii_case(&source_name));
        let target_path = target
            .as_ref()
            .map(|name| utf16z(&name.monitorDevicePath))
            .filter(|name| !name.is_empty());
        let friendly_name = target
            .as_ref()
            .map(|name| utf16z(&name.monitorFriendlyDeviceName))
            .filter(|name| !name.is_empty());
        monitors.push(Monitor {
            identity: MonitorIdentity {
                device_path: target_path,
                edid_manufacturer_id: target.as_ref().and_then(|name| {
                    (name.edidManufactureId != 0).then_some(name.edidManufactureId)
                }),
                edid_product_code_id: target.as_ref().and_then(|name| {
                    (name.edidProductCodeId != 0).then_some(name.edidProductCodeId)
                }),
                connector_instance: target.as_ref().map(|name| name.connectorInstance),
            },
            device_name: source_name,
            friendly_name,
            bounds,
            work_area: gdi.map_or(bounds, |monitor| monitor.work_area),
            primary: gdi.map_or(bounds.left == 0 && bounds.top == 0, |monitor| {
                monitor.primary
            }),
            scale_percent: gdi.and_then(|monitor| monitor.scale_percent),
            dpi: None,
        });
    }
    Ok(monitors)
}

fn active_display_paths() -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>)> {
    for _ in 0..3 {
        let mut path_count = 0;
        let mut mode_count = 0;
        let status = unsafe {
            // SAFETY: both output pointers refer to initialized u32 values.
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        };
        if status != ERROR_SUCCESS {
            bail!(
                "GetDisplayConfigBufferSizes failed with Win32 error {}",
                status.0
            )
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        let status = unsafe {
            // SAFETY: the arrays have the capacities reported above; the counts are
            // in/out values and no topology output is requested for this flag.
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if status != ERROR_SUCCESS {
            bail!("QueryDisplayConfig failed with Win32 error {}", status.0)
        }
        paths.truncate(path_count as usize);
        modes.truncate(mode_count as usize);
        return Ok((paths, modes));
    }
    bail!("display configuration changed repeatedly during capture")
}

fn source_device_name(path: &DISPLAYCONFIG_PATH_INFO) -> Result<String> {
    let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
            adapterId: path.sourceInfo.adapterId,
            id: path.sourceInfo.id,
        },
        ..Default::default()
    };
    let status = unsafe {
        // SAFETY: the packet header size and type match the backing allocation.
        DisplayConfigGetDeviceInfo(&mut name.header)
    };
    if status != 0 {
        bail!("DisplayConfigGetDeviceInfo(source) failed with Win32 error {status}")
    }
    Ok(utf16z(&name.viewGdiDeviceName))
}

fn target_device_name(path: &DISPLAYCONFIG_PATH_INFO) -> Result<DISPLAYCONFIG_TARGET_DEVICE_NAME> {
    let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
            adapterId: path.targetInfo.adapterId,
            id: path.targetInfo.id,
        },
        ..Default::default()
    };
    let status = unsafe {
        // SAFETY: the packet header size and type match the backing allocation.
        DisplayConfigGetDeviceInfo(&mut name.header)
    };
    if status != 0 {
        bail!("DisplayConfigGetDeviceInfo(target) failed with Win32 error {status}")
    }
    Ok(name)
}

fn utf16z(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

fn rect(value: RECT) -> Rect {
    Rect {
        left: value.left,
        top: value.top,
        right: value.right,
        bottom: value.bottom,
    }
}
