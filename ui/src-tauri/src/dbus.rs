//! Read-only client for the `org.clevo.CC` daemon.
//!
//! The UI backend never touches hardware: it speaks to `clevod` over the system
//! D-Bus and maps the replies into the `serde` types the frontend expects. The
//! property cache is disabled so every call reflects the daemon's current state
//! (a cached value would defeat the freshness model).

use serde::Deserialize;

/// Well-known name of the daemon.
pub const DBUS_NAME: &str = "org.clevo.CC";
/// Object path of the daemon's managed object.
pub const DBUS_PATH: &str = "/org/clevo/CC";
/// Interface name.
pub const DBUS_INTERFACE: &str = "org.clevo.CC";

/// A typed error for the UI layer.
#[derive(Debug, serde::Serialize)]
pub struct UiError {
    pub message: String,
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<zbus::Error> for UiError {
    fn from(value: zbus::Error) -> Self {
        Self {
            message: match &value {
                zbus::Error::InterfaceNotFound => {
                    "clevod is not running (org.clevo.CC not on the bus)".into()
                }
                other => other.to_string(),
            },
        }
    }
}

impl From<serde_json::Error> for UiError {
    fn from(value: serde_json::Error) -> Self {
        Self {
            message: format!("could not decode daemon reply: {value}"),
        }
    }
}

/// One fan channel, as sent to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FanReading {
    pub rpm: u32,
    /// Temperature in °C, or `null` when the EC reports none.
    ///
    /// The CPU value has already been converted with the configured TDP curve.
    pub temp_c: Option<u8>,
    pub available: bool,
}

/// The daemon's cached fan state, as sent to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FanSnapshot {
    pub cpu: FanReading,
    pub gpu1: FanReading,
    pub gpu2: FanReading,
    pub freshness: String,
    pub fan_count: u8,
    pub fan_mode: u8,
    pub perf_mode: u8,
    pub writable: bool,
    /// Whether a custom fan curve can be written.
    pub curve_writable: bool,
}

/// A single curve point, as sent to the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CurvePoint {
    pub temp: u8,
    pub duty_pct: u8,
}

/// The parsed fan curve, as sent to the frontend (and accepted back for writes).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FanCurve {
    pub fan_count: u8,
    pub init_mode: u8,
    pub kb_type: u8,
    pub cpu: Vec<CurvePoint>,
    pub gpu1: Vec<CurvePoint>,
    pub gpu2: Vec<CurvePoint>,
}

/// The shape of the daemon's `GetCurve` JSON string.
#[derive(Debug, Deserialize)]
struct CurveJson {
    fan_count: u8,
    init_mode: u8,
    kb_type: u8,
    cpu: Vec<[u8; 2]>,
    gpu1: Vec<[u8; 2]>,
    gpu2: Vec<[u8; 2]>,
}

/// Blocking wrapper around the async zbus proxy.
pub struct DaemonClient {
    connection: zbus::blocking::Connection,
    destination: String,
}

impl DaemonClient {
    /// Connect to the daemon on the system bus.
    pub fn system() -> Result<Self, UiError> {
        Self::system_with_name(DBUS_NAME)
    }

    /// Connect to a daemon on the system bus with a custom well-known name.
    pub fn system_with_name(name: &str) -> Result<Self, UiError> {
        let connection = zbus::blocking::Connection::system()?;
        Ok(Self {
            connection,
            destination: name.to_string(),
        })
    }

    /// Connect to the daemon on the session bus (used by tests).
    pub fn session() -> Result<Self, UiError> {
        Self::session_with_name(DBUS_NAME)
    }

    /// Connect to a daemon on the session bus with a custom well-known name.
    pub fn session_with_name(name: &str) -> Result<Self, UiError> {
        let connection = zbus::blocking::Connection::session()?;
        Ok(Self {
            connection,
            destination: name.to_string(),
        })
    }

