use crate::protocol::{DeviceIdentity, FeatureReport, ProtocolError, Settings};
use thiserror::Error;

pub trait FeatureTransport {
    fn send_feature_report(&mut self, report: &[u8; 16]) -> Result<(), String>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("hardware writes require the explicit --write option")]
    ConfirmationRequired,
    #[error("feature report write failed: {0}")]
    Write(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApplyResult {
    DryRun(Vec<FeatureReport>),
    Written(usize),
}

pub fn apply_settings(
    transport: &mut impl FeatureTransport,
    identity: DeviceIdentity,
    settings: &Settings,
    confirmed: bool,
) -> Result<ApplyResult, TransportError> {
    identity.ensure_supported()?;
    let reports = settings.reports()?;
    if !confirmed {
        return Ok(ApplyResult::DryRun(reports));
    }
    for report in &reports {
        transport
            .send_feature_report(&report.encode())
            .map_err(TransportError::Write)?;
    }
    Ok(ApplyResult::Written(reports.len()))
}

#[cfg(target_os = "macos")]
pub mod macos {
    use super::FeatureTransport;
    use crate::protocol::{DeviceIdentity, CONFIG_INTERFACE, FEATURE_REPORT_SIZE, INPUT_INTERFACE};
    use hidapi::{HidApi, HidDevice};

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DeviceInfo {
        pub identity: DeviceIdentity,
        pub interface: i32,
        pub manufacturer: String,
        pub product: String,
        pub usage_page: u16,
        pub usage: u16,
    }

    pub fn discover() -> Result<Vec<DeviceInfo>, String> {
        let api = HidApi::new().map_err(|error| error.to_string())?;
        Ok(api
            .device_list()
            .filter(|info| {
                (info.vendor_id() == 0x18f8 && info.product_id() == 0x1286)
                    || (info.vendor_id() == 0x30fa && info.product_id() == 0x1701)
            })
            .map(|info| DeviceInfo {
                identity: DeviceIdentity {
                    vendor_id: info.vendor_id(),
                    product_id: info.product_id(),
                },
                interface: info.interface_number(),
                manufacturer: info.manufacturer_string().unwrap_or("unknown").to_owned(),
                product: info.product_string().unwrap_or("unknown").to_owned(),
                usage_page: info.usage_page(),
                usage: info.usage(),
            })
            .collect())
    }

    pub struct ConfigDevice {
        _api: HidApi,
        device: HidDevice,
    }

    impl ConfigDevice {
        pub fn open() -> Result<Self, String> {
            let api = HidApi::new().map_err(|error| error.to_string())?;
            let info = api
                .device_list()
                .find(|info| {
                    info.vendor_id() == 0x18f8
                        && info.product_id() == 0x1286
                        && info.interface_number() == CONFIG_INTERFACE
                })
                .ok_or_else(|| {
                    "A704F configuration interface 0 (18f8:1286) not found".to_owned()
                })?;
            let device = api
                .open_path(info.path())
                .map_err(|error| error.to_string())?;
            Ok(Self { _api: api, device })
        }
    }

    impl FeatureTransport for ConfigDevice {
        fn send_feature_report(
            &mut self,
            report: &[u8; FEATURE_REPORT_SIZE],
        ) -> Result<(), String> {
            self.device
                .send_feature_report(report)
                .map_err(|error| error.to_string())
        }
    }

    pub fn interface_labels() -> (i32, i32) {
        (CONFIG_INTERFACE, INPUT_INTERFACE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockTransport {
        reports: Vec<[u8; 16]>,
    }

    impl FeatureTransport for MockTransport {
        fn send_feature_report(&mut self, report: &[u8; 16]) -> Result<(), String> {
            self.reports.push(*report);
            Ok(())
        }
    }

    #[test]
    fn dry_run_and_unsupported_device_never_write() {
        let mut transport = MockTransport::default();
        let supported = DeviceIdentity {
            vendor_id: 0x18f8,
            product_id: 0x1286,
        };
        let result =
            apply_settings(&mut transport, supported, &Settings::default(), false).unwrap();
        assert!(matches!(result, ApplyResult::DryRun(_)));
        assert!(transport.reports.is_empty());

        let unsupported = DeviceIdentity {
            vendor_id: 0x30fa,
            product_id: 0x1701,
        };
        assert!(matches!(
            apply_settings(&mut transport, unsupported, &Settings::default(), true),
            Err(TransportError::Protocol(
                ProtocolError::UnsupportedDevice { .. }
            ))
        ));
        assert!(transport.reports.is_empty());
    }

    #[test]
    fn explicit_confirmation_sends_only_validated_reports() {
        let mut transport = MockTransport::default();
        let identity = DeviceIdentity {
            vendor_id: 0x18f8,
            product_id: 0x1286,
        };
        let result = apply_settings(&mut transport, identity, &Settings::default(), true).unwrap();
        assert_eq!(result, ApplyResult::Written(14));
        assert_eq!(transport.reports.len(), 14);
        assert!(transport.reports.iter().all(|report| report[0] == 0x21));
    }
}
