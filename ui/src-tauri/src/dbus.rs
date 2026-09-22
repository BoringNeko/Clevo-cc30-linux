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

/// Parse the daemon's `GetCurve` JSON string into a [`FanCurve`].
///
/// Split out from the bus call so it can be unit tested without a daemon.
///
/// Duty arrives as a **percentage** and is passed through unchanged: the
/// daemon's D-Bus interface speaks percent, and `clevo-proto` is where that is
/// converted to and from the EC's raw 0..255. Converting here as well applied
/// it twice - a 100% point went out as 255 and the daemon rejected it with
/// "duty 255% out of range".
pub fn parse_curve_json(json: &str) -> Result<FanCurve, UiError> {
    let parsed: CurveJson = serde_json::from_str(json)?;
    let conv = |v: Vec<[u8; 2]>| {
        v.into_iter()
            .map(|[temp, duty_pct]| CurvePoint { temp, duty_pct })
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
    fn parses_curve_json_from_the_daemon() {
        // The exact shape clevod emits over D-Bus (verified live). Duty is a
        // percentage here - the raw 0..255 form only exists at the EC.
        let json = r#"{"fan_count":2,"init_mode":0,"kb_type":6,
            "cpu":[[40,25],[55,40],[75,70],[100,100]],
            "gpu1":[[40,25],[60,45],[80,75],[99,100]],
            "gpu2":[[0,0],[50,39],[70,67],[0,0]]}"#;
        let curve = parse_curve_json(json).expect("valid curve json");
        assert_eq!(curve.fan_count, 2);
        assert_eq!(curve.kb_type, 6);
        assert_eq!(curve.cpu.len(), 4);
        assert_eq!(curve.cpu[0].temp, 40);
        // Percent passes through untouched; 0..100 is the whole range.
        assert_eq!(curve.cpu[0].duty_pct, 25);
        assert_eq!(curve.cpu[3].duty_pct, 100);
        assert_eq!(curve.gpu1[3].temp, 99);
    }

    #[test]
    fn rejects_malformed_curve_json() {
        assert!(parse_curve_json("not json").is_err());
        assert!(parse_curve_json(r#"{"fan_count":2}"#).is_err());
    }
}
