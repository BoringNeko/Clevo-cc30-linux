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
    CMD_FAN_CURVE_READ, CMD_FAN_STATUS, CMD_MAIN, SUB_FAN_MODE, SUB_POWER_MODE,
};
use clevo_proto::constants::PAYLOAD_LEN;
use clevo_proto::fan_curve::parse_curve;
use clevo_proto::fan_status::parse_fan_status;
use clevo_proto::message::{build_subcommand_payload, empty_payload};
use clevo_proto::response::response_first_record;
use clevo_transport::{Transport, TransportError};

use crate::config;
use crate::state::{DaemonState, Freshness};

/// Accepted fan-mode aliases and their `121/1` values.
pub const FAN_MODES: &[(&str, u8)] = &[("auto", 0), ("max", 1), ("maxq", 5), ("quiet", 8)];

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
