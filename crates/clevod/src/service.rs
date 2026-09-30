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

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use clevo_proto::capability::{parse_capabilities, Capabilities};
use clevo_proto::command::{
    CMD_FAN_CURVE_READ, CMD_FAN_CURVE_WRITE, CMD_FAN_STATUS, CMD_MAIN, SUB_FAN_MODE, SUB_POWER_MODE,
};
use clevo_proto::constants::PAYLOAD_LEN;
use clevo_proto::fan_curve::{encode_curve, parse_curve, FanCurve, FanPoint};
use clevo_proto::fan_status::parse_fan_status;
use clevo_proto::message::{build_subcommand_payload, empty_payload, payload_from_slice};
use clevo_proto::response::response_first_record;
use clevo_transport::{
    Color, Keyboard, KeyboardError, KeyboardMode, KeyboardSnapshot, KeyboardZone, Transport,
    TransportError,
};

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
    keyboard: Option<Box<dyn Keyboard>>,
    /// Keyboard type reported by the firmware's fan-curve capability record.
    /// This is separate from `keyboard`: the firmware can advertise RGB15
    /// even when Linux has no verified write transport for it.
    keyboard_firmware_type: std::sync::atomic::AtomicU8,
    state: Shared,
    clock: Box<dyn Clock>,
    /// Last applied fan mode, mirrored for persistence.
    last_fan_mode: AtomicU64,
    /// Last applied perf mode; `u64::MAX` means "unset".
    last_perf_mode: AtomicU64,
    /// CPU TDP class used to convert the raw CPU temperature byte.
    tdp_class: clevo_proto::TdpClass,
    /// Where to persist user choices; `None` disables persistence.
    config_path: Option<PathBuf>,
    /// The config as last loaded/seeded, used as the merge base so a save does
    /// not drop fields this process never touched (`cpu_tdp_class`, ...).
    baseline: Mutex<config::Config>,
    /// True while `apply_saved` replays stored values, so those writes do not
    /// bounce straight back to disk on every startup.
    applying: std::sync::atomic::AtomicBool,
}

