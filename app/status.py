import glob
import os
import time
import re
from threading import Lock
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


def _select_stats_path(
    env_key: str, mounted_path: str, native_path: str, probe_path: str | None = None
) -> str:
    configured = os.environ.get(env_key)
    if configured:
        return configured

    check_path = probe_path or mounted_path
    return mounted_path if os.path.exists(check_path) else native_path


HOST_PROC = _select_stats_path("HOST_PROC", "/host/proc", "/proc", "/host/proc/uptime")
HOST_SYS = _select_stats_path("HOST_SYS", "/host/sys", "/sys", "/host/sys/class")
HOST_ROOT = _select_stats_path("HOST_ROOT", "/host/root", "/", "/host/root/proc")
HOST_ETC = _select_stats_path("HOST_ETC", "/host/etc", "/etc", "/host/etc/hostname")

CPU_THERMAL_PREFIXES = (
    "cpu",
    "cpuss",
)

CPU_HWMON_NAMES = {
    "coretemp",       # Intel
    "k10temp",        # AMD
    "zenpower",       # AMD
    "cpu_thermal",
    "x86_pkg_temp",
    "fam15h_power",
    "fam17h_power",
    "fam19h_power",
}

CPU_LABEL_KEYWORDS = (
    "cpu",
    "package",
    "tdie",
    "tctl",
    "core",
    "physical id",
)

_cpu_sample_lock = Lock()
_last_cpu_totals = None

# Read hardware constants once and cache
# since they won't change at runtime.
_cpu_cores: int | None = None
_cpu_max_hz: float | None = None


def _host_path(root: str, path: str) -> str:
    return os.path.join(root, path.lstrip("/"))


def is_service_online(url: str, timeout_seconds: float = 2.5) -> bool:
    req = Request(url, method="HEAD")
    try:
        with urlopen(req, timeout=timeout_seconds) as response:
            return response.status < 500
    except HTTPError as err:
        return err.code < 500
    except (URLError, TimeoutError, ValueError):
        return False


def _read_linux_uptime_seconds() -> float | None:
    try:
        with open(_host_path(HOST_PROC, "uptime"), encoding="utf-8") as f:
            value = f.read().split()[0]
            return float(value)
    except (OSError, ValueError, IndexError):
        return None


def _read_system_hostname() -> str:
    # Try the dedicated host mount first (hardened compose uses /host/etc/hostname).
    try:
        with open(_host_path(HOST_ETC, "hostname"), encoding="utf-8") as f:
            name = f.read().strip()
        if name:
            return name
    except OSError:
        pass

    # Fallback: try /etc/hostname path selected via HOST_ROOT.
    try:
        with open(_host_path(HOST_ROOT, "etc/hostname"), encoding="utf-8") as f:
            name = f.read().strip()
        if name:
            return name
    except OSError:
        pass

    # Fallback to /proc/sys/kernel/hostname
    try:
        with open(_host_path(HOST_PROC, "sys/kernel/hostname"), encoding="utf-8") as f:
            name = f.read().strip()
        if name:
            return name
    except OSError:
        return "unknown-host"


def _read_device_model() -> str:
    paths = (
        _host_path(HOST_SYS, "devices/virtual/dmi/id/product_name"),
        _host_path(HOST_SYS, "devices/virtual/dmi/id/product_version"),
        _host_path(HOST_SYS, "devices/virtual/dmi/id/board_name"),
    )
    values = []

    for path in paths:
        try:
            with open(path, encoding="utf-8") as f:
                value = f.read().strip()
            if value and value not in {
                "To Be Filled By O.E.M.",
                "None",
                "Default string",
            }:
                values.append(value)
        except OSError:
            continue

    if values:
        return " ".join(dict.fromkeys(values[:2]))
    return "this device"


def _read_linux_memory_usage() -> tuple[int, int, float] | None:
    try:
        mem_info = {}
        with open(_host_path(HOST_PROC, "meminfo"), encoding="utf-8") as f:
            for line in f:
                parts = line.replace(":", "").split()
                if len(parts) >= 2:
                    mem_info[parts[0]] = int(parts[1])

        total_kb = mem_info.get("MemTotal")
        available_kb = mem_info.get("MemAvailable")
        if not total_kb or available_kb is None:
            return None

        used_kb = max(total_kb - available_kb, 0)
        percent = (used_kb / total_kb) * 100 if total_kb > 0 else 0.0
        return total_kb * 1024, used_kb * 1024, percent
    except (OSError, ValueError):
        return None


