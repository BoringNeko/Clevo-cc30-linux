//! The daemon's cached state model.
//!
//! The daemon periodically polls the hardware and keeps the **last successful**
//! reading for each quantity. Failures do not clear the cache; instead the
//! affected value becomes [`Freshness::Stale`] (we have a value but it is old)
//! or [`Freshness::Unknown`] (we have never read it). Consumers must distinguish
//! these from a fresh reading, so a UI never presents stale data as live.
//!
//! This module is deliberately free of any I/O or D-Bus types so it can be unit
//! tested without a bus or hardware.

use clevo_proto::fan_status::period_raw_to_rpm;
use clevo_proto::{FanCurveInfo, FanStatus};

/// Fan channels the protocol can encode.
pub const MAX_FANS: usize = 3;

/// How trustworthy a cached value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Freshness {
    /// Read successfully during the most recent poll.
    Fresh,
    /// Last read failed; the value shown is from an earlier poll.
    Stale,
    /// Never read successfully.
    #[default]
    Unknown,
}

impl Freshness {
    /// Whether a value is currently present (fresh or stale).
    pub fn has_value(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

/// A cached scalar with its freshness and the monotonic millisecond timestamp
/// of the last successful read (`None` if never read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cached<T> {
    /// The last known value.
    pub value: T,
    /// How trustworthy the value is.
    pub freshness: Freshness,
    /// Monotonic ms at the last successful read.
    pub updated_at_ms: Option<u64>,
}

impl<T: Default> Default for Cached<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            freshness: Freshness::Unknown,
            updated_at_ms: None,
        }
    }
}

impl<T> Cached<T> {
    /// Mark the cached value stale (a later poll failed).
    pub fn mark_stale(&mut self) {
        if self.freshness == Freshness::Fresh {
            self.freshness = Freshness::Stale;
        }
    }

    /// Store a fresh value.
    pub fn set(&mut self, value: T, now_ms: u64) {
        self.value = value;
        self.freshness = Freshness::Fresh;
        self.updated_at_ms = Some(now_ms);
    }
}

/// One fan channel as reported to consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FanReading {
    /// Rotation period raw value from the EC.
    pub period_raw: u16,
    /// Speed in rpm derived via the Control Center formula.
    pub rpm: u32,
    /// Raw duty byte (offset unverified on this firmware).
    pub duty: u8,
    /// Raw temperature byte (conversion unverified).
    pub temp_raw: u8,
    /// Whether this channel exists on this machine.
    pub available: bool,
}

/// Complete cached fan state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FanState {
    /// CPU fan.
    pub cpu: FanReading,
    /// GPU1 fan.
    pub gpu1: FanReading,
    /// GPU2 fan (usually absent).
    pub gpu2: FanReading,
    /// Freshness shared by the reading above.
    pub freshness: Freshness,
}

impl FanState {
    /// Build from a parsed status package and the known fan count.
    ///
    /// A `fan_count` of `0` means "unknown", in which case every channel is
    /// treated as present so raw data is never hidden.
    pub fn from_status(status: &FanStatus, fan_count: u8) -> Self {
        let present = |index: usize| fan_count == 0 || (index as u8) <= fan_count;
        let read = |period: u16, duty: u8, temp: u8, index: usize| FanReading {
            period_raw: period,
            rpm: period_raw_to_rpm(period),
            duty,
            temp_raw: temp,
            available: present(index),
        };
        Self {
            cpu: read(status.cpu_rpm, status.cpu_duty, status.cpu_temp_raw, 1),
            gpu1: read(status.gpu1_rpm, status.gpu1_duty, status.gpu1_temp_raw, 2),
            gpu2: read(status.gpu2_rpm, status.gpu2_duty, status.gpu2_temp_raw, 3),
            freshness: Freshness::Fresh,
        }
    }

    /// Iterate over `(name, reading)` in channel order.
    pub fn readings(&self) -> [(&'static str, &FanReading); MAX_FANS] {
        [
            ("CPU", &self.cpu),
            ("GPU1", &self.gpu1),
            ("GPU2", &self.gpu2),
        ]
    }
}

/// The daemon's full observable state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonState {
    /// Cached fan state.
    pub fan: FanState,
    /// Cached fan curve, if it has ever been read.
    pub curve: Option<FanCurveInfo>,
    /// Last fan mode written by the daemon (`121/1`).
    pub fan_mode: Option<u8>,
    /// Last performance mode written by the daemon (`121/25`).
    pub perf_mode: Option<u8>,
}

impl DaemonState {
    /// Record a successful fan poll.
    pub fn apply_status(&mut self, status: &FanStatus, fan_count: u8) {
        self.fan = FanState::from_status(status, fan_count);
    }

    /// Mark fan readings stale after a failed poll.
    pub fn mark_fan_stale(&mut self) {
        self.fan.freshness = if self.fan.freshness == Freshness::Fresh {
            Freshness::Stale
        } else {
            Freshness::Unknown
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clevo_proto::FanStatus;

    fn status() -> FanStatus {
        FanStatus {
            cpu_rpm: 452,
            gpu1_rpm: 0,
            gpu2_rpm: 0,
            cpu_duty: 63,
            cpu_temp_raw: 37,
            gpu1_duty: 0,
            gpu1_temp_raw: 33,
            gpu2_duty: 0,
            gpu2_temp_raw: 0,
        }
    }

    #[test]
    fn fresh_after_success() {
        let mut st = DaemonState::default();
        assert_eq!(st.fan.freshness, Freshness::Unknown);
        st.apply_status(&status(), 2);
        assert_eq!(st.fan.freshness, Freshness::Fresh);
        assert_eq!(st.fan.cpu.rpm, 4770);
        assert!(!st.fan.gpu2.available);
    }

    #[test]
    fn failure_marks_stale_but_keeps_value() {
        let mut st = DaemonState::default();
        st.apply_status(&status(), 2);
        st.mark_fan_stale();
        assert_eq!(st.fan.freshness, Freshness::Stale);
        assert_eq!(st.fan.cpu.rpm, 4770);
    }

    #[test]
    fn failure_before_any_success_stays_unknown() {
        let mut st = DaemonState::default();
        st.mark_fan_stale();
        assert_eq!(st.fan.freshness, Freshness::Unknown);
    }
}
