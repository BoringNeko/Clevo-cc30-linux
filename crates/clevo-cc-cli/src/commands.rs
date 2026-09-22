//! Command implementations. All output goes through `writeln!` so it is easy to
//! test and so the CLI stays free of logging/formatting dependencies.

use std::io::{self, Write};

use clevo_proto::capability::{parse_capabilities, Page7Version};
use clevo_proto::command::{
    CMD_FAN_CURVE_READ, CMD_FAN_CURVE_WRITE, CMD_FAN_STATUS, CMD_MAIN, SUB_FAN_MODE,
    SUB_POWER_MODE,
};
use clevo_proto::constants::PAYLOAD_LEN;
use clevo_proto::fan_curve::{encode_curve, parse_curve, FanCurve, FanPoint};
use clevo_proto::fan_status::parse_fan_status;
use clevo_proto::message::{build_subcommand_payload, empty_payload, payload_from_slice};
use clevo_proto::response::response_first_record;
use clevo_transport::Transport;

use crate::snapshot::{format_json, format_row, FanSnapshot};
use crate::{CliError, FanCommand, ProfileCommand};

/// Fan mode `121/1` value for automatic control.
pub const FAN_MODE_AUTO: u8 = 0;
/// Fan mode `121/1` value for quiet / slow operation.
pub const FAN_MODE_QUIET: u8 = 8;
/// Fan mode `121/1` value selecting the curve written by command `14`.
pub const FAN_MODE_CUSTOM: u8 = 6;

/// Read the fan status package (command `12`).
pub fn read_fan_status(transport: &dyn Transport) -> Result<clevo_proto::FanStatus, CliError> {
    let raw = transport.execute(CMD_FAN_STATUS.get(), &empty_payload())?;
    let payload = response_first_record(&raw)?;
    Ok(parse_fan_status(payload)?)
}

/// Read the fan curve package (command `13`).
pub fn read_fan_curve(transport: &dyn Transport) -> Result<clevo_proto::FanCurveInfo, CliError> {
    let raw = transport.execute(CMD_FAN_CURVE_READ.get(), &empty_payload())?;
    let payload = response_first_record(&raw)?;
    Ok(parse_curve(payload)?)
}

/// Read a full fan snapshot, using command `13` only to learn the fan count.
///
/// On the mock transport `read_app_settings`/command `12` may be the only
/// supported calls; if the curve read fails the fan count is treated as
/// unknown (`0`) rather than failing the whole snapshot.
pub fn read_fan_snapshot(transport: &dyn Transport) -> Result<FanSnapshot, CliError> {
    let status = read_fan_status(transport)?;
    let fan_count = read_fan_curve(transport)
        .map(|info| info.fan_count)
        .unwrap_or(0);
    Ok(FanSnapshot::from_status(&status, fan_count))
}

/// Read the capability bitmap (`page 7`) and parse it.
pub fn read_capabilities(transport: &dyn Transport) -> Result<clevo_proto::Capabilities, CliError> {
    let page = transport.read_app_settings(7, 0, PAYLOAD_LEN as u16)?;
    Ok(parse_capabilities(&page)?)
}

