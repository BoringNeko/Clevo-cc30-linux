//! Probe the real PolicyKit authority using the production authorizer.
//!
//! Usage: `polkit_probe [pid]` (defaults to the current process).
//!
//! Prints the resolved subject and the allow/deny decision for both Clevo
//! actions. Run it as the user you want to check; it reads that process's
//! start time from `/proc` just like the daemon does.

use clevod::policy::{
    process_start_time, Authorizer, PolicyKitAuthorizer, Subject, ACTION_FAN_MODE, ACTION_PERF_MODE,
};

#[tokio::main]
async fn main() {
    let pid: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(std::process::id);

    let uid = current_uid();
    let start_time = process_start_time(pid).unwrap_or(0);
    let subject = Subject {
        pid,
        start_time,
        uid,
        session_id: None,
    };

    println!("subject: pid={pid} start_time={start_time} uid={uid}");

    let authorizer = PolicyKitAuthorizer::default();
    for action in [ACTION_FAN_MODE, ACTION_PERF_MODE] {
        let allowed = authorizer.authorized(action, &subject).await;
        println!("{action}: {}", if allowed { "ALLOWED" } else { "DENIED" });
    }
}

fn current_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}
