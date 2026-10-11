//! Named driver protocol; the kernel owns ACPI payloads and recovery.

use super::{ChargeControl, ChargeThresholds, PowerError};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) struct NativeCharge {
    pub path: PathBuf,
    pub thresholds: ChargeThresholds,
    pub start_values: Vec<u8>,
    pub end_values: Vec<u8>,
    battery: String,
}

impl NativeCharge {
    pub fn read(sys: &Path) -> Result<Option<Self>, PowerError> {
        let path = sys.join("devices/platform/CLV0001:00/flexicharger");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.raw_os_error() == Some(95) => {
                return Err(PowerError::Unsupported(
                    "firmware does not provide FlexiCharger".into(),
                ));
            }
            Err(error) => return Err(PowerError::Io(format!("{}: {error}", path.display()))),
        };
        Self::parse(path, &text).map(Some).map_err(PowerError::Io)
    }

    fn parse(path: PathBuf, text: &str) -> Result<Self, String> {
        let mut fields = BTreeMap::new();
        for line in text.lines() {
            let (key, value) = line.split_once('=').ok_or("malformed FlexiCharger field")?;
            if fields.insert(key, value).is_some() {
                return Err("duplicate FlexiCharger field".into());
            }
        }
        let get = |key| {
            fields
                .get(key)
                .copied()
                .ok_or_else(|| format!("missing FlexiCharger {key}"))
        };
        let percent = |key| {
            get(key)?
                .parse::<u8>()
                .map_err(|_| format!("invalid FlexiCharger {key}"))
        };
        let values = |key| -> Result<Vec<u8>, String> {
            let parsed = get(key)?
                .split_whitespace()
                .map(|value| value.parse::<u8>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| format!("invalid FlexiCharger {key}"))?;
            if parsed.is_empty()
                || parsed.len() > 16
                || parsed.iter().any(|value| *value == 0 || *value > 100)
                || parsed.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(format!("invalid FlexiCharger {key}"));
            }
            Ok(parsed)
        };
        let enabled = match get("enabled")? {
            "0" => false,
            "1" => true,
            _ => return Err("invalid FlexiCharger enabled".into()),
        };
        let start = percent("start")?;
        let end = percent("end")?;
        let start_values = values("start_values")?;
        let end_values = values("end_values")?;
        let battery = get("battery")?.to_string();
        if fields.len() != 6
            || !battery.starts_with('\\')
            || start >= end
            || !start_values.contains(&start)
            || !end_values.contains(&end)
        {
            return Err("invalid FlexiCharger state or battery identity".into());
        }
        Ok(Self {
            path,
            battery,
            thresholds: ChargeThresholds {
                enabled: Some(enabled),
                start: Some(start),
                end,
            },
            start_values,
            end_values,
        })
    }

    pub fn matches_battery(&self, path: &Path) -> bool {
        ["device/path", "device/firmware_node/path"]
            .iter()
            .any(|name| {
                fs::read_to_string(path.join(name)).is_ok_and(|value| value.trim() == self.battery)
            })
    }

    pub fn control(&self) -> ChargeControl {
        ChargeControl {
            backend: "clevo-flexicharger".into(),
            start_values: self.start_values.clone(),
            end_values: self.end_values.clone(),
        }
    }
}

pub(super) fn write(path: &Path, target: ChargeThresholds) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|mut file| write_command(&mut file, target))
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn write_command(writer: &mut impl Write, target: ChargeThresholds) -> std::io::Result<()> {
    // Each sysfs write is a complete command, so formatting must finish first.
    let command = format!(
        "{} {} {}\n",
        u8::from(target.enabled.unwrap_or(true)),
        target.start.unwrap(),
        target.end
    );
    writer.write_all(command.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A sysfs store callback parses each write separately; fragments are not buffered.
    struct Store {
        attempts: Vec<Vec<u8>>,
        accepted: Vec<ChargeThresholds>,
    }

    impl Write for Store {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.attempts.push(bytes.into());
            let fields = std::str::from_utf8(bytes)
                .unwrap()
                .split_whitespace()
                .map(str::parse::<u8>)
                .collect::<Result<Vec<_>, _>>();
            match fields {
                Ok(fields) if fields.len() == 3 && fields[0] <= 1 && fields[1] < fields[2] => {
                    self.accepted.push(ChargeThresholds {
                        enabled: Some(fields[0] == 1),
                        start: Some(fields[1]),
                        end: fields[2],
                    });
                    Ok(bytes.len())
                }
                _ => Err(std::io::Error::from_raw_os_error(22)),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn writes_complete_enable_and_restore_commands_to_sysfs_in_one_write() {
        let mut store = Store {
            attempts: vec![],
            accepted: vec![],
        };
        for target in [
            ChargeThresholds {
                enabled: Some(true),
                start: Some(40),
                end: 80,
            },
            ChargeThresholds {
                enabled: Some(false),
                start: Some(95),
                end: 100,
            },
        ] {
            let before = store.attempts.len();
            write_command(&mut store, target).unwrap();
            assert_eq!(store.attempts.len(), before + 1);
            assert_eq!(store.accepted.last(), Some(&target));
        }
        assert_eq!(
            store.attempts,
            vec![b"1 40 80\n".to_vec(), b"0 95 100\n".to_vec()]
        );
    }
}
