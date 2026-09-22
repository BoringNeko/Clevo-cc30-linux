//! Daemon core: poll the hardware, cache state, apply writes.
//!
//! [`Service`] owns a [`Transport`] and a set of shared, lockable state. It is
//! intentionally free of D-Bus and async runtime types so the whole control
//! flow can be exercised in tests with a mock transport and a fixed clock.
//!
//! Write policy (mirrors the CLI):
//! * the target transport must report `writable()` before any write is sent;
//! * fan mode values are validated against the driver's accepted set;
//! * performance mode values are validated to `0..=3`; when a capability bitmap
//!   is available it is also checked, otherwise the firmware adjudicates.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use clevo_proto::capability::{parse_capabilities, Capabilities};
use clevo_proto::command::{
    CMD_FAN_CURVE_READ, CMD_FAN_CURVE_WRITE, CMD_FAN_STATUS, CMD_MAIN, SUB_FAN_MODE,
    SUB_POWER_MODE,
};
use clevo_proto::constants::PAYLOAD_LEN;
use clevo_proto::fan_curve::{encode_curve, parse_curve, FanCurve, FanPoint};
use clevo_proto::fan_status::parse_fan_status;
use clevo_proto::message::{build_subcommand_payload, empty_payload, payload_from_slice};
use clevo_proto::response::response_first_record;
use clevo_transport::{Transport, TransportError};

use crate::config;
use crate::state::{DaemonState, Freshness};

/// Accepted fan-mode aliases and their `121/1` values.
///
/// `custom` (6) is accepted but is only meaningful once a curve has been
/// written; the firmware selects "use the curve stored in the EC", and until
/// command `14` has run that curve is whatever the EC shipped with.
pub const FAN_MODES: &[(&str, u8)] = &[
    ("auto", 0),
    ("max", 1),
    ("custom", 6),
    ("maxq", 5),
    ("quiet", 8),
];

/// Accepted performance-mode names and their `121/25` values.
pub const PERF_MODES: &[(&str, u8)] = &[
    ("quiet", 0),
    ("pwrsaving", 1),
    ("performance", 2),
    ("entertainment", 3),
];

/// Resolve a fan-mode name to its `121/1` value.
pub fn fan_mode_value(name: &str) -> Option<u8> {
    FAN_MODES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// Resolve a `121/1` value back to its canonical name.
pub fn fan_mode_name(value: u8) -> Option<&'static str> {
    FAN_MODES.iter().find(|(_, v)| *v == value).map(|(n, _)| *n)
}

/// Resolve a performance-mode name to its `121/25` value.
pub fn perf_mode_value(name: &str) -> Option<u8> {
    PERF_MODES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// Resolve a `121/25` value back to its canonical name.
pub fn perf_mode_name(value: u8) -> Option<&'static str> {
    PERF_MODES
        .iter()
        .find(|(_, v)| *v == value)
        .map(|(n, _)| *n)
}

/// Shared, mutable daemon state.
pub type Shared = Arc<Mutex<DaemonState>>;

/// A monotonic millisecond clock.
pub trait Clock: Send + Sync {
    /// Milliseconds since an arbitrary fixed point.
    fn now_ms(&self) -> u64;
}

/// Clock backed by [`std::time::Instant`] since process start.
#[derive(Debug)]
pub struct SystemClock {
    start: std::time::Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

/// The daemon core.
pub struct Service {
    transport: Box<dyn Transport>,
    state: Shared,
    clock: Box<dyn Clock>,
    /// Last applied fan mode, mirrored for persistence.
    last_fan_mode: AtomicU64,
    /// Last applied perf mode; `u64::MAX` means "unset".
    last_perf_mode: AtomicU64,
}

/// Sentinel for "no mode applied yet" in the atomics above.
const UNSET: u64 = u64::MAX;

/// A write request rejected by policy before reaching the hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    /// The transport cannot write.
    NotWritable,
    /// The requested mode name is unknown.
    UnknownMode(String),
    /// The value is out of range or unsupported by this machine.
    Unsupported(String),
    /// The underlying transport failed.
    Transport(String),
    /// A response was malformed.
    Protocol(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWritable => write!(f, "transport is read-only"),
            Self::UnknownMode(m) => write!(f, "unknown mode {m:?}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Transport(m) => write!(f, "transport error: {m}"),
            Self::Protocol(m) => write!(f, "protocol error: {m}"),
        }
    }
}

