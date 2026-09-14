//! The `org.clevo.CC` D-Bus interface.
//!
//! This is a thin, async wrapper over [`crate::service::Service`]. It performs
//! no policy of its own beyond the PolicyKit gate: the [`Service`] validates and
//! gates every write, so the bus layer stays boring and auditable.
//!
//! Interface layout:
//!
//! ```text
//! /org/clevo/CC
//!   properties: FanMode, PerfMode, Writable, FanFreshness, CpuRpm, GpuRpm,
//!               CpuTempRaw, GpuTempRaw, CpuDuty, GpuDuty, FanCount
//!   methods:    SetFanMode(s) -> u8     (PolicyKit)
//!               SetPerfMode(s) -> u8    (PolicyKit)
//!               GetCurve() -> s
//!               Poll()
//!   signals:    FanChanged(u, u)  Error(s)
//! ```
//!
//! The `interface` macro generates helper methods whose documentation the lint
//! cannot see, so `missing_docs` is relaxed for this module.
#![allow(missing_docs)]

use std::sync::Arc;

use zbus::interface;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use crate::policy::{self, Authorizer, PolicyKitAuthorizer};
use crate::service::Service;

/// D-Bus object implementation.
pub struct CcDaemon {
    service: Arc<Service>,
    authorizer: Arc<dyn Authorizer>,
}

impl CcDaemon {
    /// Wrap a service with the default PolicyKit authorizer.
    pub fn new(service: Arc<Service>) -> Self {
        Self::with_authorizer(service, Arc::new(PolicyKitAuthorizer::default()))
    }

    /// Wrap a service with an explicit authorizer (used by tests).
    pub fn with_authorizer(service: Arc<Service>, authorizer: Arc<dyn Authorizer>) -> Self {
        Self {
            service,
            authorizer,
        }
    }

    /// Access the wrapped service (used by tests).
    pub fn service(&self) -> &Arc<Service> {
        &self.service
    }

    /// Emit `FanChanged` from the current cached rpm.
    async fn emit_fan_changed(&self, emitter: &SignalEmitter<'_>) {
        let (cpu, gpu) = {
            let state = self.service.state();
            let state = state.lock().unwrap();
            (state.fan.cpu.rpm, state.fan.gpu1.rpm)
        };
        let _ = CcDaemon::fan_changed(emitter, cpu, gpu).await;
    }

    /// Enforce PolicyKit for a write action performed by the message sender.
    ///
    /// The sender's uid and pid are resolved through the bus daemon
    /// (`GetConnectionUnixUser` / `GetConnectionUnixProcessID`); the pid's start
    /// time is read from `/proc` so the `unix-process` subject is well formed.
    /// A missing sender (peer-to-peer connections, direct tests) is treated as
    /// root, so a root-only authorizer still exercises the interesting paths.
    async fn require(
        &self,
        sender: Option<&zbus::names::UniqueName<'_>>,
        emitter: &SignalEmitter<'_>,
        action: &str,
    ) -> zbus::fdo::Result<()> {
        let subject = match sender {
            Some(name) => {
                let dbus = zbus::fdo::DBusProxy::new(emitter.connection()).await?;
                let bus_name = zbus::names::BusName::from(name.clone());
                let uid = dbus.get_connection_unix_user(bus_name.clone()).await?;
                let pid = dbus.get_connection_unix_process_id(bus_name).await?;
                let start_time = crate::policy::process_start_time(pid).unwrap_or(0);
                let session_id =
                    crate::policy::session_for_pid(emitter.connection(), pid, uid).await;
                policy::Subject {
                    pid,
                    start_time,
                    uid,
                    session_id,
                }
            }
            None => policy::Subject::root(),
        };

        tracing::info!(
            action,
            pid = subject.pid,
            start_time = subject.start_time,
            session = ?subject.session_id,
            uid = subject.uid,
            "policy check"
        );

        if self.authorizer.authorized(action, &subject).await {
            Ok(())
        } else {
            let message = format!(
                "not authorized for {action} (pid {}, uid {})",
                subject.pid, subject.uid
            );
            let _ = CcDaemon::error(emitter, &message).await;
            Err(zbus::fdo::Error::AccessDenied(message))
        }
    }
}

/// Serve `daemon` at [`crate::DBUS_PATH`] on `connection`.
///
/// Split out from the binary so tests can host the object on a private bus.
pub async fn serve(connection: &zbus::Connection, daemon: CcDaemon) -> zbus::Result<()> {
    connection
        .object_server()
        .at(crate::DBUS_PATH, daemon)
        .await
        .map(|_| ())
}

#[interface(name = "org.clevo.CC")]
impl CcDaemon {
    /// Emitted after a successful poll with the current CPU and GPU1 rpm.
    #[zbus(signal)]
    async fn fan_changed(
        emitter: &SignalEmitter<'_>,
        cpu_rpm: u32,
        gpu_rpm: u32,
    ) -> zbus::Result<()>;

