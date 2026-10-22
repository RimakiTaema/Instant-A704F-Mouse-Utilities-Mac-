use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SUPPORTED_VENDOR_ID: u16 = 0x18f8;
pub const SUPPORTED_PRODUCT_ID: u16 = 0x1286;
pub const CONFIG_INTERFACE: i32 = 0;
pub const INPUT_INTERFACE: i32 = 1;
pub const FEATURE_REPORT_ID: u8 = 0x21;
pub const FEATURE_REPORT_SIZE: usize = 16;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("unsupported device {vendor_id:04x}:{product_id:04x}; writes are disabled")]
    UnsupportedDevice { vendor_id: u16, product_id: u16 },
    #[error("invalid A704F setting: {0}")]
    InvalidSetting(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
}

impl DeviceIdentity {
    pub fn ensure_supported(self) -> Result<(), ProtocolError> {
        if self.vendor_id == SUPPORTED_VENDOR_ID && self.product_id == SUPPORTED_PRODUCT_ID {
            Ok(())
        } else {
            Err(ProtocolError::UnsupportedDevice {
                vendor_id: self.vendor_id,
                product_id: self.product_id,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureReport {
    command: u8,
    data: [u8; 6],
}

impl FeatureReport {
    fn new(command: u8, data: [u8; 6]) -> Self {
        Self { command, data }
    }

    pub fn encode(self) -> [u8; FEATURE_REPORT_SIZE] {
        let mut report = [0; FEATURE_REPORT_SIZE];
        report[0] = FEATURE_REPORT_ID;
        report[1] = 0x09;
        report[2] = 0x07;
        report[3] = 0x03;
        report[4] = 0x01;
        report[6] = 0x08;
        report[8] = 0x07;
        report[9] = self.command;
        report[10..16].copy_from_slice(&self.data);
        report
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DpiProfile {
    pub value: u8,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub rgb_mode: u8,
    pub scroll_mode: u8,
    pub scroll_debug: u8,
    pub fire_rate: u8,
    pub active_dpi: u8,
    pub dpi_profiles: [DpiProfile; 4],
    pub bindings: Vec<(u8, u8)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rgb_mode: 0x10,
            scroll_mode: 0,
            scroll_debug: 0,
            fire_rate: 4,
            active_dpi: 0,
            dpi_profiles: [
                DpiProfile {
                    value: 3,
                    enabled: true,
                },
                DpiProfile {
                    value: 4,
                    enabled: true,
                },
                DpiProfile {
                    value: 6,
                    enabled: true,
                },
                DpiProfile {
                    value: 12,
                    enabled: true,
                },
            ],
            bindings: vec![(1, 1), (2, 2), (3, 3), (5, 4), (4, 5), (8, 11), (6, 10)],
        }
    }
}

impl Settings {
    pub fn reports(&self) -> Result<Vec<FeatureReport>, ProtocolError> {
        if !(0x10..=0x17).contains(&self.rgb_mode) {
            return Err(ProtocolError::InvalidSetting(
                "RGB mode must be 0x10..=0x17",
            ));
        }
        if self.scroll_mode > 2 {
            return Err(ProtocolError::InvalidSetting("scroll mode must be 0..=2"));
        }
        if (self.scroll_mode == 2
            && !matches!(self.scroll_debug, 1..=4 | 6..=9 | 0x0b..=0x0d | 0x10..=0x13))
            || (self.scroll_mode != 2 && self.scroll_debug != 0)
        {
            return Err(ProtocolError::InvalidSetting(
                "unsupported debug scroll action",
            ));
        }
        if self.active_dpi > 3 {
            return Err(ProtocolError::InvalidSetting(
                "active DPI profile must be 0..=3",
            ));
        }
        if self.bindings.len() != 7 {
            return Err(ProtocolError::InvalidSetting(
                "exactly seven button bindings are required",
            ));
        }
        let mut seen_buttons = [false; 9];
        for (button, action) in &self.bindings {
            if !matches!(*button, 1..=5 | 6 | 8) || seen_buttons[*button as usize] {
                return Err(ProtocolError::InvalidSetting(
                    "button bindings must use each documented physical button once",
                ));
            }
            seen_buttons[*button as usize] = true;
            if *action > 0x0b {
                return Err(ProtocolError::InvalidSetting(
                    "button action must be 0x00..=0x0b",
                ));
            }
        }
        for profile in self.dpi_profiles {
            if !(1..=15).contains(&profile.value) {
                return Err(ProtocolError::InvalidSetting(
                    "DPI profile value must be 1..=15",
                ));
            }
        }

        let mut reports = vec![
            FeatureReport::new(0x13, [0x7f, self.rgb_mode, 0, 0, 0, 0]),
            FeatureReport::new(0x11, [self.scroll_mode, self.scroll_debug, 0, 0, 0, 0]),
            FeatureReport::new(0x12, [0, self.fire_rate, 0, 0, 0, 0]),
        ];
        reports.extend(
            self.bindings
                .iter()
                .map(|(button, action)| FeatureReport::new(0x10, [*button, *action, 0, 0, 0, 0])),
        );
        let enabled_profiles = self
            .dpi_profiles
            .iter()
            .enumerate()
            .fold(0u8, |mask, (index, profile)| {
                mask | (u8::from(profile.enabled) << index)
            });
        reports.extend(
            self.dpi_profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| {
                    let encoded_value = (profile.value << 4) | (0x08 + index as u8);
                    FeatureReport::new(
                        0x09,
                        [self.active_dpi, encoded_value, enabled_profiles, 0, 0, 0],
                    )
                }),
        );
        Ok(reports)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_documented_feature_report_frame() {
        let report = FeatureReport::new(0x13, [0x7f, 0x16, 0, 0, 0, 0]).encode();
        assert_eq!(
            report,
            [0x21, 0x09, 0x07, 0x03, 0x01, 0, 0x08, 0, 0x07, 0x13, 0x7f, 0x16, 0, 0, 0, 0]
        );
    }

    #[test]
    fn rejects_unsupported_hardware_identity() {
        assert_eq!(
            DeviceIdentity {
                vendor_id: 0x30fa,
                product_id: 0x1701
            }
            .ensure_supported(),
            Err(ProtocolError::UnsupportedDevice {
                vendor_id: 0x30fa,
                product_id: 0x1701
            })
        );
    }

    #[test]
    fn rejects_malformed_settings() {
        let settings = Settings {
            active_dpi: 4,
            ..Settings::default()
        };
        assert_eq!(
            settings.reports(),
            Err(ProtocolError::InvalidSetting(
                "active DPI profile must be 0..=3"
            ))
        );
    }

    #[test]
    fn rejects_undocumented_scroll_actions_and_unknown_json_fields() {
        let settings = Settings {
            scroll_mode: 2,
            scroll_debug: 5,
            ..Settings::default()
        };
        assert_eq!(
            settings.reports(),
            Err(ProtocolError::InvalidSetting(
                "unsupported debug scroll action"
            ))
        );

        let decoded =
            serde_json::from_str::<Settings>(r#"{"rgb_mode":22,"unverified_feature":true}"#);
        assert!(decoded.is_err());
    }

    #[test]
    fn maps_dpi_profiles_and_enabled_mask() {
        let reports = Settings::default().reports().unwrap();
        assert_eq!(reports[10].encode()[9..13], [0x09, 0, 0x38, 0x0f]);
    }
}