impl std::error::Error for ServiceError {}

impl From<TransportError> for ServiceError {
    fn from(value: TransportError) -> Self {
        match value {
            TransportError::PermissionDenied => Self::NotWritable,
            TransportError::Unsupported(m) => Self::Unsupported(m),
            other => Self::Transport(other.to_string()),
        }
    }
}

impl From<clevo_proto::ProtoError> for ServiceError {
    fn from(value: clevo_proto::ProtoError) -> Self {
        Self::Protocol(value.to_string())
    }
}

impl Service {
    /// Create a service around `transport`.
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            state: Arc::new(Mutex::new(DaemonState::default())),
            clock: Box::new(SystemClock::default()),
            last_fan_mode: AtomicU64::new(UNSET),
            last_perf_mode: AtomicU64::new(UNSET),
        }
    }

    /// Replace the clock (used by tests).
    pub fn with_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// A handle to the shared state.
    pub fn state(&self) -> Shared {
        Arc::clone(&self.state)
    }

    /// Whether the transport can write.
    pub fn writable(&self) -> bool {
        self.transport.writable()
    }

    /// The transport kind.
    pub fn kind(&self) -> clevo_transport::TransportKind {
        self.transport.kind()
    }

    /// Poll fan status (command 12) and the curve (command 13, for fan count).
    ///
    /// A failed command 12 poll marks the cached readings stale; a failed curve
    /// read leaves the previous fan count in effect (best-effort).
    pub fn poll_fan(&self) -> Result<(), ServiceError> {
        let _ = self.clock.now_ms();

        match self.read_status() {
            Ok((status, fan_count)) => {
                // The firmware does not report the current mode reliably, but
                // some backends (the kernel driver) can observe it; refresh the
                // cached values so the UI can highlight the active mode.
                let modes = self.transport.current_modes().ok();
                let mut guard = self.state.lock().unwrap();
                guard.apply_status(&status, fan_count);
                if let Some((fan, perf)) = modes {
                    if fan.is_some() {
                        guard.fan_mode = fan;
                    }
                    if perf.is_some() {
                        guard.perf_mode = perf;
                    }
                }
                Ok(())
            }
            Err(err) => {
                let mut guard = self.state.lock().unwrap();
                guard.mark_fan_stale();
                Err(err)
            }
        }
    }

    fn read_status(&self) -> Result<(clevo_proto::FanStatus, u8), ServiceError> {
        let raw = self
            .transport
            .execute(CMD_FAN_STATUS.get(), &empty_payload())?;
        let payload = response_first_record(&raw)?;
        let status = parse_fan_status(payload)?;
        let fan_count = self.read_fan_count().unwrap_or(0);
        Ok((status, fan_count))
    }

    /// Read just the fan count from command 13.
    pub fn read_fan_count(&self) -> Result<u8, ServiceError> {
        let raw = self
            .transport
            .execute(CMD_FAN_CURVE_READ.get(), &empty_payload())?;
        let payload = response_first_record(&raw)?;
        Ok(parse_curve(payload)?.fan_count)
    }

    /// Read and cache the fan curve.
    pub fn read_curve(&self) -> Result<clevo_proto::FanCurveInfo, ServiceError> {
        let raw = self
            .transport
            .execute(CMD_FAN_CURVE_READ.get(), &empty_payload())?;
        let payload = response_first_record(&raw)?;
        let info = parse_curve(payload)?;
        self.state.lock().unwrap().curve = Some(info);
        Ok(info)
    }

    /// Write a custom fan curve (command `14`) and switch to the `custom` mode.
    ///
    /// The curve is fully validated (strictly increasing temperatures, duty
    /// `0..=100`) before anything is sent, and the hardware is only touched
    /// when the transport reports `writable()`. Selecting `custom` afterwards is
    /// what makes the firmware actually use the new table; without it the EC
    /// keeps interpolating from its auto curve.
    pub fn set_curve(&self, curve: &FanCurve) -> Result<(), ServiceError> {
        if !self.transport.writable() {
            return Err(ServiceError::NotWritable);
        }
        let payload = encode_curve(curve)?;
        self.transport.execute(
            CMD_FAN_CURVE_WRITE.get(),
            &payload_from_slice(&payload)?,
        )?;
        // Only select `custom` once the write itself succeeded: it is the mode
        // that makes the firmware use the table just written.
        const CUSTOM: u8 = 6;
        self.apply(SUB_FAN_MODE, CUSTOM)?;
        self.last_fan_mode.store(u64::from(CUSTOM), Ordering::SeqCst);
        self.state.lock().unwrap().fan_mode = Some(CUSTOM);
        Ok(())
    }

    /// Write a custom fan curve from the daemon's JSON wire format.
    ///
    /// The JSON uses the same shape `GetCurve` emits, i.e. four `[temp, duty]`
    /// pairs per fan. Anything malformed is rejected before the hardware is
    /// touched.
    pub fn set_curve_json(&self, json: &str) -> Result<(), ServiceError> {
        let curve = curve_from_json(json)?;
        self.set_curve(&curve)
    }

    /// Read the capability bitmap (`page 7`), if the channel is available.
    pub fn read_capabilities(&self) -> Result<Capabilities, ServiceError> {
        let page = self.transport.read_app_settings(7, 0, PAYLOAD_LEN as u16)?;
        Ok(parse_capabilities(&page)?)
    }

    /// Set the fan mode by name (`auto`/`max`/`maxq`/`quiet`).
    pub fn set_fan_mode(&self, name: &str) -> Result<u8, ServiceError> {
        let value = fan_mode_value(name).ok_or_else(|| ServiceError::UnknownMode(name.into()))?;
        self.apply(SUB_FAN_MODE, value)?;
        self.last_fan_mode.store(u64::from(value), Ordering::SeqCst);
        self.state.lock().unwrap().fan_mode = Some(value);
        Ok(value)
    }

    /// Set the performance mode by name.
    pub fn set_perf_mode(&self, name: &str) -> Result<u8, ServiceError> {
        let value = perf_mode_value(name).ok_or_else(|| ServiceError::UnknownMode(name.into()))?;
        if let Ok(caps) = self.read_capabilities() {
            if !caps.power_modes.supports(value) {
                return Err(ServiceError::Unsupported(format!(
                    "performance mode {name:?} is not advertised by this machine"
                )));
            }
        }
        self.apply(SUB_POWER_MODE, value)?;
        self.last_perf_mode
            .store(u64::from(value), Ordering::SeqCst);
        self.state.lock().unwrap().perf_mode = Some(value);
        Ok(value)
    }

    /// Send a `CMD_MAIN` sub-command write through the transport.
    fn apply(&self, sub: u8, value: u8) -> Result<(), ServiceError> {
        if !self.transport.writable() {
            return Err(ServiceError::NotWritable);
        }
        let payload = build_subcommand_payload(u32::from(value), sub);
        self.transport.execute(CMD_MAIN.get(), &payload)?;
        Ok(())
    }

    /// Re-apply persisted modes on startup (design decision D9).
    ///
    /// Failures are non-fatal and reported to the caller for logging.
    pub fn apply_saved(&self, config: &config::Config) -> Vec<String> {
        let mut failures = Vec::new();
        if !config.apply_on_start || !self.transport.writable() {
            return failures;
        }
        if let Some(value) = config.fan_mode {
            match fan_mode_name(value) {
                Some(name) => {
                    if let Err(e) = self.set_fan_mode(name) {
                        failures.push(format!("fan_mode {value}: {e}"));
                    }
                }
                None => failures.push(format!("fan_mode {value}: no such mode")),
            }
        }
        if let Some(value) = config.perf_mode {
            match perf_mode_name(value) {
                Some(name) => {
                    if let Err(e) = self.set_perf_mode(name) {
                        failures.push(format!("perf_mode {value}: {e}"));
                    }
                }
                None => failures.push(format!("perf_mode {value}: no such mode")),
            }
        }
        failures
    }

    /// Build the config representing the current in-memory modes.
    pub fn to_config(&self) -> config::Config {
        let fan = self.last_fan_mode.load(Ordering::SeqCst);
        let perf = self.last_perf_mode.load(Ordering::SeqCst);
        config::Config {
            fan_mode: (fan != UNSET).then_some(fan as u8),
            perf_mode: (perf != UNSET).then_some(perf as u8),
            ..config::Config::default()
        }
    }
}

