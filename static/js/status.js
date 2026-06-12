function setServiceDotState(dot, state) {
    dot.classList.remove("status-online", "status-offline", "status-unknown");
    dot.classList.add(`status-${state}`);
}

const consecutiveFailures = new Map();
let latestServiceStatusRequestId = 0;
const OFFLINE_FAILURE_THRESHOLD = 2;

function applyServiceStatus(dot, state) {
    setServiceDotState(dot, state);

    dot.title =
        state === "online"
            ? "Online"
            : state === "offline"
              ? "Offline"
              : "Unknown";

    return state;
}

async function readStatusStream(res, onStatus) {
    const decoder = new TextDecoder();
    const reader = res.body?.getReader();

    if (!reader) {
        const text = await res.text();
        for (const line of text.split(/\r?\n/)) {
            if (!line.trim()) continue;
            onStatus(JSON.parse(line));
        }
        return;
    }

    let buffer = "";
    while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });

        let newlineIndex = buffer.indexOf("\n");
        while (newlineIndex !== -1) {
            const line = buffer.slice(0, newlineIndex).trim();
            buffer = buffer.slice(newlineIndex + 1);
            if (line) {
                onStatus(JSON.parse(line));
            }
            newlineIndex = buffer.indexOf("\n");
        }
    }

    const tail = buffer.trim();
    if (tail) {
        onStatus(JSON.parse(tail));
    }
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

    const requestId = ++latestServiceStatusRequestId;
    const statusDotsById = new Map(
        [...statusDots].map((dot) => [dot.getAttribute("data-service-id"), dot])
    );

    try {
        const res = await fetch(`/api/service-status?stream=1&t=${Date.now()}`, {
            method: "GET",
            cache: "no-store",
        });
        if (!res.ok) throw new Error("status endpoint failed");

        await readStatusStream(res, (status) => {
            if (requestId !== latestServiceStatusRequestId) return;

            const serviceId = String(status?.id);
            const isOnline = status?.online;
            const dot = statusDotsById.get(serviceId);

            if (!dot) return;

            if (isOnline === true) {
                consecutiveFailures.set(serviceId, 0);
                applyServiceStatus(dot, "online");
            } else if (isOnline === false) {
                const failureCount = (consecutiveFailures.get(serviceId) || 0) + 1;
                consecutiveFailures.set(serviceId, failureCount);

                if (failureCount >= OFFLINE_FAILURE_THRESHOLD) {
                    applyServiceStatus(dot, "offline");
                } else {
                    applyServiceStatus(dot, "unknown");
                }
            } else {
                applyServiceStatus(dot, "unknown");
            }
        });
    } catch (err) {
        console.error("Failed to refresh service statuses:", err);

        statusDots.forEach((dot) => {
            applyServiceStatus(dot, "unknown");
        });
    }
}
