export function formatUptime(seconds) {
    const totalSeconds = Math.max(0, Number(seconds) || 0);
    const days = Math.floor(totalSeconds / 86400);
    const hours = Math.floor((totalSeconds % 86400) / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);

    if (days > 0) return `${days}d ${hours}h ${minutes}m`;
    if (hours > 0) return `${hours}h ${minutes}m`;
    return `${minutes}m`;
}

export function formatBytes(bytes) {
    const value = Number(bytes);
    if (!Number.isFinite(value) || value <= 0) return null;
    const gb = value / 1024 ** 3;
    if (gb >= 1) return `${gb.toFixed(1)} GB`;
    const mb = value / 1024 ** 2;
    return `${mb.toFixed(0)} MB`;
}

export async function updateSystemStats() {
    const uptimeEl = document.getElementById("uptimeValue");
    const ramEl = document.getElementById("ramValue");
    const cpuEl = document.getElementById("cpuValue");
    const diskEl = document.getElementById("diskValue");
    const uptimeFooterEl = document.getElementById("uptimeFooter");
    const ramFooterEl = document.getElementById("ramFooter");
    const cpuFooterEl = document.getElementById("cpuFooter");
    const diskFooterEl = document.getElementById("diskFooter");

    if (!uptimeEl || !ramEl || !cpuEl || !diskEl) return;

    try {
        const res = await fetch(`/api/system-stats?t=${Date.now()}`, {
            method: "GET",
            cache: "no-store",
        });
        if (!res.ok) throw new Error("system stats endpoint failed");

        const payload = await res.json();

        // Uptime
        uptimeEl.textContent = formatUptime(payload?.uptime_seconds);
        if (uptimeFooterEl) {
            const upSec = payload?.uptime_seconds || 0;
            const startDate = new Date(Date.now() - upSec * 1000);
            uptimeFooterEl.textContent = startDate.toLocaleString();
        }

        // RAM
        const ramPercent = payload?.memory?.percent;
        const ramUsed = formatBytes(payload?.memory?.used_bytes);
        const ramTotal = formatBytes(payload?.memory?.total_bytes);
        ramEl.textContent =
            typeof ramPercent === "number" ? `${ramPercent.toFixed(1)}%` : "Unavailable";
        if (ramFooterEl) {
            ramFooterEl.textContent = ramUsed && ramTotal ? `${ramUsed} / ${ramTotal}` : "";
        }

        // CPU
        const cpuPercent = payload?.cpu?.percent;
        const cores = payload?.cpu?.cores;
        const maxHz = payload?.cpu?.max_hz;
        const tempCelsius = payload?.cpu?.temperature_celsius;
        cpuEl.textContent =
            typeof cpuPercent === "number" ? `${cpuPercent.toFixed(1)}%` : "Warming up...";
        if (cpuFooterEl) {
            const details = [];
            if (cores) details.push(`${cores} cores`);
            if (typeof maxHz === "number") details.push(`${(maxHz / 1e9).toFixed(2)} GHz`);
            if (typeof tempCelsius === "number") details.push(`${tempCelsius}°C`);
            cpuFooterEl.textContent = details.join(" \u00b7 ");
        }

        // Disk
        const diskPercent = payload?.disk?.percent;
        const diskUsed = formatBytes(payload?.disk?.used_bytes);
        const diskTotal = formatBytes(payload?.disk?.total_bytes);
        diskEl.textContent =
            typeof diskPercent === "number" ? `${diskPercent.toFixed(1)}%` : "Unavailable";
        if (diskFooterEl) {
            diskFooterEl.textContent =
                diskUsed && diskTotal ? `${diskUsed} / ${diskTotal}` : "";
        }
    } catch {
        uptimeEl.textContent = "Unavailable";
        ramEl.textContent = "Unavailable";
        cpuEl.textContent = "Unavailable";
        diskEl.textContent = "Unavailable";
        if (uptimeFooterEl) uptimeFooterEl.textContent = "";
        if (ramFooterEl) ramFooterEl.textContent = "";
        if (cpuFooterEl) cpuFooterEl.textContent = "";
        if (diskFooterEl) diskFooterEl.textContent = "";
    }
}
