//! Shared capability discovery for daemon and direct CLI backends.

use clevo_proto::capability::{capabilities_from_gcmd, parse_capabilities};
use clevo_proto::command::GCMD_CAPABILITY_COMMANDS;
use clevo_proto::message::empty_payload;
use clevo_proto::response::{response_integer, DSM_NOT_SUPPORTED};
use clevo_proto::{Capabilities, GcmdCapabilities};

use crate::error::TransportResult;
use crate::{Transport, TransportError, PAYLOAD_BYTES};

/// Prefer a real page 7; query GCMD only when AppSettings is unsupported.
///
/// I/O errors and malformed/unknown page versions remain errors. In particular,
/// a failed query must never turn into an all-supported feature bitmap.
pub fn read_capabilities<T: Transport + ?Sized>(transport: &T) -> TransportResult<Capabilities> {
    match transport.read_app_settings(7, 0, PAYLOAD_BYTES as u16) {
        Ok(page) => return parse_capabilities(&page).map_err(malformed),
        Err(TransportError::Unsupported(_)) => {}
        Err(error) => return Err(error),
    }

    read_gcmd_capabilities(transport)
}

pub(crate) fn read_gcmd_capabilities<T: Transport + ?Sized>(
    transport: &T,
) -> TransportResult<Capabilities> {
    let mut values = [0u32; 5];
    for (slot, command) in values.iter_mut().zip(GCMD_CAPABILITY_COMMANDS) {
        let response = transport.execute(command.get(), &empty_payload())?;
        let value = response_integer(&response).map_err(malformed)?;
        *slot = value;
    }
    decode_gcmd_capabilities(values)
}

pub(crate) fn decode_gcmd_capabilities(values: [u32; 5]) -> TransportResult<Capabilities> {
    for value in values {
        if value == DSM_NOT_SUPPORTED || value == u32::MAX {
            return Err(TransportError::FirmwareRejected(value as i32));
        }
    }
    Ok(capabilities_from_gcmd(GcmdCapabilities {
        features: values[0],
        extended_features: values[1],
        supported_apps: values[2],
        gpu_power_status: values[3],
        platform_features: values[4],
    }))
}

fn malformed(error: clevo_proto::ProtoError) -> TransportError {
    TransportError::MalformedResponse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockTransport;

    const GCMD: &str = include_str!("../../../fixtures/gcmd.fixture");

    #[test]
    fn gcmd_fixture_reconstructs_capabilities_without_appsettings() {
        let transport = MockTransport::from_fixture_str(GCMD).unwrap();
        let caps = transport.read_capabilities().unwrap();
        assert!(caps.power_modes.performance);
        let raw = caps.gcmd.unwrap();
        assert_eq!(raw.features, 0x0090_0011);
        assert_eq!(raw.extended_features, 0x3004);
        assert_eq!(raw.supported_apps, 0x883);
        assert_eq!(raw.gpu_power_status, 1);
        assert_eq!(raw.platform_features, 0x0800_0020);
    }

    #[test]
    fn a_real_page7_is_authoritative_and_bad_pages_do_not_fall_back() {
        let mut transport = MockTransport::from_fixture_str(GCMD).unwrap();
        let mut page = vec![0; PAYLOAD_BYTES];
        page[17] = 1;
        transport.insert_app_settings(7, 0, page.clone());
        let caps = transport.read_capabilities().unwrap();
        assert!(caps.power_modes.quiet);
        assert!(!caps.power_modes.performance);
        assert!(caps.gcmd.is_none());
        page[0] = 2;
        transport.insert_app_settings(7, 0, page);
        assert!(matches!(
            transport.read_capabilities(),
            Err(TransportError::MalformedResponse(_))
        ));
    }

    #[test]
    fn missing_malformed_and_rejected_gcmd_queries_are_errors() {
        for response in [
            "",
            "00",
            "0000000000000000010000000000040002000080",
            "00000000000000000100000000000400ffffffff",
        ] {
            let mut fixture = crate::mock::parse_fixture(GCMD).unwrap();
            if response.is_empty() {
                fixture.entries.retain(|entry| entry.command != 96);
            } else {
                let replacement =
                    crate::mock::parse_fixture(&format!("exec 60 * {response}\n")).unwrap();
                fixture
                    .entries
                    .iter_mut()
                    .find(|entry| entry.command == 96)
                    .unwrap()
                    .response = replacement.entries[0].response.clone();
            }
            assert!(MockTransport::from_fixture(fixture)
                .read_capabilities()
                .is_err());
        }
    }
}
