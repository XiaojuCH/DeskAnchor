use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopState {
    pub display: DisplayConfiguration,
    pub icons: Vec<DesktopIcon>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconIdentity {
    pub kind: IconIdentityKind,
    pub value: String,
}

impl IconIdentity {
    pub fn shell_parsing_name(value: String) -> Self {
        Self {
            kind: IconIdentityKind::ShellDesktopAbsoluteParsingName,
            value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IconIdentityKind {
    ShellDesktopAbsoluteParsingName,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopIcon {
    pub identity: IconIdentity,
    pub display_name: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayConfiguration {
    pub signature: String,
    pub coordinate_space: CoordinateSpace,
    pub monitors: Vec<Monitor>,
}

impl DisplayConfiguration {
    pub fn new(monitors: Vec<Monitor>) -> Self {
        let signature = normalized_monitor_signature(&monitors);
        Self {
            signature,
            coordinate_space: CoordinateSpace::ExplorerDesktopView,
            monitors,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoordinateSpace {
    /// Raw coordinates returned by IFolderView::GetItemPosition.
    ExplorerDesktopView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub identity: MonitorIdentity,
    pub device_name: String,
    pub friendly_name: Option<String>,
    pub bounds: Rect,
    pub work_area: Rect,
    pub primary: bool,
    pub scale_percent: Option<u32>,
    pub dpi: Option<Dpi>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorIdentity {
    pub device_path: Option<String>,
    pub edid_manufacturer_id: Option<u16>,
    pub edid_product_code_id: Option<u16>,
    pub connector_instance: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dpi {
    pub x: u32,
    pub y: u32,
}

/// Produces an order-independent signature for monitor identity, arrangement,
/// resolution, primary status, and effective scaling.
pub fn normalized_monitor_signature(monitors: &[Monitor]) -> String {
    let mut parts: Vec<String> = monitors.iter().map(monitor_signature_part).collect();
    parts.sort_unstable();
    format!("display-v1|{}", parts.join("|"))
}

fn monitor_signature_part(monitor: &Monitor) -> String {
    let identity = if let Some(path) = monitor.identity.device_path.as_deref() {
        format!("path:{}", normalize_text(path))
    } else if let (Some(manufacturer), Some(product)) = (
        monitor.identity.edid_manufacturer_id,
        monitor.identity.edid_product_code_id,
    ) {
        format!(
            "edid:{manufacturer:04x}:{product:04x}:{}",
            monitor.identity.connector_instance.unwrap_or_default()
        )
    } else {
        format!("source:{}", normalize_text(&monitor.device_name))
    };

    let mut part = format!(
        "{identity}@{},{},{}x{};primary={}",
        monitor.bounds.left,
        monitor.bounds.top,
        monitor.bounds.width(),
        monitor.bounds.height(),
        u8::from(monitor.primary)
    );
    if let Some(dpi) = monitor.dpi {
        let _ = write!(part, ";dpi={}x{}", dpi.x, dpi.y);
    }
    if let Some(scale_percent) = monitor.scale_percent {
        let _ = write!(part, ";scale={scale_percent}");
    }
    part
}

fn normalize_text(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(path: &str, left: i32, primary: bool) -> Monitor {
        Monitor {
            identity: MonitorIdentity {
                device_path: Some(path.into()),
                edid_manufacturer_id: Some(0x1234),
                edid_product_code_id: Some(0x5678),
                connector_instance: Some(0),
            },
            device_name: r"\\.\DISPLAY1".into(),
            friendly_name: Some("Panel".into()),
            bounds: Rect {
                left,
                top: 0,
                right: left + 1920,
                bottom: 1080,
            },
            work_area: Rect {
                left,
                top: 0,
                right: left + 1920,
                bottom: 1040,
            },
            primary,
            scale_percent: Some(100),
            dpi: None,
        }
    }

    #[test]
    fn monitor_signature_is_order_and_path_case_independent() {
        let left = monitor(r"\\?\DISPLAY#ABC", -1920, false);
        let primary = monitor(r"\\?\display#DEF", 0, true);
        let first = normalized_monitor_signature(&[left.clone(), primary.clone()]);
        let mut upper_left = left;
        upper_left.identity.device_path = Some(r"\\?\display#abc".into());
        let second = normalized_monitor_signature(&[primary, upper_left]);
        assert_eq!(first, second);
    }

    #[test]
    fn monitor_signature_changes_with_arrangement() {
        let first = normalized_monitor_signature(&[monitor("panel-a", 0, true)]);
        let second = normalized_monitor_signature(&[monitor("panel-a", 1920, true)]);
        assert_ne!(first, second);
    }
}
