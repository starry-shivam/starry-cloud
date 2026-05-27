import { applyTheme, initTheme } from "./theme.js";
import { updateHostStatus, updateServiceStatuses } from "./status.js";
import { updateSystemStats } from "./stats.js";
import { updateHeroGreeting, updateHeroSubtitle } from "./clock.js";

const HOST_STATUS_INTERVAL = 6000;
const SERVICE_STATUS_INTERVAL = 4000;
const SYSTEM_STATS_INTERVAL = 5000;

async function registerServiceWorker() {
    if (!("serviceWorker" in navigator)) return;
    try {
        await navigator.serviceWorker.register("/static/js/sw.js", { scope: "/" });
    } catch (err) {
        console.error("Service worker registration failed:", err);
    }
}

// Init
const yearEl = document.getElementById("year");
if (yearEl) yearEl.textContent = new Date().getFullYear();

updateHeroGreeting();
updateHeroSubtitle();
setInterval(() => { updateHeroGreeting(); updateHeroSubtitle(); }, 1000);

const loader = document.getElementById("loader");
if (loader) {
    loader.classList.add("loader-hidden");
    setTimeout(() => loader.remove(), 380);
}

initTheme();
applyTheme();
registerServiceWorker();
updateHostStatus();
updateServiceStatuses();
updateSystemStats();

setInterval(updateHostStatus, HOST_STATUS_INTERVAL);
setInterval(updateServiceStatuses, SERVICE_STATUS_INTERVAL);
setInterval(updateSystemStats, SYSTEM_STATS_INTERVAL);
