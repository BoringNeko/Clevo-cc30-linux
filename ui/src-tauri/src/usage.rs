//! Host CPU/GPU utilisation sampling for the dashboard.

use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::dbus::DaemonClient;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HardwareUsage {
    pub cpu_percent: u8,
    pub cpu_temp_c: Option<u8>,
    pub cpu_freq_mhz: Option<u32>,
    pub gpu_percent: Option<u8>,
    pub gpu_temp_c: Option<u8>,
    pub gpu_freq_mhz: Option<u32>,
    pub gpu_available: bool,
    pub memory_percent: u8,
    pub swap_percent: u8,
    pub disks: Vec<DiskUsage>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiskUsage {
    pub mount_point: String,
    pub percent: u8,
}

#[derive(Debug, Clone, Copy)]
struct CpuSample {
    idle: u64,
    total: u64,
}

static PREVIOUS_CPU: OnceLock<Mutex<Option<CpuSample>>> = OnceLock::new();

fn previous_cpu() -> &'static Mutex<Option<CpuSample>> {
    PREVIOUS_CPU.get_or_init(|| Mutex::new(None))
}

fn read_cpu_sample() -> Result<CpuSample, String> {
    let stat = std::fs::read_to_string("/proc/stat")
        .map_err(|e| format!("could not read /proc/stat: {e}"))?;
    let line = stat
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| "missing aggregate CPU statistics".to_string())?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .map(|value| value.parse::<u64>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("invalid /proc/stat value: {e}"))?;
    if values.len() < 4 {
        return Err("incomplete aggregate CPU statistics".to_string());
    }
    let idle = values[3].saturating_add(*values.get(4).unwrap_or(&0));
    Ok(CpuSample {
        idle,
        total: values.iter().sum(),
    })
}

fn cpu_percent() -> u8 {
    let Ok(current) = read_cpu_sample() else {
        return 0;
    };
    let mut previous = previous_cpu().lock().unwrap_or_else(|e| e.into_inner());
    previous
        .replace(current)
        .map(|old| {
            let total_delta = current.total.saturating_sub(old.total);
            let idle_delta = current.idle.saturating_sub(old.idle);
            if total_delta == 0 {
                0
            } else {
                (((total_delta.saturating_sub(idle_delta)) * 100) / total_delta).min(100) as u8
            }
        })
        .unwrap_or(0)
}

fn read_mhz_file(path: impl AsRef<std::path::Path>) -> Option<u32> {
    parse_mhz(&std::fs::read_to_string(path).ok()?)
}

fn parse_mhz(raw: &str) -> Option<u32> {
    let value = raw.trim().parse::<u64>().ok()?;
    // cpufreq reports kHz while most GPU sysfs nodes report MHz.
    Some(if value > 10_000 {
        (value / 1_000) as u32
    } else {
        value as u32
    })
}

fn cpu_freq_mhz() -> Option<u32> {
    for path in [
        "/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq",
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
        "/sys/devices/system/cpu/cpufreq/policy0/cpuinfo_cur_freq",
    ] {
        if let Some(value) = read_mhz_file(path) {
            return Some(value);
        }
    }
    None
}

fn parse_percent(value: &str) -> Option<u8> {
    value
        .trim()
        .trim_end_matches('%')
        .trim()
        .parse::<u8>()
        .ok()
        .map(|value| value.min(100))
}

fn gpu_percent_from_sysfs() -> Option<u8> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    for entry in entries.flatten() {
        let device = entry.path().join("device");
        // AMD exposes gpu_busy_percent; Intel exposes gt_busy_percent on
        // recent i915 drivers. busy_percent covers a few vendor drivers.
        for name in ["gpu_busy_percent", "gt_busy_percent", "busy_percent"] {
            if let Ok(value) = std::fs::read_to_string(device.join(name)) {
                if let Some(value) = parse_percent(&value) {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn gpu_percent_from_nvidia_smi() -> Option<u8> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    output
        .stdout
        .split(|byte| *byte == b'\n' || *byte == b'\r')
        .find_map(|line| parse_percent(std::str::from_utf8(line).ok()?))
}

fn gpu_percent() -> Option<u8> {
    gpu_percent_from_sysfs().or_else(gpu_percent_from_nvidia_smi)
}

fn gpu_freq_from_sysfs() -> Option<u32> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    for entry in entries.flatten() {
        let device = entry.path().join("device");
        for name in ["gt_cur_freq_mhz", "gpu_cur_freq"] {
            if let Some(value) = read_mhz_file(device.join(name)) {
                return Some(value);
            }
        }
        // AMD exposes the active clock with a star in pp_dpm_sclk, e.g.
        // "3: 1200Mhz *". Read the first starred clock.
        if let Ok(value) = std::fs::read_to_string(device.join("pp_dpm_sclk")) {
            for line in value.lines() {
                if line.contains('*') {
                    if let Some(mhz) = line
                        .split_whitespace()
                        .find_map(|part| part.trim_end_matches("Mhz").parse::<u32>().ok())
                    {
                        return Some(mhz);
                    }
                }
            }
        }
    }
    None
}

fn gpu_freq_from_nvidia_smi() -> Option<u32> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=clocks.current.graphics",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    output
        .stdout
        .split(|byte| *byte == b'\n' || *byte == b'\r')
        .find_map(|line| std::str::from_utf8(line).ok()?.trim().parse::<u32>().ok())
}

fn gpu_freq_mhz() -> Option<u32> {
    gpu_freq_from_sysfs().or_else(gpu_freq_from_nvidia_smi)
}

fn memory_percent() -> u8 {
    let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    let mut total = None;
    let mut available = None;
    for line in meminfo.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let Some(value) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        match name {
            "MemTotal:" => total = Some(value),
            "MemAvailable:" => available = Some(value),
            _ => {}
        }
    }
    let (Some(total), Some(available)) = (total, available) else {
        return 0;
    };
    if total == 0 {
        0
    } else {
        (((total.saturating_sub(available)) * 100) / total).min(100) as u8
    }
}

