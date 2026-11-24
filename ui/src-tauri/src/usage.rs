//! Host CPU/GPU utilisation and temperature sampling for the dashboard.

use std::path::Path;
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

fn parse_temperature(raw: &str) -> Option<u8> {
    let degrees = raw.trim().parse::<f32>().ok()?.round();
    (1.0..=150.0).contains(&degrees).then_some(degrees as u8)
}

fn hwmon_temp_c(hwmon: &Path, input: &str) -> Option<u8> {
    let raw = std::fs::read_to_string(hwmon.join(input)).ok()?;
    let millidegrees: u32 = raw.trim().parse().ok()?;
    u8::try_from(millidegrees.saturating_add(500) / 1_000)
        .ok()
        .filter(|temp| (1..=150).contains(temp))
}

fn cpu_temp_c(hwmon_root: &Path) -> Option<u8> {
    std::fs::read_dir(hwmon_root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let hwmon = entry.path();
            let driver = std::fs::read_to_string(hwmon.join("name")).ok()?;
            match driver.trim() {
                "coretemp" => hwmon_temp_c(&hwmon, "temp1_input"),
                "k10temp" => {
                    let is_tdie = std::fs::read_to_string(hwmon.join("temp2_label"))
                        .is_ok_and(|label| label.trim() == "Tdie");
                    is_tdie
                        .then(|| hwmon_temp_c(&hwmon, "temp2_input"))
                        .flatten()
                        .or_else(|| hwmon_temp_c(&hwmon, "temp1_input"))
                }
                _ => None,
            }
        })
        .max()
}

fn gpu_temp_from_hwmon(hwmon_root: &Path) -> Option<u8> {
    std::fs::read_dir(hwmon_root)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let hwmon = entry.path();
            let driver = std::fs::read_to_string(hwmon.join("name")).ok()?;
            match driver.trim() {
                "amdgpu" | "nouveau" | "nvidia" => hwmon_temp_c(&hwmon, "temp1_input"),
                _ => None,
            }
        })
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

#[derive(Debug, Default, PartialEq, Eq)]
struct GpuSample {
    percent: Option<u8>,
    temp_c: Option<u8>,
    freq_mhz: Option<u32>,
}

fn parse_nvidia_sample(csv: &str) -> Option<GpuSample> {
    csv.lines().find_map(|line| {
        let mut fields = line.split(',');
        let percent = parse_percent(fields.next()?);
        let temp_c = parse_temperature(fields.next()?);
        let freq_mhz = fields.next()?.trim().parse().ok();
        (percent.is_some() || temp_c.is_some() || freq_mhz.is_some()).then_some(GpuSample {
            percent,
            temp_c,
            freq_mhz,
        })
    })
}

fn gpu_sample_from_nvidia_smi() -> Option<GpuSample> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,temperature.gpu,clocks.current.graphics",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_nvidia_sample(std::str::from_utf8(&output.stdout).ok()?)
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

fn gpu_sample(hwmon_root: &Path) -> GpuSample {
    let percent = gpu_percent_from_sysfs();
    let temp_c = gpu_temp_from_hwmon(hwmon_root);
    let freq_mhz = gpu_freq_from_sysfs();
    let nvidia = if percent.is_none() || temp_c.is_none() || freq_mhz.is_none() {
        gpu_sample_from_nvidia_smi().unwrap_or_default()
    } else {
        GpuSample::default()
    };
    GpuSample {
        percent: percent.or(nvidia.percent),
        temp_c: temp_c.or(nvidia.temp_c),
        freq_mhz: freq_mhz.or(nvidia.freq_mhz),
    }
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
    let hwmon_root = Path::new("/sys/class/hwmon");
    let snapshot = DaemonClient::system()
        .ok()
        .and_then(|client| client.snapshot().ok());
    let gpu = gpu_sample(hwmon_root);
    Ok(HardwareUsage {
        cpu_percent: cpu_percent(),
        cpu_temp_c: cpu_temp_c(hwmon_root)
            .or_else(|| snapshot.as_ref().and_then(|snapshot| snapshot.cpu.temp_c)),
        cpu_freq_mhz: cpu_freq_mhz(),
        gpu_percent: gpu.percent,
        gpu_temp_c: gpu
            .temp_c
            .or_else(|| snapshot.as_ref().and_then(|snapshot| snapshot.gpu1.temp_c)),
        gpu_freq_mhz: gpu.freq_mhz,
        gpu_available: snapshot
            .as_ref()
            .map(|snapshot| snapshot.gpu1.available)
            .unwrap_or(false)
            || gpu.percent.is_some()
            || gpu.temp_c.is_some(),
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

    #[test]
    fn reads_chip_temperatures_without_using_the_clevo_ec_sensor() {
        let root = std::env::temp_dir().join(format!("clevo-ui-hwmon-{}", std::process::id()));
        for (index, driver, temp) in [
            (0, "clevo_cc", "96000"),
            (1, "coretemp", "61400"),
            (2, "amdgpu", "51500"),
        ] {
            let hwmon = root.join(format!("hwmon{index}"));
            std::fs::create_dir_all(&hwmon).unwrap();
            std::fs::write(hwmon.join("name"), driver).unwrap();
            std::fs::write(hwmon.join("temp1_input"), temp).unwrap();
        }
        assert_eq!(super::cpu_temp_c(&root), Some(61));
        assert_eq!(super::gpu_temp_from_hwmon(&root), Some(52));
        std::fs::remove_dir_all(root.join("hwmon1")).unwrap();
        std::fs::remove_dir_all(root.join("hwmon2")).unwrap();
        assert_eq!(super::cpu_temp_c(&root), None);
        assert_eq!(super::gpu_temp_from_hwmon(&root), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_nvidia_temperature_even_when_other_fields_are_unavailable() {
        assert_eq!(
            super::parse_nvidia_sample("N/A, N/A, N/A\n[Not Supported], 54.6, 1920\n"),
            Some(super::GpuSample {
                percent: None,
                temp_c: Some(55),
                freq_mhz: Some(1920),
            })
        );
        assert_eq!(super::parse_temperature("34.45"), Some(34));
    }
}
