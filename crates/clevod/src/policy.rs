//! PolicyKit authorization for privileged write methods.
//!
//! Reads are open to any local user (monitoring is harmless). Writes
//! (`SetFanMode`, `SetPerfMode`) require the caller to be authorized for a
//! PolicyKit action, matching the design decision that fan/performance control
//! is an administrator operation.
//!
//! The authority is queried over the system bus with
//! `org.freedesktop.PolicyKit1.Authority.CheckAuthorization`, passing a
//! `unix-process` subject. PolicyKit requires the **process start time** in
//! addition to the pid (see `pkcheck --help`: `PID[,START_TIME[,UID]]`); a
//! subject of `{pid, start-time: 0}` is rejected by polkit 127. The start time
//! is the 22nd field of `/proc/<pid>/stat` and must be read at the same time as
//! the pid so it describes the same process instance.
//!
//! When PolicyKit is not running (e.g. a bare container) the daemon falls back
//! to a configurable default via [`decide`]; the default for the real daemon is
//! to allow only `root` (checked by uid).
//!
//! The pure decision logic is separated from the bus call so it can be unit
//! tested: [`decide`] maps a [`PolicyOutcome`] to allow/deny.

use std::collections::HashMap;

/// PolicyKit action for fan-mode changes.
pub const ACTION_FAN_MODE: &str = "org.clevo.CC.set-fan-mode";
/// PolicyKit action for performance-mode changes.
pub const ACTION_PERF_MODE: &str = "org.clevo.CC.set-perf-mode";

/// Identity of the process requesting a write.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Subject {
    /// Process id of the caller.
    pub pid: u32,
    /// Process start time (22nd field of `/proc/<pid>/stat`); `0` when unknown.
    pub start_time: u64,
    /// Unix user id of the caller.
    pub uid: u32,
    /// Login session id the caller belongs to, when known.
    ///
    /// A `unix-session` subject lets PolicyKit scope `auth_admin_keep` to the
    /// session, so a user is prompted once and later writes in the same session
    /// are not prompted again. With only a `unix-process` subject every call is
    /// a new subject and the authorization cannot be reused.
    pub session_id: Option<String>,
}

impl Subject {
    /// The kernel's own identity, used for peer-to-peer connections where there
    /// is no bus sender.
    pub fn root() -> Self {
        Self {
            pid: std::process::id(),
            start_time: 0,
            uid: 0,
            session_id: None,
        }
    }
}

/// The result of a PolicyKit check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyOutcome {
    /// PolicyKit explicitly authorized the subject.
    Authorized,
    /// PolicyKit explicitly denied the subject.
    Denied,
    /// PolicyKit is not available; the caller must apply its fallback policy.
    Unavailable(String),
}

/// Map a PolicyKit outcome to a boolean, applying `fallback` when the authority
/// is unavailable or the check could not be performed.
pub fn decide(outcome: &PolicyOutcome, fallback: bool) -> bool {
    match outcome {
        PolicyOutcome::Authorized => true,
        PolicyOutcome::Denied => false,
        PolicyOutcome::Unavailable(_) => fallback,
    }
}

/// A source of authorization decisions.
///
/// Async because the real implementation must `await` the PolicyKit call on the
/// same runtime as the D-Bus service; blocking would deadlock a worker thread.
#[async_trait::async_trait]
pub trait Authorizer: Send + Sync {
    /// Whether `subject` may perform `action`.
    async fn authorized(&self, action: &str, subject: &Subject) -> bool;
}

/// Always denies writes. Safe default for tests and read-only transports.
pub struct DenyAll;

#[async_trait::async_trait]
impl Authorizer for DenyAll {
    async fn authorized(&self, _action: &str, _subject: &Subject) -> bool {
        false
    }
}

/// Allows every write. For offline tests only.
pub struct AllowAll;

#[async_trait::async_trait]
impl Authorizer for AllowAll {
    async fn authorized(&self, _action: &str, _subject: &Subject) -> bool {
        true
    }
}

/// Calls PolicyKit, falling back to "only root" when it is unavailable.
pub struct PolicyKitAuthorizer {
    /// Results cached per `(action, subject)` for the process lifetime so a
    /// single authorized write does not prompt twice. Writes are rare.
    cache: std::sync::Mutex<HashMap<(String, Subject), bool>>,
    /// Whether to call the real authority; tests set this off.
    query: bool,
    /// Whether to allow interactive authentication.
    allow_interaction: bool,
    /// How long to wait for an interactive authorization before giving up.
    interaction_timeout: std::time::Duration,
}