def _read_linux_cpu_percent() -> float | None:
    global _last_cpu_totals

    try:
        with open(_host_path(HOST_PROC, "stat"), encoding="utf-8") as f:
            first = f.readline().strip()
        if not first.startswith("cpu "):
            return None

        values = [int(v) for v in first.split()[1:]]
        if len(values) < 4:
            return None

        idle = values[3] + (values[4] if len(values) > 4 else 0)
        total = sum(values)

        with _cpu_sample_lock:
            previous = _last_cpu_totals
            _last_cpu_totals = (total, idle)

        if previous is None:
            return None

        prev_total, prev_idle = previous
        total_delta = total - prev_total
        idle_delta = idle - prev_idle
        if total_delta <= 0:
            return None

        busy_ratio = 1 - (idle_delta / total_delta)
        return max(0.0, min(100.0, busy_ratio * 100.0))
    except (OSError, ValueError):
        return None


def _read_linux_cpu_info() -> tuple[int, float | None]:
    """Return (core_count, max_clock_hz_or_None)."""
    core_count = 0
    try:
        with open(_host_path(HOST_PROC, "cpuinfo"), encoding="utf-8") as f:
            for line in f:
                if line.startswith("processor"):
                    core_count += 1
    except OSError:
        pass
    core_count = core_count or 1

    max_hz = None
    try:
        # cpuinfo_max_freq is in kHz; choose the highest across all cpuN cores.
        cpu_root = _host_path(HOST_SYS, "devices/system/cpu")
        for entry in os.listdir(cpu_root):
            if not re.fullmatch(r"cpu\d+", entry):
                continue

            freq_path = os.path.join(cpu_root, entry, "cpufreq", "cpuinfo_max_freq")
            try:
                with open(freq_path, encoding="utf-8") as f:
                    hz = int(f.read().strip()) * 1000
                max_hz = hz if max_hz is None else max(max_hz, hz)
            except (OSError, ValueError):
                continue
    except (OSError, ValueError):
        # Fall back to the highest reported MHz in /proc/cpuinfo.
        try:
            with open(_host_path(HOST_PROC, "cpuinfo"), encoding="utf-8") as f:
                for line in f:
                    if line.lower().startswith("cpu mhz"):
                        hz = float(line.split(":", 1)[1].strip()) * 1_000_000
                        max_hz = hz if max_hz is None else max(max_hz, hz)
        except (OSError, ValueError):
            pass

    return core_count, max_hz


def _read_linux_disk_usage() -> tuple[int, int, float] | None:
    try:
        stat = os.statvfs(HOST_ROOT)
        total_bytes = stat.f_blocks * stat.f_frsize
        free_bytes = stat.f_bavail * stat.f_frsize
        used_bytes = total_bytes - free_bytes
        percent = (used_bytes / total_bytes) * 100 if total_bytes > 0 else 0.0
        return total_bytes, used_bytes, percent
    except OSError:
        return None


def _read_temp_file(path: str) -> float | None:
    """Read a temperature file and return Celsius value."""
    try:
        with open(path, encoding="utf-8") as f:
            value = int(f.read().strip())

        temp = value / 1000.0

        if 0 <= temp < 200:
            return round(temp, 1)

    except (OSError, ValueError):
        pass

    return None


