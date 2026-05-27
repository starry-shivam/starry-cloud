function setServiceDotState(dot, state) {
    dot.classList.remove("status-online", "status-offline", "status-unknown");
    dot.classList.add(`status-${state}`);
}

export async function updateHostStatus() {
    const dot = document.getElementById("statusDot");
    const pill = dot?.parentElement;

    if (!dot || !pill) return;

    try {
        const res = await fetch("/", { method: "HEAD" });

        if (res.ok) {
            pill.classList.add("status-online");
            pill.classList.remove("status-offline");
        } else {
            throw new Error();
        }
    } catch {
        pill.classList.add("status-offline");
        pill.classList.remove("status-online");
    }
}

export async function updateServiceStatuses() {
    const statusDots = document.querySelectorAll(".service-status[data-service-id]");
    if (!statusDots.length) return;

    try {
        const res = await fetch(`/api/service-status?t=${Date.now()}`, {
            method: "GET",
            cache: "no-store",
        });
        if (!res.ok) throw new Error("status endpoint failed");

        const payload = await res.json();
        const statuses = payload?.statuses || {};

        statusDots.forEach((dot) => {
            const serviceId = dot.getAttribute("data-service-id");
            const isOnline = statuses[String(serviceId)];

            if (isOnline === true) {
                setServiceDotState(dot, "online");
                dot.title = "Online";
            } else if (isOnline === false) {
                setServiceDotState(dot, "offline");
                dot.title = "Offline";
            } else {
                setServiceDotState(dot, "unknown");
                dot.title = "Unknown";
            }
        });
    } catch {
        statusDots.forEach((dot) => {
            setServiceDotState(dot, "unknown");
            dot.title = "Unknown";
        });
    }
}
