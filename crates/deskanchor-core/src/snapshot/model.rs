use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::desktop::{
    DesktopIcon, DesktopState, DisplayConfiguration, normalized_monitor_signature,
};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u32,
    pub created_at: String,
    pub display: DisplayConfiguration,
    pub icons: Vec<DesktopIcon>,
}

impl Snapshot {
    pub fn capture(state: DesktopState) -> Result<Self, SnapshotError> {
        let created_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| SnapshotError::InvalidCreatedAt(error.to_string()))?;
        let snapshot = Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            created_at,
            display: state.display,
            icons: state.icons,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_json(input: &str) -> Result<Self, SnapshotError> {
        let value: Value = serde_json::from_str(input)?;
        let version = value
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .ok_or(SnapshotError::MissingSchemaVersion)?;
        let version =
            u32::try_from(version).map_err(|_| SnapshotError::UnsupportedSchema(u32::MAX))?;
        if version != CURRENT_SCHEMA_VERSION {
            return Err(SnapshotError::UnsupportedSchema(version));
        }
        let snapshot: Self = serde_json::from_value(value)?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn to_pretty_json(&self) -> Result<String, SnapshotError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn validate(&self) -> Result<(), SnapshotError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(SnapshotError::UnsupportedSchema(self.schema_version));
        }
        OffsetDateTime::parse(&self.created_at, &Rfc3339)
            .map_err(|error| SnapshotError::InvalidCreatedAt(error.to_string()))?;
        if self.display.monitors.is_empty() {
            return Err(SnapshotError::NoMonitors);
        }
        let expected_signature = normalized_monitor_signature(&self.display.monitors);
        if self.display.signature != expected_signature {
            return Err(SnapshotError::DisplaySignatureMismatch);
        }
        for monitor in &self.display.monitors {
            if monitor.bounds.width() <= 0 || monitor.bounds.height() <= 0 {
                return Err(SnapshotError::InvalidMonitorBounds);
            }
            if let Some(dpi) = monitor.dpi
                && (dpi.x == 0 || dpi.y == 0)
            {
                return Err(SnapshotError::InvalidMonitorDpi);
            }
            if monitor.scale_percent == Some(0) {
                return Err(SnapshotError::InvalidMonitorScale);
            }
        }
        if self.icons.iter().any(|icon| icon.identity.value.is_empty()) {
            return Err(SnapshotError::EmptyIconIdentity);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("snapshot JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("snapshot has no numeric schemaVersion")]
    MissingSchemaVersion,
    #[error("snapshot schema version {0} is not supported")]
    UnsupportedSchema(u32),
    #[error("snapshot createdAt is not valid RFC 3339: {0}")]
    InvalidCreatedAt(String),
    #[error("snapshot display configuration contains no monitors")]
    NoMonitors,
    #[error("snapshot monitor bounds must have positive width and height")]
    InvalidMonitorBounds,
    #[error("snapshot monitor DPI values must be positive")]
    InvalidMonitorDpi,
    #[error("snapshot monitor scale percentage must be positive")]
    InvalidMonitorScale,
    #[error("snapshot display signature does not match its monitor data")]
    DisplaySignatureMismatch,
    #[error("snapshot contains an empty icon identity")]
    EmptyIconIdentity,
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::desktop::{CoordinateSpace, Dpi, IconIdentity, Monitor, MonitorIdentity, Rect};

    use super::*;

    pub(crate) fn sample_snapshot() -> Snapshot {
        let monitors = vec![Monitor {
            identity: MonitorIdentity {
                device_path: Some("monitor-a".into()),
                edid_manufacturer_id: Some(1),
                edid_product_code_id: Some(2),
                connector_instance: Some(0),
            },
            device_name: r"\\.\DISPLAY1".into(),
            friendly_name: Some("Panel".into()),
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            work_area: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            primary: true,
            scale_percent: Some(100),
            dpi: Some(Dpi { x: 96, y: 96 }),
        }];
        Snapshot {
            schema_version: CURRENT_SCHEMA_VERSION,
            created_at: "2026-08-27T01:02:03Z".into(),
            display: DisplayConfiguration {
                signature: normalized_monitor_signature(&monitors),
                coordinate_space: CoordinateSpace::ExplorerDesktopView,
                monitors,
            },
            icons: vec![DesktopIcon {
                identity: IconIdentity::shell_parsing_name("C:\\Desktop\\a.txt".into()),
                display_name: "a.txt".into(),
                x: 10,
                y: 20,
            }],
        }
    }

    #[test]
    fn snapshot_round_trips_as_human_readable_json() {
        let snapshot = sample_snapshot();
        let json = snapshot.to_pretty_json().expect("serialize snapshot");
        assert!(json.contains("\"schemaVersion\": 1"));
        assert!(json.contains("\"displayName\": \"a.txt\""));
        assert_eq!(
            Snapshot::from_json(&json).expect("parse snapshot"),
            snapshot
        );
    }

    #[test]
    fn unsupported_schema_is_rejected_before_deserialization() {
        let json = r#"{"schemaVersion":999,"createdAt":"ignored"}"#;
        assert!(matches!(
            Snapshot::from_json(json),
            Err(SnapshotError::UnsupportedSchema(999))
        ));
    }

    #[test]
    fn invalid_snapshot_is_rejected() {
        let mut snapshot = sample_snapshot();
        snapshot.icons[0].identity.value.clear();
        let json = serde_json::to_string(&snapshot).expect("serialize invalid test input");
        assert!(matches!(
            Snapshot::from_json(&json),
            Err(SnapshotError::EmptyIconIdentity)
        ));
    }
}