/// Sentinel for "no mode applied yet" in the atomics above.
const UNSET: u64 = u64::MAX;
const UNKNOWN_KEYBOARD_TYPE: u8 = u8::MAX;

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
    /// The keyboard RGB capability is absent or rejected a request.
    Keyboard(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWritable => write!(f, "transport is read-only"),
            Self::UnknownMode(m) => write!(f, "unknown mode {m:?}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Transport(m) => write!(f, "transport error: {m}"),
            Self::Protocol(m) => write!(f, "protocol error: {m}"),
            Self::Keyboard(m) => write!(f, "keyboard error: {m}"),
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

impl From<KeyboardError> for ServiceError {
    fn from(value: KeyboardError) -> Self {
        Self::Keyboard(value.to_string())
    }
}

impl Service {
    /// Create a service around `transport`.
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            keyboard: None,
            keyboard_firmware_type: std::sync::atomic::AtomicU8::new(UNKNOWN_KEYBOARD_TYPE),
            state: Arc::new(Mutex::new(DaemonState::default())),
            clock: Box::new(SystemClock::default()),
            last_fan_mode: AtomicU64::new(UNSET),
            last_perf_mode: AtomicU64::new(UNSET),
            // No conversion by default: correct on the reference machine and
            // the vendor's own behaviour for an unlisted CPU. Config overrides.
            tdp_class: clevo_proto::TdpClass::Raw,
            config_path: None,
            baseline: Mutex::new(config::Config::default()),
            applying: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Attach an optionally discovered keyboard RGB backend.
    pub fn with_keyboard(mut self, keyboard: Option<Box<dyn Keyboard>>) -> Self {
        self.keyboard = keyboard;
        self
    }

    /// Persist user choices to `path` on every successful write.
    ///
    /// The daemon owns persistence (design decision D9): the EC forgets
    /// everything on reboot, so without this a saved curve and the mode that
    /// selects it would both be lost. Pass `None` to disable persistence
    /// (tests, `--mock`).
    pub fn with_config_path(mut self, path: Option<PathBuf>) -> Self {
        self.config_path = path;
        self
    }

    /// Seed the merge base from the config that was just loaded.
    ///
    /// Every save merges the live modes into this value, so fields the daemon
    /// does not manage (`cpu_tdp_class`, `apply_on_start`, ...) survive.
    pub fn set_baseline(&self, config: &config::Config) {
        *self.baseline.lock().unwrap() = config.clone();
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

    /// The CPU TDP class used to convert the raw CPU temperature.
    ///
    /// Starts as the COLORFUL P15 23's class and can be overridden with
    /// [`Self::set_tdp_class`] (typically from the loaded config).
    pub fn tdp_class(&self) -> clevo_proto::TdpClass {
        self.tdp_class
    }

    /// Override the CPU TDP class used for temperature conversion.
    pub fn set_tdp_class(&mut self, tdp: clevo_proto::TdpClass) {
        self.tdp_class = tdp;
    }

    /// Whether the transport can write.
    pub fn writable(&self) -> bool {
        self.transport.writable()
    }

    /// The transport kind.
    pub fn kind(&self) -> clevo_transport::TransportKind {
        self.transport.kind()
    }

    /// Return the keyboard snapshot when a compatible controller was found.
    pub fn keyboard_snapshot(&self) -> Option<KeyboardSnapshot> {
        self.keyboard.as_ref().map(|keyboard| keyboard.snapshot())
    }

    /// Record the keyboard type reported by the firmware, when available.
    pub fn set_keyboard_firmware_type(&self, keyboard_type: Option<u8>) {
        self.keyboard_firmware_type.store(
            keyboard_type.unwrap_or(UNKNOWN_KEYBOARD_TYPE),
            Ordering::Relaxed,
        );
    }

    /// Return the firmware keyboard type (`6` is RGB15Color).
    pub fn keyboard_firmware_type(&self) -> Option<u8> {
        match self.keyboard_firmware_type.load(Ordering::Relaxed) {
            UNKNOWN_KEYBOARD_TYPE => None,
            value => Some(value),
        }
    }

    fn keyboard(&self) -> Result<&dyn Keyboard, ServiceError> {
        self.keyboard.as_deref().ok_or_else(|| {
            ServiceError::Unsupported("no compatible keyboard RGB controller".into())
        })
    }

    /// Set a keyboard effect mode.
    pub fn set_keyboard_mode(&self, mode: &str) -> Result<(), ServiceError> {
        let mode = KeyboardMode::parse(mode)
            .ok_or_else(|| ServiceError::UnknownMode(format!("keyboard mode {mode:?}")))?;
        let keyboard = self.keyboard()?;
        // Refuse effects this backend cannot drive instead of letting the
        // hardware layer reject them with a less specific message.
        if !keyboard.snapshot().info.modes.contains(&mode) {
            return Err(ServiceError::Unsupported(format!(
                "keyboard effect {mode:?} is not available on this controller"
            )));
        }
        keyboard.set_mode(mode)?;
        self.persist();
        Ok(())
    }

    /// Set keyboard brightness as a percentage in `0..=100`.
    pub fn set_keyboard_brightness(&self, percent: u8) -> Result<u8, ServiceError> {
        self.keyboard()?.set_brightness(percent)?;
        self.persist();
        Ok(percent)
    }

    /// Apply a color to a logical keyboard zone.
    pub fn set_keyboard_zone(&self, zone: &str, color: Color) -> Result<(), ServiceError> {
        let zone = KeyboardZone::parse(zone)
            .ok_or_else(|| ServiceError::Unsupported(format!("unknown keyboard zone {zone:?}")))?;
        self.keyboard()?.set_zone(zone, color)?;
        self.persist();
        Ok(())
    }

    /// Apply a color to one key in the verified 6x20 layout.
    pub fn set_keyboard_key(&self, row: u8, col: u8, color: Color) -> Result<(), ServiceError> {
        self.keyboard()?.set_per_key(row, col, color)?;
        self.persist();
        Ok(())
    }

    /// Poll fan status (command 12) and the curve (command 13, for fan count).
    ///
    /// A failed command 12 poll marks the cached readings stale; a failed curve
    /// read leaves the previous fan count in effect (best-effort).
    pub fn poll_fan(&self) -> Result<(), ServiceError> {
        let _ = self.clock.now_ms();
        let tdp = self.tdp_class();

        match self.read_status() {
            Ok((status, fan_count)) => {
                // The firmware does not report the current mode reliably, but
                // some backends (the kernel driver) can observe it; refresh the
                // cached values so the UI can highlight the active mode.
                let modes = self.transport.current_modes().ok();
                let mut guard = self.state.lock().unwrap();
                guard.apply_status(&status, fan_count, tdp);
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
        let info = self.read_curve_uncached()?;
        self.state.lock().unwrap().curve = Some(info);
        Ok(info)
    }

    /// Read the fan curve without updating the cached "latest EC curve".
    ///
    /// Used by the factory-curve snapshot, which reads the EC before a write:
    /// caching that value would leave the `FanCurve` property showing the
    /// pre-write table, which is exactly the stale read-back the property
    /// deliberately avoids.
    fn read_curve_uncached(&self) -> Result<clevo_proto::FanCurveInfo, ServiceError> {
        let raw = self
            .transport
            .execute(CMD_FAN_CURVE_READ.get(), &empty_payload())?;
        let payload = response_first_record(&raw)?;
        Ok(parse_curve(payload)?)
    }

    /// The curve the firmware shipped with, if it was captured.
    ///
    /// `None` until [`Self::capture_factory_curve`] has run, which is why the
    /// UI must treat a missing default as "unknown" rather than assume one.
    pub fn factory_curve(&self) -> Option<FanCurve> {
        self.baseline
            .lock()
            .unwrap()
            .factory_curve
            .map(config::FanCurveWire::to_curve)
    }

    /// Snapshot the EC's current curve as the factory default, when it is one.
    ///
    /// The firmware has no command that restores the shipped curve, and the EC
    /// forgets whatever was written on power loss (see `docs/hardware-notes.md`
    /// §13.3). So the shipped table is recoverable at every cold boot, from the
    /// moment the daemon starts until its replay writes the saved curve.
    ///
    /// Capturing naively at startup would be wrong on a warm restart: the EC
    /// then still holds the *user's* curve, which must not be recorded as the
    /// default. The two are told apart by comparing what the EC reports against
    /// the saved curve - if they agree on everything command `14` controls, the
    /// EC is holding the daemon's own write and the shipped table is not
    /// available this boot; if they differ, the EC forgot it and what it shows
    /// is the shipped table.
    ///
    /// The comparison is [`FanCurve::same_writable_state`], not equality: a
    /// written curve never reads back identical (T1/T4 stay with the EC, and an
    /// absent channel is left alone), so equality would misread every warm
    /// restart as a cold boot.
    ///
    /// A no-op once captured, and best-effort: a failed read leaves the default
    /// unknown, which the UI reports honestly rather than guessing.
    pub fn capture_factory_curve(&self) {
        // Cheap check first: once captured, later curve writes must not re-read.
        if self.baseline.lock().unwrap().factory_curve.is_some() {
            return;
        }
        let saved = self
            .baseline
            .lock()
            .unwrap()
            .fan_curve
            .map(|wire| wire.to_curve());
        let Ok(info) = self.read_curve_uncached() else {
            return;
        };

        let mut baseline = self.baseline.lock().unwrap();
        // Re-check under the lock: a concurrent call may have captured it.
        if baseline.factory_curve.is_some() {
            return;
        }
        if saved.is_some_and(|saved| saved.same_writable_state(&info.curve)) {
            // The EC holds the curve the daemon saved, so this is not a cold
            // boot and the shipped table is gone.
            return;
        }
        baseline.factory_curve = Some(config::FanCurveWire::from_curve(&info.curve));
        drop(baseline);
        self.persist();
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
        // Capture the shipped curve before the first write overwrites it: after
        // this command the EC no longer holds the factory table, and there is no
        // command that restores it.
        self.capture_factory_curve();
        let payload = encode_curve(curve)?;
        self.transport
            .execute(CMD_FAN_CURVE_WRITE.get(), &payload_from_slice(&payload)?)?;
        // Only select `custom` once the write itself succeeded: it is the mode
        // that makes the firmware use the table just written.
        const CUSTOM: u8 = 6;
        self.apply(SUB_FAN_MODE, CUSTOM)?;
        {
            let mut state = self.state.lock().unwrap();
            state.fan_mode = Some(CUSTOM);
            state.saved_curve = Some(*curve);
        }
        self.last_fan_mode
            .store(u64::from(CUSTOM), Ordering::SeqCst);
        self.persist();
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
        self.persist();
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
        self.persist();
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
        // The loaded file becomes the merge base for later saves, so replaying
        // (and any write that follows) preserves fields we do not manage.
        self.set_baseline(config);

        // Capture the shipped curve before the replay can overwrite it. On a
        // fresh boot the EC holds it; once a curve has been written there is no
        // command that brings it back. This is a no-op once captured, and when a
        // user curve is already stored (whose EC contents are not the factory
        // table). Suppression during replay does not apply here, because
        // `capture_factory_curve` runs before `applying` is set.
        self.capture_factory_curve();

        if !config.apply_on_start {
            return Vec::new();
        }

        // Replaying stored values must not immediately rewrite the file it was
        // read from; persistence is for user actions, not startup.
        self.applying.store(true, Ordering::SeqCst);
        let failures = self.replay_saved(config);
        self.applying.store(false, Ordering::SeqCst);
        failures
    }

    /// The actual replay, with persistence suppressed by the caller.
    fn replay_saved(&self, config: &config::Config) -> Vec<String> {
        let mut failures = Vec::new();

        // A persisted curve is only written when it is meant to be active:
        // `custom` (or no saved mode, where applying the curve implies
        // `custom`). Normal modes must not rewrite the EC curve on startup.
        let mut curve_applied = false;
        let mut curve_invalid = false;
        if self.transport.writable() {
            if let Some(wire) = config.fan_curve {
                let curve = wire.to_curve();
                if let Err(err) = encode_curve(&curve) {
                    curve_invalid = true;
                    failures.push(format!("fan_curve: {err}"));
                } else {
                    self.state.lock().unwrap().saved_curve = Some(curve);
                    let mode_uses_curve = config.fan_mode.is_none() || config.fan_mode == Some(6);
                    if mode_uses_curve {
                        match self.set_curve(&curve) {
                            Ok(()) => curve_applied = true,
                            Err(err) => failures.push(format!("fan_curve: {err}")),
                        }
                    }
                }
            }

            if let Some(value) = config.fan_mode {
                match fan_mode_name(value) {
                    Some(name) => {
                        if value == 6 && config.fan_curve.is_some() {
                            // `set_curve` performs command 14 followed by the
                            // custom-mode write. Never select custom after a
                            // failed or invalid curve restore.
                            if curve_applied {
                                // Already applied by set_curve.
                            } else if curve_invalid {
                                failures.push(
                                    "fan_mode 6: skipped because the saved curve is invalid"
                                        .to_string(),
                                );
                            } else {
                                failures.push(
                                "fan_mode 6: skipped because the saved curve could not be written"
                                    .to_string(),
                            );
                            }
                        } else if let Err(e) = self.set_fan_mode(name) {
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
        }

        if let (Some(keyboard), Some(saved)) = (self.keyboard.as_ref(), config.keyboard.as_ref()) {
            // A config written before the capability was narrowed can name an
            // effect this backend no longer offers (e.g. a `wave` saved when
            // the single-zone EC was wrongly assumed to animate). Fall back to
            // the closest supported mode — static keeps the saved colour — and
            // do not report it as a failure, or every start would log one.
            let restore_mode = |keyboard: &dyn Keyboard| -> Result<(), ServiceError> {
                let Some(mode) = KeyboardMode::parse(&saved.mode) else {
                    return Err(ServiceError::UnknownMode(format!(
                        "keyboard mode {:?}",
                        saved.mode
                    )));
                };
                let supported = keyboard.snapshot().info.modes;
                let mode = if supported.contains(&mode) {
                    mode
                } else {
                    KeyboardMode::Static
                };
                Ok(keyboard.set_mode(mode)?)
            };

            if keyboard.snapshot().info.backend == "acpi-dchu" {
                // This machine's RGB15 path has one physical channel. Restore
                // one representative persisted color; replaying left/middle/
                // right in sequence would make the last color overwrite the
                // entire keyboard. Restore colors and brightness before the
                // mode because applying colors after `wave` can switch the EC
                // back to static mode.
                if let Some(key) = saved.keys.first() {
                    if let Err(err) = keyboard.set_zone(KeyboardZone::All, key.color.into()) {
                        failures.push(format!("keyboard zone All: {err}"));
                    }
                }

                if let Err(err) = keyboard.set_brightness(saved.brightness) {
                    failures.push(format!("keyboard brightness: {err}"));
                }
                if let Err(err) = restore_mode(keyboard.as_ref()) {
                    failures.push(format!("keyboard mode: {err}"));
                }
            } else {
                if let Err(err) = restore_mode(keyboard.as_ref()) {
                    failures.push(format!("keyboard mode: {err}"));
                }
                if let Err(err) = keyboard.set_brightness(saved.brightness) {
                    failures.push(format!("keyboard brightness: {err}"));
                }
                for key in &saved.keys {
                    if let Err(err) = keyboard.set_per_key(key.row, key.col, key.color.into()) {
                        failures.push(format!("keyboard key ({},{}): {err}", key.row, key.col));
                    }
                }
            }
        }
        failures
    }

    /// Build the config representing the current in-memory modes.
    ///
    /// Starts from the loaded baseline so fields this process never manages
    /// (`cpu_tdp_class`, `apply_on_start`) are preserved, then overlays the
    /// live modes. Only fields the daemon actually set are overwritten: a mode
    /// that was never written this session (the `UNSET` sentinel) keeps the
    /// value from the baseline rather than being cleared.
    pub fn to_config(&self) -> config::Config {
        let mut config = self.baseline.lock().unwrap().clone();

        let fan = self.last_fan_mode.load(Ordering::SeqCst);
        if fan != UNSET {
            config.fan_mode = Some(fan as u8);
        }

        let perf = self.last_perf_mode.load(Ordering::SeqCst);
        if perf != UNSET {
            config.perf_mode = Some(perf as u8);
        }

        let curve = self.state.lock().unwrap().saved_curve;
        if curve.is_some() {
            config.fan_curve = curve.as_ref().map(config::FanCurveWire::from_curve);
        }

        if let Some(snapshot) = self.keyboard_snapshot() {
            config.keyboard = Some(config::KeyboardConfig::from_snapshot(&snapshot));
        }

        config
    }

    /// Persist the current configuration, if a path is configured.
    ///
    /// Failures are logged, not propagated: persistence must never turn a
    /// successful hardware write into an error. Called after each successful
    /// write, and suppressed during startup replay so reading the file does not
    /// immediately rewrite it.
    fn persist(&self) {
        if self.applying.load(Ordering::SeqCst) {
            return;
        }
        let Some(path) = self.config_path.as_deref() else {
            return;
        };
        let config = self.to_config();
        if let Err(err) = config::save(path, &config) {
            tracing::warn!(
                path = %path.display(),
                %err,
                "could not persist configuration"
            );
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
