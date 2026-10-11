//! Blocking D-Bus client for the `clevo-cc` CLI.
//!
//! The daemon ([`clevod`](https://github.com/)) is the only process that talks
//! to the hardware. When the CLI is pointed at the `dbus` backend it becomes a
//! thin client of `org.clevo.CC` and never opens `/proc/acpi/call` itself.
//!
//! zbus is async, so this module owns a small Tokio runtime and `block_on`s each
//! call. That keeps the rest of the CLI synchronous and testable.

use clevo_transport::TransportError;

/// Well-known name of the daemon.
pub const DBUS_NAME: &str = "org.clevo.CC";
/// Object path of the daemon's managed object.
pub const DBUS_PATH: &str = "/org/clevo/CC";
/// Interface name.
pub const DBUS_INTERFACE: &str = "org.clevo.CC";

/// Errors from the D-Bus client.
#[derive(Debug)]
pub enum DbusError {
    /// The daemon is not reachable (not running, or no bus).
    Unavailable(String),
    /// The daemon rejected the call (including PolicyKit denials).
    Rejected(String),
    /// A reply could not be decoded.
    Decode(String),
}

impl std::fmt::Display for DbusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(m) => write!(f, "clevod is not available: {m}"),
            Self::Rejected(m) => write!(f, "daemon rejected the request: {m}"),
            Self::Decode(m) => write!(f, "could not decode daemon reply: {m}"),
        }
    }
}

impl std::error::Error for DbusError {}

impl From<DbusError> for TransportError {
    fn from(value: DbusError) -> Self {
        match value {
            DbusError::Unavailable(m) => TransportError::Io(m),
            DbusError::Rejected(_) => TransportError::PermissionDenied,
            DbusError::Decode(m) => TransportError::MalformedResponse(m),
        }
    }
}

/// A snapshot of the daemon's cached state, as read from D-Bus properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbusStatus {
    /// Whether the daemon can perform writes.
    pub writable: bool,
    /// `fresh` / `stale` / `unknown`.
    pub freshness: String,
    /// Fan mode value (`255` = unset).
    pub fan_mode: u8,
    /// Performance mode value (`255` = unset).
    pub perf_mode: u8,
    /// Number of fans (`0` = unknown).
    pub fan_count: u8,
    /// CPU rpm.
    pub cpu_rpm: u32,
    /// GPU1 rpm.
    pub gpu_rpm: u32,
    /// CPU temperature in °C (`0` = the EC reports none).
    pub cpu_temp_c: u8,
    /// GPU1 temperature in °C (`0` = the EC reports none).
    pub gpu_temp_c: u8,
    /// Whether a custom fan curve can be written.
    pub curve_writable: bool,
}

/// Blocking client over `org.clevo.CC`.
pub struct DbusClient {
    connection: zbus::blocking::Connection,
    destination: String,
}

impl DbusClient {
    /// Connect with a short method timeout for non-activating doctor probes.
    pub fn diagnostics(session: bool) -> Result<Self, DbusError> {
        let builder = if session {
            zbus::blocking::connection::Builder::session()
        } else {
            zbus::blocking::connection::Builder::system()
        }
        .map_err(|e| DbusError::Unavailable(e.to_string()))?;
        let connection = builder
            .method_timeout(std::time::Duration::from_secs(3))
            .build()
            .map_err(|e| DbusError::Unavailable(e.to_string()))?;
        Ok(Self {
            connection,
            destination: DBUS_NAME.into(),
        })
    }

    /// Resolve an existing daemon to its unique bus name without activating it.
    /// Subsequent diagnostic calls stay on that instance even if it exits.
    pub fn diagnostic_service(&self) -> Result<Option<Self>, DbusError> {
        let proxy = zbus::blocking::fdo::DBusProxy::new(&self.connection)
            .map_err(|e| DbusError::Unavailable(e.to_string()))?;
        let name: zbus::names::BusName<'_> = self
            .destination
            .as_str()
            .try_into()
            .map_err(|e: zbus::names::Error| DbusError::Decode(e.to_string()))?;
        if !proxy
            .name_has_owner(name.clone())
            .map_err(|e| DbusError::Unavailable(e.to_string()))?
        {
            return Ok(None);
        }
        let owner = proxy
            .get_name_owner(name)
            .map_err(|e| DbusError::Unavailable(e.to_string()))?;
        Ok(Some(Self {
            connection: self.connection.clone(),
            destination: owner.to_string(),
        }))
    }