    fn proxy(&self) -> Result<zbus::blocking::Proxy<'_>, UiError> {
        Ok(zbus::blocking::Proxy::new(
            &self.connection,
            self.destination.as_str(),
            DBUS_PATH,
            DBUS_INTERFACE,
        )?)
    }

    fn prop<T>(&self, name: &str) -> Result<T, UiError>
    where
        T: TryFrom<zbus::zvariant::OwnedValue> + zbus::zvariant::Type,
        <T as TryFrom<zbus::zvariant::OwnedValue>>::Error: std::fmt::Display,
    {
        let value: zbus::zvariant::OwnedValue = self.proxy()?.get_property(name)?;
        T::try_from(value).map_err(|e| UiError {
            message: format!("could not decode property {name}: {e}"),
        })
    }

    /// Ask the daemon to poll the hardware once.
    pub fn poll(&self) -> Result<(), UiError> {
        self.proxy()?.call_method("Poll", &())?;
        Ok(())
    }

    /// Set the fan mode by name (`auto`/`max`/`maxq`/`quiet`).
    ///
    /// The daemon authorizes the write through PolicyKit and returns the numeric
    /// `121/1` value it applied. A denied or unsupported request comes back as a
    /// D-Bus error, which [`UiError`] turns into a readable message.
    pub fn set_fan_mode(&self, mode: &str) -> Result<u8, UiError> {
        let reply = self.proxy()?.call_method("SetFanMode", &(mode,))?;
        reply.body().deserialize().map_err(|e| UiError {
            message: format!("could not decode SetFanMode reply: {e}"),
        })
    }

    /// Set the performance mode by name.
    pub fn set_perf_mode(&self, mode: &str) -> Result<u8, UiError> {
        let reply = self.proxy()?.call_method("SetPerfMode", &(mode,))?;
        reply.body().deserialize().map_err(|e| UiError {
            message: format!("could not decode SetPerfMode reply: {e}"),
        })
    }

    /// Write a custom fan curve and select the `custom` fan mode.
    ///
    /// The curve is sent as the daemon's JSON wire shape. The daemon validates
    /// it and authorizes the write through PolicyKit; a denial or a malformed
    /// curve comes back as a D-Bus error.
    pub fn set_curve(&self, curve_json: &str) -> Result<(), UiError> {
        self.proxy()?.call_method("SetCurve", &(curve_json,))?;
        Ok(())
    }

    /// Read the full cached snapshot.
    ///
    /// The daemon's `FanCount` property is only populated after a curve read, so
    /// when it is `0` (unknown) we fill it in from `GetCurve`; the curve is
    /// cheap and changes rarely.
    pub fn snapshot(&self) -> Result<FanSnapshot, UiError> {
        let mut fan_count: u8 = self.prop("FanCount")?;
        if fan_count == 0 {
            if let Ok(curve) = self.curve() {
                fan_count = curve.fan_count;
            }
        }
        let reading = |rpm: u32, temp_c: u8, index: u8| FanReading {
            rpm,
            temp_c: (temp_c != 0).then_some(temp_c),
            available: fan_count == 0 || index <= fan_count,
        };
        Ok(FanSnapshot {
            cpu: reading(self.prop("CpuRpm")?, self.prop("CpuTempC")?, 1),
            gpu1: reading(self.prop("GpuRpm")?, self.prop("GpuTempC")?, 2),
            gpu2: FanReading {
                rpm: 0,
                temp_c: None,
                available: fan_count >= 3,
            },
            freshness: self.prop("FanFreshness")?,
            fan_count,
            fan_mode: self.prop("FanMode")?,
            perf_mode: self.prop("PerfMode")?,
            writable: self.prop("Writable")?,
            curve_writable: self.prop("CurveWritable")?,
        })
    }

    /// Read and parse the fan curve.
    pub fn curve(&self) -> Result<FanCurve, UiError> {
        let json: String = self
            .proxy()?
            .call_method("GetCurve", &())?
            .body()
            .deserialize()
            .map_err(|e| UiError {
                message: format!("could not decode curve reply: {e}"),
            })?;
        parse_curve_json(&json)
    }
}