    /// Emitted when the daemon fails to read the hardware.
    #[zbus(signal)]
    async fn error(emitter: &SignalEmitter<'_>, message: &str) -> zbus::Result<()>;

    /// Set the fan mode; returns the numeric `121/1` value applied.
    async fn set_fan_mode(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_context)] emitter: SignalEmitter<'_>,
        mode: &str,
    ) -> zbus::fdo::Result<u8> {
        self.require(header.sender(), &emitter, policy::ACTION_FAN_MODE)
            .await?;
        match self.service.set_fan_mode(mode) {
            Ok(value) => {
                self.emit_fan_changed(&emitter).await;
                Ok(value)
            }
            Err(e) => {
                let message = e.to_string();
                let _ = CcDaemon::error(&emitter, &message).await;
                Err(zbus::fdo::Error::Failed(message))
            }
        }
    }

    /// Set the performance mode; returns the numeric `121/25` value applied.
    async fn set_perf_mode(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_context)] emitter: SignalEmitter<'_>,
        mode: &str,
    ) -> zbus::fdo::Result<u8> {
        self.require(header.sender(), &emitter, policy::ACTION_PERF_MODE)
            .await?;
        self.service
            .set_perf_mode(mode)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Refresh the cached fan state from the hardware.
    async fn poll(
        &self,
        #[zbus(signal_context)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        match self.service.poll_fan() {
            Ok(()) => {
                self.emit_fan_changed(&emitter).await;
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                let _ = CcDaemon::error(&emitter, &message).await;
                Err(zbus::fdo::Error::Failed(message))
            }
        }
    }

    /// Read the fan curve and return it as a compact JSON string.
    async fn get_curve(&self) -> zbus::fdo::Result<String> {
        let info = self
            .service
            .read_curve()
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(curve_to_json(&info))
    }

    /// Current fan mode value (`121/1`), or `255` if never set this session.
    #[zbus(property)]
    fn fan_mode(&self) -> u8 {
        self.service.state().lock().unwrap().fan_mode.unwrap_or(255)
    }

    /// Current performance mode value (`121/25`), or `255` if never set.
    #[zbus(property)]
    fn perf_mode(&self) -> u8 {
        self.service
            .state()
            .lock()
            .unwrap()
            .perf_mode
            .unwrap_or(255)
    }

    /// Whether the daemon can perform hardware writes.
    #[zbus(property)]
    fn writable(&self) -> bool {
        self.service.writable()
    }

    /// Freshness of the cached fan readings: `fresh`, `stale` or `unknown`.
    #[zbus(property)]
    fn fan_freshness(&self) -> String {
        freshness_str(self.service.state().lock().unwrap().fan.freshness)
    }

    /// CPU fan speed in rpm.
    #[zbus(property)]
    fn cpu_rpm(&self) -> u32 {
        self.service.state().lock().unwrap().fan.cpu.rpm
    }

    /// GPU1 fan speed in rpm.
    #[zbus(property)]
    fn gpu_rpm(&self) -> u32 {
        self.service.state().lock().unwrap().fan.gpu1.rpm
    }

    /// CPU temperature raw byte (conversion unverified).
    #[zbus(property)]
    fn cpu_temp_raw(&self) -> u8 {
        self.service.state().lock().unwrap().fan.cpu.temp_raw
    }

    /// GPU1 temperature raw byte (conversion unverified).
    #[zbus(property)]
    fn gpu_temp_raw(&self) -> u8 {
        self.service.state().lock().unwrap().fan.gpu1.temp_raw
    }

    /// CPU duty raw byte (offset unverified).
    #[zbus(property)]
    fn cpu_duty(&self) -> u8 {
        self.service.state().lock().unwrap().fan.cpu.duty
    }

    /// GPU1 duty raw byte (offset unverified).
    #[zbus(property)]
    fn gpu_duty(&self) -> u8 {
        self.service.state().lock().unwrap().fan.gpu1.duty
    }

    /// Number of fans reported by the firmware (`0` = unknown).
    #[zbus(property)]
    fn fan_count(&self) -> u8 {
        self.service
            .state()
            .lock()
            .unwrap()
            .curve
            .map(|c| c.fan_count)
            .unwrap_or(0)
    }
}

fn freshness_str(freshness: crate::state::Freshness) -> String {
    use crate::state::Freshness;
    match freshness {
        Freshness::Fresh => "fresh",
        Freshness::Stale => "stale",
        Freshness::Unknown => "unknown",
    }
    .to_string()
}

/// Render a [`clevo_proto::FanCurveInfo`] as JSON.
pub fn curve_to_json(info: &clevo_proto::FanCurveInfo) -> String {
    let points = |curve: &[clevo_proto::FanPoint; 4]| {
        curve
            .iter()
            .map(|p| format!("[{},{}]", p.temp, p.duty_pct))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{{\"fan_count\":{},\"init_mode\":{},\"kb_type\":{},\"cpu\":[{}],\"gpu1\":[{}],\"gpu2\":[{}]}}",
        info.fan_count,
        info.init_mode,
        info.kb_type,
        points(&info.curve.cpu),
        points(&info.curve.gpu1),
        points(&info.curve.gpu2),
    )
}