def _read_linux_cpu_temperature() -> float | None:
    """Read CPU temperature with multi-pass strategy for reliability.
    
    PASS 1: Thermal zones - prioritizes CPU-related zones
    PASS 2: hwmon devices - checks device names and sensor labels
    PASS 3: Fallback to any remaining thermal data
    
    Returns the maximum temperature found in CPU-specific sources.
    """
    thermal_candidates = []
    fallback_candidates = []

    #
    # PASS 1: thermal zones
    #
    thermal_root = _host_path(HOST_SYS, "class/thermal")

    try:
        if os.path.isdir(thermal_root):

            for zone in os.listdir(thermal_root):

                if not zone.startswith("thermal_zone"):
                    continue

                zone_dir = os.path.join(thermal_root, zone)

                type_file = os.path.join(zone_dir, "type")
                temp_file = os.path.join(zone_dir, "temp")

                temp = _read_temp_file(temp_file)

                if temp is None:
                    continue

                sensor_type = ""

                try:
                    with open(type_file, encoding="utf-8") as f:
                        sensor_type = f.read().strip().lower()
                except OSError:
                    pass

                if sensor_type.startswith(CPU_THERMAL_PREFIXES):
                    thermal_candidates.append(temp)

                elif any(
                    k in sensor_type
                    for k in (
                        "cpu",
                        "tcpu",
                        "package",
                        "x86_pkg",
                        "soc",
                    )
                ):
                    thermal_candidates.append(temp)

                else:
                    fallback_candidates.append(temp)

    except OSError:
        pass

    if thermal_candidates:
        return max(thermal_candidates)

    #
    # PASS 2: hwmon
    #
    hwmon_root = _host_path(HOST_SYS, "class/hwmon")

    try:
        if os.path.isdir(hwmon_root):

            preferred = []
            generic = []

            for hwmon in os.listdir(hwmon_root):

                hwmon_dir = os.path.join(hwmon_root, hwmon)

                if not os.path.isdir(hwmon_dir):
                    continue

                hwmon_name = ""

                try:
                    with open(
                        os.path.join(hwmon_dir, "name"),
                        encoding="utf-8",
                    ) as f:
                        hwmon_name = f.read().strip().lower()
                except OSError:
                    pass

                for input_file in glob.glob(
                    os.path.join(hwmon_dir, "temp*_input")
                ):
                    temp = _read_temp_file(input_file)

                    if temp is None:
                        continue

                    label = ""

                    label_file = input_file.replace(
                        "_input",
                        "_label",
                    )

                    try:
                        with open(
                            label_file,
                            encoding="utf-8",
                        ) as f:
                            label = f.read().strip().lower()
                    except OSError:
                        pass

                    is_cpu = False

                    if hwmon_name in CPU_HWMON_NAMES:
                        is_cpu = True

                    elif any(
                        word in label
                        for word in CPU_LABEL_KEYWORDS
                    ):
                        is_cpu = True

                    if is_cpu:
                        preferred.append(temp)
                    else:
                        generic.append(temp)

            if preferred:
                return max(preferred)

            if generic:
                return max(generic)

    except OSError:
        pass

    #
    # PASS 3: final fallback
    #
    if fallback_candidates:
        return max(fallback_candidates)

    return None


def get_system_stats() -> dict:
    uptime_seconds = _read_linux_uptime_seconds()
    if uptime_seconds is None:
        uptime_seconds = time.monotonic()

    mem_total_bytes = None
    mem_used_bytes = None
    memory_percent = None
    memory_stats = _read_linux_memory_usage()
    if memory_stats:
        mem_total_bytes, mem_used_bytes, memory_percent = memory_stats

    cpu_percent = _read_linux_cpu_percent()
    cpu_temperature = _read_linux_cpu_temperature()
    global _cpu_cores, _cpu_max_hz
    if _cpu_cores is None:
        _cpu_cores, _cpu_max_hz = _read_linux_cpu_info()
    cpu_cores, cpu_max_hz = _cpu_cores, _cpu_max_hz

    disk_total_bytes = None
    disk_used_bytes = None
    disk_percent = None
    disk_stats = _read_linux_disk_usage()
    if disk_stats:
        disk_total_bytes, disk_used_bytes, disk_percent = disk_stats

    return {
        "uptime_seconds": int(max(0, uptime_seconds)),
        "memory": {
            "total_bytes": mem_total_bytes,
            "used_bytes": mem_used_bytes,
            "percent": None if memory_percent is None else round(memory_percent, 1),
        },
        "cpu": {
            "percent": None if cpu_percent is None else round(cpu_percent, 1),
            "cores": cpu_cores,
            "max_hz": cpu_max_hz,
            "temperature_celsius": cpu_temperature,
        },
        "disk": {
            "total_bytes": disk_total_bytes,
            "used_bytes": disk_used_bytes,
            "percent": None if disk_percent is None else round(disk_percent, 1),
        },
        "timestamp": int(time.time()),
    }