/// Dispatch a fan sub-command.
pub fn run_fan(
    transport: &dyn Transport,
    command: &FanCommand,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    match command {
        FanCommand::Status => {
            let snapshot = read_fan_snapshot(transport)?;
            for (name, reading) in snapshot.readings() {
                if reading.available {
                    let temp = match reading.temp_c {
                        Some(t) => format!("{t}C"),
                        None => "n/a".to_string(),
                    };
                    writeln!(
                        out,
                        "{name:<4} rpm={:<5} period_raw={:<5} duty={:<4}% temp={}",
                        reading.rpm, reading.period_raw, reading.duty_pct, temp
                    )?;
                } else {
                    writeln!(out, "{name:<4} n/a (channel not present)")?;
                }
            }
            Ok(())
        }
        FanCommand::Curve => {
            let info = read_fan_curve(transport)?;
            writeln!(out, "fan count : {}", info.fan_count)?;
            writeln!(out, "init mode : {}", info.init_mode)?;
            writeln!(out, "kb type   : {}", info.kb_type)?;
            for (name, points) in [
                ("cpu", &info.curve.cpu),
                ("gpu1", &info.curve.gpu1),
                ("gpu2", &info.curve.gpu2),
            ] {
                write!(out, "{name:<4}:")?;
                for point in points {
                    write!(out, " ({}C,{}%)", point.temp, point.duty_pct)?;
                }
                writeln!(out)?;
            }
            Ok(())
        }
        FanCommand::SetMode { mode, apply } => {
            let value = match mode.as_str() {
                "auto" => FAN_MODE_AUTO,
                "quiet" => FAN_MODE_QUIET,
                "maxq" => 5,
                "max" => 1,
                "custom" => FAN_MODE_CUSTOM,
                other => {
                    return Err(CliError::Invalid(format!(
                        "unknown fan mode {other:?}; expected auto, quiet, maxq, max or custom"
                    )))
                }
            };
            set_axis(transport, SUB_FAN_MODE, value, *apply, out, "fan mode")
        }
        FanCommand::SetCurve {
            cpu,
            gpu1,
            gpu2,
            apply,
        } => run_set_curve(transport, cpu, gpu1.as_deref(), gpu2.as_deref(), *apply, out),
        FanCommand::Watch {
            interval_ms,
            count,
            json,
        } => run_fan_watch(transport, *interval_ms, *count, *json, out),
    }
}

/// Parse a `temp,duty` point list into a four-point curve.
///
/// Exactly four pairs are required; temperatures must strictly increase and
/// duty must be `0..=100`. Validation happens here as well as in
/// `clevo_proto::fan_curve::encode_curve` so the CLI can print a precise
/// message about which point was wrong.
pub fn parse_curve_arg(text: &str) -> Result<[FanPoint; 4], CliError> {
    let mut points = [FanPoint {
        temp: 0,
        duty_pct: 0,
    }; 4];
    let entries: Vec<&str> = text.split_whitespace().collect();
    if entries.len() != 4 {
        return Err(CliError::Invalid(format!(
            "expected 4 `temp,duty` points, got {} in {text:?}",
            entries.len()
        )));
    }
    for (i, entry) in entries.iter().enumerate() {
        let (temp, duty) = entry.split_once(',').ok_or_else(|| {
            CliError::Invalid(format!("point {i} {entry:?} is not `temp,duty`"))
        })?;
        let temp: u8 = temp
            .trim()
            .parse()
            .map_err(|e| CliError::Invalid(format!("point {i} temperature {temp:?}: {e}")))?;
        let duty: u8 = duty
            .trim()
            .parse()
            .map_err(|e| CliError::Invalid(format!("point {i} duty {duty:?}: {e}")))?;
        if duty > 100 {
            return Err(CliError::Invalid(format!(
                "point {i} duty {duty}% exceeds 100%"
            )));
        }
        points[i] = FanPoint {
            temp,
            duty_pct: duty,
        };
    }
    for i in 0..3 {
        if points[i + 1].temp <= points[i].temp {
            return Err(CliError::Invalid(format!(
                "temperatures must strictly increase: T{}={} >= T{}={}",
                i + 1,
                points[i].temp,
                i + 2,
                points[i + 1].temp
            )));
        }
    }
    Ok(points)
}

/// Build a [`FanCurve`] from the three per-fan point lists.
pub fn curve_from_args(
    cpu: &str,
    gpu1: Option<&str>,
    gpu2: Option<&str>,
) -> Result<FanCurve, CliError> {
    let cpu = parse_curve_arg(cpu)?;
    // Most machines share the CPU and GPU1 curve shape; defaulting to the CPU
    // curve matches what the Control Center does when a user only edits one.
    let gpu1 = match gpu1 {
        Some(text) => parse_curve_arg(text)?,
        None => cpu,
    };
    let gpu2 = match gpu2 {
        Some(text) => parse_curve_arg(text)?,
        None => [FanPoint {
            temp: 0,
            duty_pct: 0,
        }; 4],
    };
    Ok(FanCurve { cpu, gpu1, gpu2 })
}

