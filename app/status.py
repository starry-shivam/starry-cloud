import os
import time
from socket import gethostname
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
HOST_SYS = _select_stats_path("HOST_SYS", "/host/sys", "/sys", "/host/sys/devices")
HOST_ROOT = _select_stats_path("HOST_ROOT", "/host/root", "/", "/host/root/proc")

_cpu_sample_lock = Lock()
_last_cpu_totals = None


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
    try:
        with open(_host_path(HOST_PROC, "sys/kernel/hostname"), encoding="utf-8") as f:
            name = f.read().strip()
        if name:
            return name
    except OSError:
        pass

    try:
        name = gethostname().strip()
        return name or "unknown-host"
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
            if value and value not in {"To Be Filled By O.E.M.", "None", "Default string"}:
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
        # cpuinfo_max_freq is in kHz
        with open(
            _host_path(HOST_SYS, "devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq"),
            encoding="utf-8",
        ) as f:
            max_hz = int(f.read().strip()) * 1000
    except (OSError, ValueError):
        # Fall back to the reported MHz in /proc/cpuinfo
        try:
            with open(_host_path(HOST_PROC, "cpuinfo"), encoding="utf-8") as f:
                for line in f:
                    if line.lower().startswith("cpu mhz"):
                        max_hz = float(line.split(":", 1)[1].strip()) * 1_000_000
                        break
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
    cpu_cores, cpu_max_hz = _read_linux_cpu_info()

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
        },
        "disk": {
            "total_bytes": disk_total_bytes,
            "used_bytes": disk_used_bytes,
            "percent": None if disk_percent is None else round(disk_percent, 1),
        },
        "timestamp": int(time.time()),
    }
