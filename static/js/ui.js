(function () {
  const THEME_KEY = "theme";
  const STATUS_CHECK_INTERVAL = 5000; // ms
  const STATUS_ENDPOINT = "/";

  /* ---------------------------
   * Theme handling
   * ------------------------- */

  function applySavedTheme(root) {
    const saved = localStorage.getItem(THEME_KEY);

    if (saved === "light" || saved === "dark") {
      root.setAttribute("data-theme", saved);
    } else {
      // follow system preference
      root.removeAttribute("data-theme");
    }
  }

  function toggleTheme(root) {
    const current = root.getAttribute("data-theme");

    const newTheme =
      current === "dark" ? "light" :
      current === "light" ? "dark" :
      matchMedia("(prefers-color-scheme: dark)").matches ? "light" : "dark";

    root.setAttribute("data-theme", newTheme);
    localStorage.setItem(THEME_KEY, newTheme);
  }

  function initTheme() {
    const root = document.documentElement;
    applySavedTheme(root);

    const btn = document.getElementById("themeToggle");
    if (!btn) return;

    btn.addEventListener("click", () => toggleTheme(root));
  }

  // Set current year in footer
  document.getElementById("year").textContent = new Date().getFullYear();

  /* ---------------------------
   * Service worker
   * ------------------------- */

  function registerServiceWorker() {
    if (!("serviceWorker" in navigator)) return;

    navigator.serviceWorker
      .register("/static/js/sw.js")
      .catch(err => {
        console.error("Service worker registration failed:", err);
      });
  }

  /* ---------------------------
   * Status dot / host check
   * ------------------------- */

  async function updateStatusDot() {
    const dot = document.getElementById("statusDot");
    if (!dot) return; // nothing to update

    try {
      const res = await fetch(STATUS_ENDPOINT, { method: "HEAD" });

      if (res.ok) {
        dot.style.background = "#22c55e"; // green
        dot.style.boxShadow = "0 0 0 4px rgba(34, 197, 94, 0.25)";
      } else {
        throw new Error("Not OK");
      }
    } catch (e) {
      dot.style.background = "#ef4444"; // red
      dot.style.boxShadow = "0 0 0 4px rgba(239, 68, 68, 0.25)";
    }
  }

  function startStatusPolling() {
    updateStatusDot(); // initial
    setInterval(updateStatusDot, STATUS_CHECK_INTERVAL);
  }

  /* ---------------------------
   * Bootstrapping
   * ------------------------- */

  // DOM-dependent init
  document.addEventListener("DOMContentLoaded", () => {
    initTheme();
    startStatusPolling();
  });

  // SW should wait until full load
  window.addEventListener("load", registerServiceWorker);
})();