/// Encode a curve to the daemon's JSON wire format.
pub fn curve_to_json(curve: &FanCurve) -> String {
    let points = |points: &[FanPoint; 4]| {
        points
            .iter()
            .map(|p| format!("[{},{}]", p.temp, p.duty_pct))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{{\"cpu\":[{}],\"gpu1\":[{}],\"gpu2\":[{}]}}",
        points(&curve.cpu),
        points(&curve.gpu1),
        points(&curve.gpu2)
    )
}

/// Write a custom fan curve, or print what would be written.
fn run_set_curve(
    transport: &dyn Transport,
    cpu: &str,
    gpu1: Option<&str>,
    gpu2: Option<&str>,
    apply: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let curve = curve_from_args(cpu, gpu1, gpu2)?;
    let payload = encode_curve(&curve)?;

    if !apply {
        writeln!(out, "dry run: would write fan curve (command 14) and set custom mode")?;
        for (name, points) in [
            ("cpu", &curve.cpu),
            ("gpu1", &curve.gpu1),
            ("gpu2", &curve.gpu2),
        ] {
            write!(out, "{name:<4}:")?;
            for point in points {
                write!(out, " ({}C,{}%)", point.temp, point.duty_pct)?;
            }
            writeln!(out)?;
        }
        writeln!(
            out,
            "payload: [2]={} [3]={} [4]={} [5]={}",
            payload[2], payload[3], payload[4], payload[5]
        )?;
        writeln!(out, "pass --apply to execute")?;
        return Ok(());
    }

    if !transport.writable() {
        return Err(CliError::Transport(
            clevo_transport::TransportError::NotVerified("fan curve write".into()),
        ));
    }

    transport.execute(CMD_FAN_CURVE_WRITE.get(), &payload_from_slice(&payload)?)?;
    // Selecting `custom` is what makes the firmware use the new table.
    let mode = build_subcommand_payload(u32::from(FAN_MODE_CUSTOM), SUB_FAN_MODE);
    transport.execute(CMD_MAIN.get(), &mode)?;
    writeln!(out, "wrote fan curve and set fan mode to custom")?;
    Ok(())
}

/// Continuously print fan status until `count` samples are emitted (`0` = run
/// until interrupted) or the caller stops reading.
pub fn run_fan_watch(
    transport: &dyn Transport,
    interval_ms: u64,
    count: u64,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut emitted: u64 = 0;
    loop {
        let snapshot = read_fan_snapshot(transport)?;
        if json {
            writeln!(out, "{}", format_json(&snapshot))?;
        } else {
            writeln!(out, "{}", format_row(&snapshot))?;
        }
        out.flush()?;

        emitted += 1;
        if count != 0 && emitted >= count {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
    }
}

/// Print a capability report.
pub fn print_capabilities(caps: &clevo_proto::Capabilities, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "page7 version   : {}", version_str(caps.version))?;
    writeln!(out, "power mode UI id: {}", caps.power_mode_ui_id)?;
    writeln!(out, "turbo fan       : {}", caps.turbo_fan)?;
    writeln!(out, "ms hybrid switch: {}", caps.ms_hybrid_switch)?;
    writeln!(out, "nvidia power off: {}", caps.nvidia_power_off)?;
    writeln!(out, "slow fan        : {}", caps.slow_fan)?;
    let names = ["quiet", "pwrsaving", "performance", "entertainment"];
    for (value, name) in names.iter().enumerate() {
        writeln!(
            out,
            "{value} {name:<14} {}",
            if caps.power_modes.supports(value as u8) {
                "supported"
            } else {
                "unsupported"
            }
        )?;
    }
    Ok(())
}

/// Dispatch a profile sub-command.
pub fn run_profile(
    transport: &dyn Transport,
    command: &ProfileCommand,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    match command {
        ProfileCommand::List => {
            let caps = read_capabilities(transport)?;
            print_capabilities(&caps, out)?;
            Ok(())
        }
        ProfileCommand::Set { value, apply } => {
            if *value > 3 {
                return Err(CliError::Invalid(format!(
                    "performance mode must be 0..=3, got {value}"
                )));
            }
            let caps = read_capabilities(transport)?;
            if !caps.power_modes.supports(*value) {
                return Err(CliError::Invalid(format!(
                    "performance mode {value} is not supported by this machine"
                )));
            }
            set_axis(
                transport,
                SUB_POWER_MODE,
                *value,
                *apply,
                out,
                "performance mode",
            )
        }
    }
}

fn set_axis(
    transport: &dyn Transport,
    sub: u8,
    value: u8,
    apply: bool,
    out: &mut dyn Write,
    what: &str,
) -> Result<(), CliError> {
    if !apply {
        writeln!(
            out,
            "dry run: would set {what} to {value} (command {} sub {sub}); pass --apply to execute",
            CMD_MAIN.get()
        )?;
        return Ok(());
    }

    if !transport.writable() {
        return Err(CliError::Transport(
            clevo_transport::TransportError::NotVerified(format!("{what} write")),
        ));
    }

    let payload = build_subcommand_payload(u32::from(value), sub);
    transport.execute(CMD_MAIN.get(), &payload)?;
    writeln!(out, "set {what} to {value}")?;
    Ok(())
}

fn version_str(version: Page7Version) -> &'static str {
    match version {
        Page7Version::V0 => "v0 (0x0000)",
        Page7Version::V1 => "v1 (0x0100)",
    }
}

/// Print a diagnostic report. Does not modify the system.
pub fn run_doctor(transport: &dyn Transport, out: &mut dyn Write) -> Result<(), CliError> {
    writeln!(out, "clevo-cc doctor")?;
    writeln!(out, "transport    : {:?}", transport.kind())?;
    writeln!(out, "writable     : {}", transport.writable())?;
    writeln!(out, "os           : {}", std::env::consts::OS)?;
    writeln!(out, "arch         : {}", std::env::consts::ARCH)?;
    writeln!(
        out,
        "acpi_call    : {}",
        match transport.kind() {
            clevo_transport::TransportKind::AcpiCall => "in use (read-only: commands 12 and 13)",
            _ => "not in use (mock transport)",
        }
    )?;
    writeln!(out, "no changes were made.")?;
    Ok(())
}

/// Default empty output writer helper for tests.
pub fn string_output() -> io::BufWriter<Vec<u8>> {
    io::BufWriter::new(Vec::new())
}

/// Run the CLI against the `clevod` daemon over D-Bus.
///
/// This is the preferred path: the daemon owns the hardware, validates writes
/// and gates them through PolicyKit, and the CLI only formats its replies.
pub fn run_dbus(
    client: &crate::dbus::DbusClient,
    command: &crate::Commands,
    watch: Option<(u64, u64, bool)>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    match command {
        crate::Commands::Doctor => {
            writeln!(out, "clevo-cc doctor")?;
            writeln!(out, "transport    : Dbus ({})", crate::dbus::DBUS_NAME)?;
            let status = client.status()?;
            writeln!(out, "writable     : {}", status.writable)?;
            writeln!(out, "os           : {}", std::env::consts::OS)?;
            writeln!(out, "no changes were made.")?;
            Ok(())
        }
        crate::Commands::Capabilities => Err(CliError::TransportUnavailable(
            "capabilities are not exposed over D-Bus yet".into(),
        )),
        crate::Commands::Fan(fan) => run_fan_dbus(client, fan, watch, out),
        crate::Commands::Profile(profile) => run_profile_dbus(client, profile, out),
    }
}

fn run_fan_dbus(
    client: &crate::dbus::DbusClient,
    command: &FanCommand,
    watch: Option<(u64, u64, bool)>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    match command {
        FanCommand::Status => print_dbus_status(client, out),
        FanCommand::Curve => {
            let json = client.get_curve()?;
            writeln!(out, "{json}")?;
            Ok(())
        }
        FanCommand::Watch { .. } => {
            let (interval_ms, count, json) =
                watch.ok_or_else(|| CliError::Invalid("missing watch parameters".into()))?;
            let mut emitted = 0u64;
            loop {
                if json {
                    let status = client.status()?;
                    writeln!(out, "{}", dbus_json(&status))?;
                } else {
                    print_dbus_status(client, out)?;
                }
                out.flush()?;
                emitted += 1;
                if count != 0 && emitted >= count {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(interval_ms));
            }
        }
        FanCommand::SetMode { mode, apply } => {
            if !*apply {
                writeln!(out, "dry run: would set fan mode to {mode:?}; pass --apply")?;
                return Ok(());
            }
            let value = client.set_fan_mode(mode)?;
            writeln!(out, "set fan mode {mode:?} (121/1 = {value})")?;
            Ok(())
        }
        FanCommand::SetCurve {
            cpu,
            gpu1,
            gpu2,
            apply,
        } => {
            let curve = curve_from_args(cpu, gpu1.as_deref(), gpu2.as_deref())?;
            if !*apply {
                writeln!(
                    out,
                    "dry run: would write fan curve and set custom mode via the daemon"
                )?;
                writeln!(out, "{}", curve_to_json(&curve))?;
                writeln!(out, "pass --apply to execute")?;
                return Ok(());
            }
            client.set_curve(&curve_to_json(&curve))?;
            writeln!(out, "wrote fan curve and set fan mode to custom")?;
            Ok(())
        }
    }
}

fn run_profile_dbus(
    client: &crate::dbus::DbusClient,
    command: &ProfileCommand,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    match command {
        ProfileCommand::List => {
            writeln!(
                out,
                "performance modes are validated by the daemon; use `profile set <0..3> --apply`"
            )?;
            Ok(())
        }
        ProfileCommand::Set { value, apply } => {
            if *value > 3 {
                return Err(CliError::Invalid(format!(
                    "performance mode must be 0..=3, got {value}"
                )));
            }
            let name = perf_mode_name(*value)
                .ok_or_else(|| CliError::Invalid(format!("no such performance mode {value}")))?;
            if !*apply {
                writeln!(
                    out,
                    "dry run: would set performance mode {name:?}; pass --apply"
                )?;
                return Ok(());
            }
            let applied = client.set_perf_mode(name)?;
            writeln!(out, "set performance mode {name:?} (121/25 = {applied})")?;
            Ok(())
        }
    }
}

fn perf_mode_name(value: u8) -> Option<&'static str> {
    match value {
        0 => Some("quiet"),
        1 => Some("pwrsaving"),
        2 => Some("performance"),
        3 => Some("entertainment"),
        _ => None,
    }
}