    /// Connect to the daemon on the system bus.
    pub fn system() -> Result<Self, DbusError> {
        Self::system_with_name(DBUS_NAME)
    }

    /// Connect to the daemon on the bus selected by `session`.
    ///
    /// `session = true` targets the per-user session bus (a daemon started with
    /// `--session-bus`); `false` targets the system bus used in normal installs.
    pub fn connect(session: bool) -> Result<Self, DbusError> {
        if session {
            Self::session()
        } else {
            Self::system()
        }
    }

    /// Connect to a daemon on the system bus with a custom well-known name.
    pub fn system_with_name(name: &str) -> Result<Self, DbusError> {
        let connection = zbus::blocking::Connection::system()
            .map_err(|e| DbusError::Unavailable(e.to_string()))?;
        Ok(Self {
            connection,
            destination: name.to_string(),
        })
    }

    /// Connect to the daemon on the session bus (used by tests).
    pub fn session() -> Result<Self, DbusError> {
        Self::session_with_name(DBUS_NAME)
    }

    /// Connect to a daemon on the session bus with a custom well-known name.
    pub fn session_with_name(name: &str) -> Result<Self, DbusError> {
        let connection = zbus::blocking::Connection::session()
            .map_err(|e| DbusError::Unavailable(e.to_string()))?;
        Ok(Self {
            connection,
            destination: name.to_string(),
        })
    }

    fn proxy<'a>(&'a self, interface: &'a str) -> Result<zbus::blocking::Proxy<'a>, DbusError> {
        zbus::blocking::Proxy::new(
            &self.connection,
            self.destination.as_str(),
            DBUS_PATH,
            interface,
        )
        .map_err(|e| DbusError::Unavailable(e.to_string()))
    }

    /// Invoke `Poll` to refresh the daemon's cache.
    pub fn poll(&self) -> Result<(), DbusError> {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        proxy.call_method("Poll", &()).map_err(|e| classify(&e))?;
        Ok(())
    }

    /// Set the fan mode by name.
    pub fn set_fan_mode(&self, mode: &str) -> Result<u8, DbusError> {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        let reply = proxy
            .call_method("SetFanMode", &(mode,))
            .map_err(|e| classify(&e))?;
        reply
            .body()
            .deserialize()
            .map_err(|e| DbusError::Decode(e.to_string()))
    }

    /// Set the performance mode by name.
    pub fn set_perf_mode(&self, mode: &str) -> Result<u8, DbusError> {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        let reply = proxy
            .call_method("SetPerfMode", &(mode,))
            .map_err(|e| classify(&e))?;
        reply
            .body()
            .deserialize()
            .map_err(|e| DbusError::Decode(e.to_string()))
    }

    /// Fetch typed capabilities, preserving unknown fields as `None`.
    pub fn get_capabilities(&self) -> Result<clevo_proto::Capabilities, DbusError> {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        let reply = proxy
            .call_method("GetCapabilities", &())
            .map_err(|e| classify(&e))?;
        let json: String = reply
            .body()
            .deserialize()
            .map_err(|e| DbusError::Decode(e.to_string()))?;
        serde_json::from_str(&json).map_err(|e| DbusError::Decode(e.to_string()))
    }

    /// Read the standard battery and desktop power interfaces.
    pub fn power(&self) -> Result<clevod::power::PowerSnapshot, DbusError> {
        let json: String = self
            .proxy(DBUS_INTERFACE)?
            .call_method("GetPower", &())
            .map_err(|error| classify(&error))?
            .body()
            .deserialize()
            .map_err(|error| DbusError::Decode(error.to_string()))?;
        serde_json::from_str(&json).map_err(|error| DbusError::Decode(error.to_string()))
    }

    /// Apply standard battery thresholds through the authorized daemon.
    pub fn set_charge_thresholds(
        &self,
        battery: &str,
        start: Option<u8>,
        end: u8,
    ) -> Result<clevod::power::ChargeThresholds, DbusError> {
        self.set_charge_window(battery, start, end, None)
    }

