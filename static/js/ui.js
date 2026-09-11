import { applyTheme, initTheme } from "./theme.js";
import { updateHostStatus, updateServiceStatuses } from "./status.js";
import { updateSystemStats } from "./stats.js";
import { updateHeroGreeting, updateHeroSubtitle } from "./clock.js";

const HOST_STATUS_INTERVAL = 2000;
const SERVICE_STATUS_INTERVAL = 4000;
const SYSTEM_STATS_INTERVAL = 3000;

async function registerServiceWorker() {
    if (!("serviceWorker" in navigator)) return;
    try {
        await navigator.serviceWorker.register("/static/js/sw.js", { scope: "/" });
    } catch (err) {
        console.error("Service worker registration failed:", err);
    }
}

function isPrivateHostname(hostname) {
    if (!hostname) return false;
    if (hostname === "localhost" || hostname.endsWith(".local")) return true;

    const ipv4 = hostname.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
    if (!ipv4) return false;

    const [a, b] = ipv4.slice(1, 3).map(Number);
    return (
        a === 10 ||
        a === 127 ||
        (a === 172 && b >= 16 && b <= 31) ||
        (a === 192 && b === 168)
    );
}

// Prefer the LAN link when the dashboard itself is reached over the local
// network (faster, no internet round-trip), otherwise fall back to the domain link.
function getPreferredServiceUrl(card) {
    const lanUrl = card.getAttribute("data-lan-url");
    if (lanUrl && isPrivateHostname(window.location.hostname)) return lanUrl;
    return card.getAttribute("data-url");
}

// Service cards with both a domain and LAN link expose explicit chip links
// for each method; clicking elsewhere on the card opens the preferred one.
function initServiceCards() {
    document.querySelectorAll(".service-card[data-url]").forEach((card) => {
        card.setAttribute("role", "link");
        card.setAttribute("tabindex", "0");

        const openPreferred = () => {
            const url = getPreferredServiceUrl(card);
            if (url) window.open(url, "_blank", "noopener,noreferrer");
        };

        card.addEventListener("click", (event) => {
            if (event.target.closest(".service-action-btn")) return;
            openPreferred();
        });

        card.addEventListener("keydown", (event) => {
            if (event.target.closest(".service-action-btn")) return;
            if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                openPreferred();
            }
        });
    });
}

// Init
const yearEl = document.getElementById("year");
if (yearEl) yearEl.textContent = new Date().getFullYear();

updateHeroGreeting();
updateHeroSubtitle();
setInterval(() => { updateHeroGreeting(); updateHeroSubtitle(); }, 1000);

initTheme();
applyTheme();
initServiceCards();

const loader = document.getElementById("loader");
if (loader) {
    loader.classList.add("loader-hidden");
    setTimeout(() => loader.remove(), 380);
}
registerServiceWorker();
updateHostStatus();
updateServiceStatuses();
updateSystemStats();

setInterval(updateHostStatus, HOST_STATUS_INTERVAL);
setInterval(updateServiceStatuses, SERVICE_STATUS_INTERVAL);
setInterval(updateSystemStats, SYSTEM_STATS_INTERVAL);