fn print_dbus_status(
    client: &crate::dbus::DbusClient,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let status = client.status()?;
    let temp = |c: u8| {
        if c == 0 {
            "n/a".to_string()
        } else {
            format!("{c}C")
        }
    };
    writeln!(
        out,
        "CPU  rpm={:<5} duty={:<4}% temp={}",
        status.cpu_rpm,
        crate::duty_pct(status.cpu_duty),
        temp(status.cpu_temp_c)
    )?;
    if status.fan_count == 0 || status.fan_count >= 2 {
        writeln!(
            out,
            "GPU1 rpm={:<5} duty={:<4}% temp={}",
            status.gpu_rpm,
            crate::duty_pct(status.gpu_duty),
            temp(status.gpu_temp_c)
        )?;
    } else {
        writeln!(out, "GPU1 n/a (channel not present)")?;
    }
    if status.fan_count < 3 {
        writeln!(out, "GPU2 n/a (channel not present)")?;
    }
    writeln!(
        out,
        "freshness: {}  fan_mode: {}  perf_mode: {}",
        status.freshness, status.fan_mode, status.perf_mode
    )?;
    Ok(())
}

fn dbus_json(status: &crate::dbus::DbusStatus) -> String {
    let temp = |c: u8| {
        if c == 0 {
            "null".to_string()
        } else {
            c.to_string()
        }
    };
    let gpu1 = if status.fan_count == 0 || status.fan_count >= 2 {
        format!(
            "{{\"rpm\":{},\"duty_pct\":{},\"temp_c\":{}}}",
            status.gpu_rpm,
            crate::duty_pct(status.gpu_duty),
            temp(status.gpu_temp_c)
        )
    } else {
        "null".to_string()
    };
    format!(
        "{{\"cpu\":{{\"rpm\":{},\"duty_pct\":{},\"temp_c\":{}}},\"gpu1\":{},\"freshness\":\"{}\",\"fan_mode\":{},\"perf_mode\":{}}}",
        status.cpu_rpm,
        crate::duty_pct(status.cpu_duty),
        temp(status.cpu_temp_c),
        gpu1,
        status.freshness,
        status.fan_mode,
        status.perf_mode
    )
}