impl Default for PolicyKitAuthorizer {
    fn default() -> Self {
        Self {
            cache: std::sync::Mutex::new(HashMap::new()),
            query: true,
            // Interactive authentication is enabled so a desktop agent can
            // prompt the user. Without an agent, polkit blocks waiting for one;
            // the timeout below turns that into a denial instead of a hung call.
            //
            // (An earlier observation of `authorized=true` with no agent was a
            // stale temporary authorization, not a consequence of this flag:
            // verified by revoking temporary authorizations, after which flag 1
            // blocks/fails and flag 0 denies cleanly.)
            allow_interaction: true,
            interaction_timeout: std::time::Duration::from_secs(60),
        }
    }
}

impl PolicyKitAuthorizer {
    /// An authorizer that never calls the bus and uses the "root only" fallback.
    pub fn without_bus() -> Self {
        Self {
            cache: std::sync::Mutex::new(HashMap::new()),
            query: false,
            allow_interaction: false,
            interaction_timeout: std::time::Duration::from_secs(0),
        }
    }

    /// The fallback when PolicyKit is unreachable: only uid 0 (root) is allowed.
    pub fn fallback_allow(uid: u32) -> bool {
        uid == 0
    }
}

#[async_trait::async_trait]
impl Authorizer for PolicyKitAuthorizer {
    async fn authorized(&self, action: &str, subject: &Subject) -> bool {
        let key = (action.to_string(), subject.clone());
        if let Some(hit) = self.cache.lock().unwrap().get(&key) {
            return *hit;
        }

        let outcome = if self.query {
            let call = check_authorization(action, subject, self.allow_interaction);
            // An interactive check can block while polkit waits for an
            // authentication agent. A timeout keeps the daemon responsive when
            // no agent is present (headless, no desktop session).
            match tokio::time::timeout(self.interaction_timeout, call).await {
                Ok(outcome) => outcome,
                Err(_) => PolicyOutcome::Unavailable(format!(
                    "authorization timed out after {:?} (no authentication agent?)",
                    self.interaction_timeout
                )),
            }
        } else {
            PolicyOutcome::Unavailable("bus queries disabled".into())
        };
        let allowed = decide(&outcome, Self::fallback_allow(subject.uid));
        tracing::debug!(
            action,
            uid = subject.uid,
            ?outcome,
            allowed,
            "policykit outcome"
        );
        self.cache.lock().unwrap().insert(key, allowed);
        allowed
    }
}

/// Perform the PolicyKit `CheckAuthorization` call for a subject.
async fn check_authorization(
    action: &str,
    subject: &Subject,
    allow_interaction: bool,
) -> PolicyOutcome {
    use zbus::zvariant::Value;

    let connection = match zbus::Connection::system().await {
        Ok(c) => c,
        Err(e) => return PolicyOutcome::Unavailable(e.to_string()),
    };

    // Subject is the struct `(sa{sv})`: a type name plus a property map. It must
    // be passed as a tuple; a bare map would be serialized as `a{sv}` and
    // rejected with InvalidArgs.
    //
    // Use a `unix-process` subject (pid + start time). It carries the caller's
    // concrete identity, which is what `pkcheck -p` and the common policy rules
    // match on. A `unix-session` subject was tried so `auth_admin_keep` could be
    // cached per session, but rules that match `subject.user` do not fire for it
    // in practice, so writes were denied even with a matching allow rule.
    let mut subject_props: HashMap<&str, Value<'_>> = HashMap::new();
    subject_props.insert("pid", Value::from(subject.pid));
    subject_props.insert("start-time", Value::from(subject.start_time));
    let subject_arg = ("unix-process", subject_props);
    let _ = &subject.session_id; // session is kept for logging only

    // Details are `a{ss}` (string to string), not `a{sv}`.
    let details: HashMap<&str, &str> = HashMap::new();
    // Flag 1 = AllowUserInteraction.
    let flags: u32 = u32::from(allow_interaction);

    let proxy = match zbus::Proxy::new(
        &connection,
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        "org.freedesktop.PolicyKit1.Authority",
    )
    .await
    {
        Ok(p) => p,
        Err(e) => return PolicyOutcome::Unavailable(e.to_string()),
    };

    let reply = proxy
        .call_method(
            "CheckAuthorization",
            &(subject_arg, action, details, flags, ""),
        )
        .await;

    match reply {
        Ok(msg) => {
            let body = msg.body();
            // Reply is `(bba{ss})`: authorized, challenge, details.
            let value: Result<(bool, bool, HashMap<String, String>), _> = body.deserialize();
            match value {
                Ok((authorized, _, _)) if authorized => PolicyOutcome::Authorized,
                Ok(_) => PolicyOutcome::Denied,
                Err(e) => PolicyOutcome::Unavailable(e.to_string()),
            }
        }
        Err(e) => PolicyOutcome::Unavailable(e.to_string()),
    }
}