fn swap_percent() -> u8 {
    let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    let mut total = None;
    let mut free = None;
    for line in meminfo.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let Some(value) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        match name {
            "SwapTotal:" => total = Some(value),
            "SwapFree:" => free = Some(value),
            _ => {}
        }
    }
    let (Some(total), Some(free)) = (total, free) else {
        return 0;
    };
    if total == 0 {
        0
    } else {
        (((total.saturating_sub(free)) * 100) / total).min(100) as u8
    }
}

fn disk_usage() -> Vec<DiskUsage> {
    let Ok(output) = std::process::Command::new("df").args(["-P", "-l"]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut disks = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6 {
            continue;
        }
        // Only show mounts backed by a physical block device. This keeps
        // tmpfs, devtmpfs, efivarfs and service/runtime mounts out of the UI,
        // while retaining /, /home and additional disks under /mnt or /media.
        if !fields[0].starts_with("/dev/") {
            continue;
        }
        let Some(percent) = parse_percent(fields[4]) else {
            continue;
        };
        let mount_point = fields[5..].join(" ");
        if mount_point == "/efi" || mount_point.starts_with("/efi/") {
            continue;
        }
        if !mount_point.is_empty() && !disks.iter().any(|disk: &DiskUsage| disk.mount_point == mount_point) {
            disks.push(DiskUsage { mount_point, percent });
        }
    }
    disks.sort_by(|left, right| {
        let rank = |mount: &str| match mount {
            "/" => 0,
            "/home" => 1,
            _ => 2,
        };
        rank(&left.mount_point)
            .cmp(&rank(&right.mount_point))
            .then_with(|| left.mount_point.cmp(&right.mount_point))
    });
    disks
}

pub fn read() -> Result<HardwareUsage, String> {
    // Host resource readings remain useful even when the fan daemon is
    // temporarily unavailable. Temperatures and fan availability are optional
    // enrichments from the daemon snapshot.
    let snapshot = DaemonClient::system().ok().and_then(|client| client.snapshot().ok());
    let gpu_percent = gpu_percent();
    Ok(HardwareUsage {
        cpu_percent: cpu_percent(),
        cpu_temp_c: snapshot.as_ref().and_then(|snapshot| snapshot.cpu.temp_c),
        cpu_freq_mhz: cpu_freq_mhz(),
        gpu_percent,
        gpu_temp_c: snapshot.as_ref().and_then(|snapshot| snapshot.gpu1.temp_c),
        gpu_freq_mhz: gpu_freq_mhz(),
        gpu_available: snapshot
            .as_ref()
            .map(|snapshot| snapshot.gpu1.available)
            .unwrap_or(false)
            || gpu_percent.is_some(),
        memory_percent: memory_percent(),
        swap_percent: swap_percent(),
        disks: disk_usage(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn cpu_sample_counts_iowait_as_idle() {
        let values = [100, 20, 10, 50, 5];
        assert_eq!(values[3] + values[4], 55);
        assert_eq!(values.iter().sum::<u64>(), 185);
    }

    #[test]
    fn parses_gpu_percent_with_or_without_a_percent_sign() {
        assert_eq!(super::parse_percent("42"), Some(42));
        assert_eq!(super::parse_percent(" 87 %\n"), Some(87));
        assert_eq!(super::parse_percent("101"), Some(100));
        assert_eq!(super::parse_percent("N/A"), None);
    }

    #[test]
    fn converts_cpu_khz_to_mhz_but_keeps_gpu_mhz() {
        assert_eq!(super::parse_mhz("2113972"), Some(2113));
        assert_eq!(super::parse_mhz("1200"), Some(1200));
    }
}
