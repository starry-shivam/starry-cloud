use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

#[derive(Clone)]
struct HostPaths {
    proc_root: PathBuf,
    sys_root: PathBuf,
    root: PathBuf,
    etc_root: PathBuf,
}

pub struct SystemMonitor {
    paths: HostPaths,
    previous_cpu: Mutex<Option<(u64, u64)>>,
    cpu_info: OnceLock<(usize, Option<f64>)>,
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self {
            paths: HostPaths::from_environment(),
            previous_cpu: Mutex::new(None),
            cpu_info: OnceLock::new(),
        }
    }
}

impl HostPaths {
    fn from_environment() -> Self {
        Self {
            proc_root: select_path("HOST_PROC", "/host/proc", "/proc", "/host/proc/uptime"),
            sys_root: select_path("HOST_SYS", "/host/sys", "/sys", "/host/sys/class"),
            root: select_path("HOST_ROOT", "/host/root", "/", "/host/root/proc"),
            etc_root: select_path("HOST_ETC", "/host/etc", "/etc", "/host/etc/hostname"),
        }
    }

    fn proc(&self, path: &str) -> PathBuf {
        self.proc_root.join(path.trim_start_matches('/'))
    }

    fn sys(&self, path: &str) -> PathBuf {
        self.sys_root.join(path.trim_start_matches('/'))
    }
}

fn select_path(env_key: &str, mounted: &str, native: &str, probe: &str) -> PathBuf {
    if let Ok(configured) = env::var(env_key) {
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
    }
    let mounted_path = PathBuf::from(mounted);
    if Path::new(probe).exists() {
        mounted_path
    } else {
        PathBuf::from(native)
    }
}

fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn uptime_seconds(paths: &HostPaths) -> u64 {
    read_text(&paths.proc("uptime"))
        .and_then(|value| value.split_whitespace().next()?.parse::<f64>().ok())
        .map(|value| value.max(0.0) as u64)
        .unwrap_or(0)
}

fn memory_stats(paths: &HostPaths) -> Option<(u64, u64, f64)> {
    let contents = fs::read_to_string(paths.proc("meminfo")).ok()?;
    let mut total = None;
    let mut available = None;
    for line in contents.lines() {
        let (name, rest) = line.split_once(':')?;
        let amount = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        match name {
            "MemTotal" => total = Some(amount),
            "MemAvailable" => available = Some(amount),
            _ => {}
        }
    }
    let total = total? * 1024;
    let used = total.saturating_sub(available? * 1024);
    let percent = if total == 0 {
        0.0
    } else {
        used as f64 / total as f64 * 100.0
    };
    Some((total, used, percent))
}

fn cpu_totals(paths: &HostPaths) -> Option<(u64, u64)> {
    let contents = fs::read_to_string(paths.proc("stat")).ok()?;
    let first = contents.lines().next()?;
    let mut values = first.strip_prefix("cpu ")?.split_whitespace();
    let parsed = values
        .by_ref()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if parsed.len() < 4 {
        return None;
    }
    let idle = parsed[3] + parsed.get(4).copied().unwrap_or_default();
    Some((parsed.iter().sum(), idle))
}

fn cpu_percent(paths: &HostPaths, previous: &Mutex<Option<(u64, u64)>>) -> Option<f64> {
    let current = cpu_totals(paths)?;
    let mut previous = previous.lock().unwrap_or_else(|error| error.into_inner());
    let old = previous.replace(current)?;
    let total_delta = current.0.checked_sub(old.0)?;
    let idle_delta = current.1.checked_sub(old.1)?;
    if total_delta == 0 {
        return None;
    }
    Some(((1.0 - idle_delta as f64 / total_delta as f64) * 100.0).clamp(0.0, 100.0))
}

fn cpu_information(paths: &HostPaths) -> (usize, Option<f64>) {
    let cpuinfo = fs::read_to_string(paths.proc("cpuinfo")).unwrap_or_default();
    let cores = cpuinfo
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count()
        .max(1);

    let cpu_root = paths.sys("devices/system/cpu");
    let mut max_hz = fs::read_dir(cpu_root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let index = name.strip_prefix("cpu")?;
            if index.is_empty() || !index.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            read_text(&entry.path().join("cpufreq/cpuinfo_max_freq"))?
                .parse::<u64>()
                .ok()
                .map(|khz| khz as f64 * 1000.0)
        })
        .max_by(|left, right| left.total_cmp(right));

    if max_hz.is_none() {
        max_hz = cpuinfo
            .lines()
            .filter(|line| line.to_ascii_lowercase().starts_with("cpu mhz"))
            .filter_map(|line| line.split_once(':')?.1.trim().parse::<f64>().ok())
            .map(|mhz| mhz * 1_000_000.0)
            .max_by(|left, right| left.total_cmp(right));
    }

    (cores, max_hz)
}