/// Resolve a pid to its login session id via logind.
///
/// Tries `GetSessionByPID` first, then falls back to listing sessions and
/// picking the user's active one. The fallback matters because the caller (e.g.
/// `busctl` or a short-lived GUI helper) may have exited by the time logind is
/// queried, in which case the pid lookup returns nothing but the user still has
/// an active session whose authorization should be reused.
///
/// Returns `None` if logind is unavailable or the user has no session; the
/// caller then falls back to a `unix-process` subject.
pub async fn session_for_pid(connection: &zbus::Connection, pid: u32, uid: u32) -> Option<String> {
    let manager = zbus::Proxy::new(
        connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .ok()?;

    // Preferred: the exact session for the pid.
    if let Ok(reply) = manager.call_method("GetSessionByPID", &(pid,)).await {
        if let Ok(path) = reply
            .body()
            .deserialize::<zbus::zvariant::OwnedObjectPath>()
        {
            if let Some(id) = session_id_for_path(connection, &path).await {
                return Some(id);
            }
        }
    }

    // Fallback: the user's active session (the caller may already be gone).
    let reply = manager.call_method("ListSessions", &()).await.ok()?;
    let sessions: Vec<(String, u32, String, String, zbus::zvariant::OwnedObjectPath)> =
        reply.body().deserialize().ok()?;
    for (id, session_uid, _user, _seat, path) in sessions {
        if session_uid != uid {
            continue;
        }
        // Prefer an active session, but accept the first match otherwise.
        if session_is_active(connection, &path).await {
            return Some(id);
        }
    }
    None
}

/// Read the `Id` property of a session object.
async fn session_id_for_path(
    connection: &zbus::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> Option<String> {
    let proxy = zbus::Proxy::new(
        connection,
        "org.freedesktop.login1",
        path.as_str(),
        "org.freedesktop.login1.Session",
    )
    .await
    .ok()?;
    proxy.get_property::<String>("Id").await.ok()
}

/// Whether a session object reports `Active == true`.
async fn session_is_active(
    connection: &zbus::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> bool {
    let proxy = match zbus::Proxy::new(
        connection,
        "org.freedesktop.login1",
        path.as_str(),
        "org.freedesktop.login1.Session",
    )
    .await
    {
        Ok(p) => p,
        Err(_) => return false,
    };
    proxy.get_property::<bool>("Active").await.unwrap_or(false)
}

/// Resolve a pid to its start time (22nd field of `/proc/<pid>/stat`).
///
/// Returns `None` if the process is gone or `/proc` is unavailable.
pub fn process_start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The comm field is wrapped in parentheses and may contain spaces, so parse
    // from the last ')' onwards; the start time is the 20th field after it.
    let after_comm = &stat[stat.rfind(')')? + 1..];
    let mut fields = after_comm.split_whitespace();
    // Fields after comm: state(1) ... starttime(20).
    let start_time = fields.nth(19)?;
    start_time.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(uid: u32) -> Subject {
        Subject {
            pid: 1,
            start_time: 1,
            uid,
            session_id: None,
        }
    }

    #[test]
    fn decide_maps_outcomes() {
        assert!(decide(&PolicyOutcome::Authorized, false));
        assert!(!decide(&PolicyOutcome::Denied, true));
        assert!(decide(&PolicyOutcome::Unavailable("x".into()), true));
        assert!(!decide(&PolicyOutcome::Unavailable("x".into()), false));
    }

    #[tokio::test]
    async fn deny_all_denies_everyone() {
        assert!(!DenyAll.authorized(ACTION_FAN_MODE, &subject(0)).await);
        assert!(!DenyAll.authorized(ACTION_PERF_MODE, &subject(1000)).await);
    }

    #[test]
    fn fallback_is_root_only() {
        assert!(PolicyKitAuthorizer::fallback_allow(0));
        assert!(!PolicyKitAuthorizer::fallback_allow(1000));
    }

    #[tokio::test]
    async fn without_bus_uses_root_fallback() {
        let authorizer = PolicyKitAuthorizer::without_bus();
        assert!(authorizer.authorized(ACTION_FAN_MODE, &subject(0)).await);
        assert_eq!(
            authorizer.authorized(ACTION_FAN_MODE, &subject(4242)).await,
            PolicyKitAuthorizer::fallback_allow(4242)
        );
    }

    #[test]
    fn start_time_of_current_process_is_known() {
        let start = process_start_time(std::process::id()).expect("own start time");
        assert!(start > 0, "start time should be a positive jiffy count");
    }

    #[test]
    fn start_time_of_missing_process_is_none() {
        assert!(process_start_time(u32::MAX).is_none());
    }
}