/// Convert an EC duty value (raw 0..255) into a percentage for the UI.
///
/// The daemon and the EC work in raw 8-bit duty; the interface talks in percent
/// because that is what a person reads. Skipping this conversion put values
/// like 179 into a field named `duty_pct`, which the chart then drew off the
/// top of the plot (anything above 100 maps past the axis).
pub fn raw_duty_to_pct(raw: u8) -> u8 {
    ((raw as u32 * 100 + 127) / 255).min(100) as u8
}

/// Convert a UI percentage back into the EC's raw duty value.
pub fn pct_to_raw_duty(pct: u8) -> u8 {
    ((pct as u32 * 255 + 50) / 100).min(255) as u8
}

/// Parse the daemon's `GetCurve` JSON string into a [`FanCurve`].
///
/// Split out from the bus call so it can be unit tested without a daemon.
/// Duty values are converted from the EC's raw 0..255 to percent here, so
/// everything above this layer (UI included) can treat `duty_pct` as a real
/// percentage.
pub fn parse_curve_json(json: &str) -> Result<FanCurve, UiError> {
    let parsed: CurveJson = serde_json::from_str(json)?;
    let conv = |v: Vec<[u8; 2]>| {
        v.into_iter()
            .map(|[temp, duty]| CurvePoint {
                temp,
                duty_pct: raw_duty_to_pct(duty),
            })
            .collect()
    };
    Ok(FanCurve {
        fan_count: parsed.fan_count,
        init_mode: parsed.init_mode,
        kb_type: parsed.kb_type,
        cpu: conv(parsed.cpu),
        gpu1: conv(parsed.gpu1),
        gpu2: conv(parsed.gpu2),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_conversion_round_trips_and_saturates() {
        // The exact live values: the EC reports raw duty, the UI shows percent.
        assert_eq!(raw_duty_to_pct(0), 0);
        assert_eq!(raw_duty_to_pct(255), 100);
        assert_eq!(raw_duty_to_pct(102), 40); // 40% written, read back as raw
        assert_eq!(raw_duty_to_pct(179), 70);
        assert_eq!(raw_duty_to_pct(115), 45);
        assert_eq!(pct_to_raw_duty(0), 0);
        assert_eq!(pct_to_raw_duty(100), 255);
        assert_eq!(pct_to_raw_duty(40), 102);
        assert_eq!(pct_to_raw_duty(70), 179);
        // Values above 100 cannot arise from the UI, but must not overflow.
        assert_eq!(pct_to_raw_duty(255), 255);
    }

    #[test]
    fn parses_curve_json_from_the_daemon() {
        // The shape clevod emits, with duty as the EC's raw 0..255.
        let json = r#"{"fan_count":2,"init_mode":0,"kb_type":6,
            "cpu":[[40,63],[60,102],[80,179],[100,255]],
            "gpu1":[[40,63],[60,115],[80,191],[99,255]],
            "gpu2":[[0,0],[0,255],[0,128],[0,0]]}"#;
        let curve = parse_curve_json(json).expect("valid curve json");
        assert_eq!(curve.fan_count, 2);
        assert_eq!(curve.kb_type, 6);
        assert_eq!(curve.cpu.len(), 4);
        assert_eq!(curve.cpu[0].temp, 40);
        // Raw duty is converted to a percentage on the way in.
        assert_eq!(curve.cpu[0].duty_pct, 25);
        assert_eq!(curve.cpu[1].duty_pct, 40);
        assert_eq!(curve.cpu[2].duty_pct, 70);
        assert_eq!(curve.cpu[3].duty_pct, 100);
        assert_eq!(curve.gpu1[3].temp, 99);
    }

    #[test]
    fn rejects_malformed_curve_json() {
        assert!(parse_curve_json("not json").is_err());
        assert!(parse_curve_json(r#"{"fan_count":2}"#).is_err());
    }
}