impl DaemonState {
    /// Freshness of the cached fan readings (convenience for consumers).
    pub fn fan_freshness(&self) -> Freshness {
        self.fan.freshness
    }
}

/// Parse the daemon's curve JSON into a [`FanCurve`].
///
/// Accepted shape (exactly what `GetCurve` emits), where each fan carries four
/// `[temp, duty_pct]` pairs:
///
/// ```json
/// {"cpu":[[40,25],[60,36],[80,53],[100,100]],
///  "gpu1":[[40,25],[60,36],[80,53],[99,100]],
///  "gpu2":[[0,0],[0,0],[0,0],[0,0]]}
/// ```
///
/// `fan_count`, `init_mode` and `kb_type` are accepted and ignored so a curve
/// round-tripped from `GetCurve` can be edited in place and sent back. Parsing
/// is deliberately strict: a wrong number of points or an out-of-range value is
/// an error rather than a silently truncated curve.
pub fn curve_from_json(json: &str) -> Result<FanCurve, ServiceError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| ServiceError::Protocol(format!("curve json: {e}")))?;

    fn fan(value: &serde_json::Value, name: &str) -> Result<[FanPoint; 4], ServiceError> {
        let array = value
            .get(name)
            .and_then(|v| v.as_array())
            .ok_or_else(|| ServiceError::Protocol(format!("curve json: missing array {name:?}")))?;
        if array.len() != 4 {
            return Err(ServiceError::Protocol(format!(
                "curve json: {name} must have exactly 4 points, got {}",
                array.len()
            )));
        }
        let mut points = [FanPoint {
            temp: 0,
            duty_pct: 0,
        }; 4];
        for (i, entry) in array.iter().enumerate() {
            let pair = entry.as_array().ok_or_else(|| {
                ServiceError::Protocol(format!("curve json: {name}[{i}] is not an array"))
            })?;
            if pair.len() != 2 {
                return Err(ServiceError::Protocol(format!(
                    "curve json: {name}[{i}] must be [temp, duty]"
                )));
            }
            let temp = pair[0].as_u64().ok_or_else(|| {
                ServiceError::Protocol(format!("curve json: {name}[{i}].temp is not an integer"))
            })?;
            let duty = pair[1].as_u64().ok_or_else(|| {
                ServiceError::Protocol(format!("curve json: {name}[{i}].duty is not an integer"))
            })?;
            if temp > 255 {
                return Err(ServiceError::Protocol(format!(
                    "curve json: {name}[{i}].temp {temp} out of range"
                )));
            }
            if duty > 100 {
                return Err(ServiceError::Protocol(format!(
                    "curve json: {name}[{i}].duty {duty}% out of range"
                )));
            }
            points[i] = FanPoint {
                temp: temp as u8,
                duty_pct: duty as u8,
            };
        }
        Ok(points)
    }

    Ok(FanCurve {
        cpu: fan(&value, "cpu")?,
        gpu1: fan(&value, "gpu1")?,
        gpu2: fan(&value, "gpu2")?,
    })
}