    /// Apply a native enable state and charge window in one authorized operation.
    pub fn set_charge_window(
        &self,
        battery: &str,
        start: Option<u8>,
        end: u8,
        enabled: Option<bool>,
    ) -> Result<clevod::power::ChargeThresholds, DbusError> {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        let reply = match enabled {
            Some(enabled) => proxy.call_method(
                "SetChargeWindow",
                &(battery, start.unwrap_or(255), end, enabled),
            ),
            None => proxy.call_method("SetChargeThresholds", &(battery, start.unwrap_or(255), end)),
        }
        .map_err(|error| classify(&error))?;
        let json: String = reply
            .body()
            .deserialize()
            .map_err(|error| DbusError::Decode(error.to_string()))?;
        serde_json::from_str(&json).map_err(|error| DbusError::Decode(error.to_string()))
    }

    /// Fetch the fan curve as the daemon's JSON string.
    pub fn get_curve(&self) -> Result<String, DbusError> {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        let reply = proxy
            .call_method("GetCurve", &())
            .map_err(|e| classify(&e))?;
        reply
            .body()
            .deserialize()
            .map_err(|e| DbusError::Decode(e.to_string()))
    }

    /// Write a custom fan curve and select the `custom` fan mode.
    ///
    /// The daemon validates the curve and authorizes the write through
    /// PolicyKit; a denial or a malformed curve comes back as a D-Bus error.
    pub fn set_curve(&self, curve_json: &str) -> Result<(), DbusError> {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        proxy
            .call_method("SetCurve", &(curve_json,))
            .map_err(|e| classify(&e))?;
        Ok(())
    }

    fn prop<T>(&self, name: &str) -> Result<T, DbusError>
    where
        T: TryFrom<zbus::zvariant::OwnedValue> + zbus::zvariant::Type,
        <T as TryFrom<zbus::zvariant::OwnedValue>>::Error: std::fmt::Display,
    {
        let proxy = self.proxy(DBUS_INTERFACE)?;
        let value: zbus::zvariant::OwnedValue =
            proxy.get_property(name).map_err(|e| classify(&e))?;
        T::try_from(value).map_err(|e| DbusError::Decode(e.to_string()))
    }

    /// Read the full cached status.
    pub fn status(&self) -> Result<DbusStatus, DbusError> {
        Ok(DbusStatus {
            writable: self.prop("Writable")?,
            freshness: self.prop("FanFreshness")?,
            fan_mode: self.prop("FanMode")?,
            perf_mode: self.prop("PerfMode")?,
            fan_count: self.prop("FanCount")?,
            cpu_rpm: self.prop("CpuRpm")?,
            gpu_rpm: self.prop("GpuRpm")?,
            cpu_temp_c: self.prop("CpuTempC")?,
            gpu_temp_c: self.prop("GpuTempC")?,
            curve_writable: self.prop("CurveWritable")?,
        })
    }

    /// Whether clevod found the compatible ITE keyboard RGB controller.
    pub fn keyboard_available(&self) -> Result<bool, DbusError> {
        self.prop("KeyboardAvailable")
    }

    /// Keyboard type reported by firmware (`255` means unknown).
    pub fn keyboard_firmware_type(&self) -> Result<u8, DbusError> {
        self.prop("KeyboardFirmwareType")
    }
}

/// Map a zbus error to a [`DbusError`], distinguishing policy denials.
fn classify(err: &zbus::Error) -> DbusError {
    match err {
        zbus::Error::MethodError(name, detail, _) => {
            let code = name.as_str();
            let detail = detail.as_deref().unwrap_or("");
            if code.contains("AccessDenied")
                || code.contains("NotAuthorized")
                || code.contains("AuthFailed")
            {
                DbusError::Rejected(format!(
                    "{code}: {}",
                    if detail.is_empty() {
                        "authorization failed"
                    } else {
                        detail
                    }
                ))
            } else {
                DbusError::Rejected(format!("{code}: {detail}"))
            }
        }
        zbus::Error::InterfaceNotFound => {
            DbusError::Unavailable("org.clevo.CC is not exported (is clevod running?)".into())
        }
        zbus::Error::Failure(m) => DbusError::Rejected(m.clone()),
        other => DbusError::Unavailable(other.to_string()),
    }
}