fn disk_stats(root: &Path) -> Option<(u64, u64, f64)> {
    let path = std::ffi::CString::new(root.as_os_str().as_encoded_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    let total = (stats.f_blocks as u64).saturating_mul(stats.f_frsize as u64);
    let free = (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64);
    let used = total.saturating_sub(free);
    let percent = if total == 0 {
        0.0
    } else {
        used as f64 / total as f64 * 100.0
    };
    Some((total, used, percent))
}

fn read_temperature(path: &Path) -> Option<f64> {
    let raw = read_text(path)?.parse::<i64>().ok()? as f64 / 1000.0;
    (0.0..200.0)
        .contains(&raw)
        .then_some((raw * 10.0).round() / 10.0)
}

fn cpu_temperature(paths: &HostPaths) -> Option<f64> {
    let thermal_root = paths.sys("class/thermal");
    let mut cpu_candidates = Vec::new();
    let mut fallback_candidates = Vec::new();

    if let Ok(entries) = fs::read_dir(thermal_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("thermal_zone") {
                continue;
            }
            let directory = entry.path();
            let Some(temperature) = read_temperature(&directory.join("temp")) else {
                continue;
            };
            let sensor = read_text(&directory.join("type"))
                .unwrap_or_default()
                .to_lowercase();
            if sensor.starts_with("cpu")
                || sensor.starts_with("cpuss")
                || ["cpu", "tcpu", "package", "x86_pkg", "soc"]
                    .iter()
                    .any(|keyword| sensor.contains(keyword))
            {
                cpu_candidates.push(temperature);
            } else {
                fallback_candidates.push(temperature);
            }
        }
    }
    if let Some(value) = cpu_candidates.into_iter().max_by(f64::total_cmp) {
        return Some(value);
    }

    let preferred_names = [
        "coretemp",
        "k10temp",
        "zenpower",
        "cpu_thermal",
        "x86_pkg_temp",
        "fam15h_power",
        "fam17h_power",
        "fam19h_power",
    ];
    let cpu_keywords = ["cpu", "package", "tdie", "tctl", "core", "physical id"];
    let hwmon_root = paths.sys("class/hwmon");
    let mut preferred = Vec::new();
    let mut generic = Vec::new();
    if let Ok(devices) = fs::read_dir(hwmon_root) {
        for device in devices.flatten() {
            let directory = device.path();
            let name = read_text(&directory.join("name"))
                .unwrap_or_default()
                .to_lowercase();
            let is_known_cpu = preferred_names.contains(&name.as_str());
            let Ok(files) = fs::read_dir(&directory) else {
                continue;
            };
            for file in files.flatten() {
                let filename = file.file_name();
                let filename = filename.to_string_lossy();
                if !filename.starts_with("temp") || !filename.ends_with("_input") {
                    continue;
                }
                let Some(temperature) = read_temperature(&file.path()) else {
                    continue;
                };
                let label_path =
                    PathBuf::from(file.path().to_string_lossy().replace("_input", "_label"));
                let label = read_text(&label_path).unwrap_or_default().to_lowercase();
                if is_known_cpu || cpu_keywords.iter().any(|keyword| label.contains(keyword)) {
                    preferred.push(temperature);
                } else {
                    generic.push(temperature);
                }
            }
        }
    }

    preferred
        .into_iter()
        .max_by(f64::total_cmp)
        .or_else(|| generic.into_iter().max_by(f64::total_cmp))
        .or_else(|| fallback_candidates.into_iter().max_by(f64::total_cmp))
}

fn system_hostname(paths: &HostPaths) -> String {
    let candidates = [
        paths.etc_root.join("hostname"),
        paths.root.join("etc/hostname"),
        paths.proc("sys/kernel/hostname"),
    ];
    candidates
        .iter()
        .filter_map(|path| read_text(path))
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown-host".to_owned())
}

fn device_model(paths: &HostPaths) -> String {
    let paths = [
        paths.sys("devices/virtual/dmi/id/product_name"),
        paths.sys("devices/virtual/dmi/id/product_version"),
        paths.sys("devices/virtual/dmi/id/board_name"),
    ];
    let ignored = ["To Be Filled By O.E.M.", "None", "Default string"];
    let values = paths
        .iter()
        .filter_map(|path| read_text(path))
        .filter(|value| !value.is_empty() && !ignored.contains(&value.as_str()))
        .take(2)
        .collect::<Vec<_>>();
    if values.is_empty() {
        "this device".to_owned()
    } else {
        values.join(" ")
    }
}

fn rounded(value: Option<f64>) -> Option<f64> {
    value.map(|number| (number * 10.0).round() / 10.0)
}

impl SystemMonitor {
    pub fn hostname(&self) -> String {
        system_hostname(&self.paths)
    }

    pub fn device_model(&self) -> String {
        device_model(&self.paths)
    }

    pub fn stats(&self) -> Value {
        let (cores, max_hz) = self.cpu_info.get_or_init(|| cpu_information(&self.paths));
        let memory = memory_stats(&self.paths);
        let disk = disk_stats(&self.paths.root);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        json!({
            "uptime_seconds": uptime_seconds(&self.paths),
            "memory": {
                "total_bytes": memory.map(|stats| stats.0),
                "used_bytes": memory.map(|stats| stats.1),
                "percent": memory.map(|stats| rounded(Some(stats.2))).flatten(),
            },
            "cpu": {
                "percent": rounded(cpu_percent(&self.paths, &self.previous_cpu)),
                "cores": cores,
                "max_hz": max_hz,
                "temperature_celsius": cpu_temperature(&self.paths),
            },
            "disk": {
                "total_bytes": disk.map(|stats| stats.0),
                "used_bytes": disk.map(|stats| stats.1),
                "percent": disk.map(|stats| rounded(Some(stats.2))).flatten(),
            },
            "timestamp": timestamp,
        })
    }
}
