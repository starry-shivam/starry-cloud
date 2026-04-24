(function() {
    const THEME_KEY = "theme";
    const STATUS_INTERVAL = 5000;
    const THEME_COLORS = {
        light: "#ffffff",
        dark: "#020617"
    };

    const root = document.documentElement;
    const icon = document.getElementById("themeIcon");
    const toggleBtn = document.getElementById("themeToggle");

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    function getSystemTheme() {
        return mediaQuery.matches ? "dark" : "light";
    }

    function setAutoIcon() {
        if (!icon) return;

        icon.innerHTML = `
          <!-- Outer circle -->
          <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="2"></circle>

          <!-- Left half filled -->
          <path d="
              M12 3
              A9 9 0 0 0 12 21
            Z
          " fill="currentColor"></path>
       `;
    }

    function setIcon(theme, mode) {
        if (!icon) return;

        // Auto mode indicator
        if (mode === "auto") {
            setAutoIcon();
            return;
        }

        if (theme === "dark") {
            // Moon
            icon.innerHTML = `
                <path d="M21 12.79A9 9 0 1111.21 3
                         7 7 0 0021 12.79z"></path>
            `;
        } else {
            // Sun
            icon.innerHTML = `
                <circle cx="12" cy="12" r="5"></circle>
                <line x1="12" y1="1" x2="12" y2="3"></line>
                <line x1="12" y1="21" x2="12" y2="23"></line>
                <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"></line>
                <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"></line>
                <line x1="1" y1="12" x2="3" y2="12"></line>
                <line x1="21" y1="12" x2="23" y2="12"></line>
                <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"></line>
                <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"></line>
            `;
        }
    }

    function syncBrowserChrome(theme) {
        const color = THEME_COLORS[theme] || THEME_COLORS.light;

        const themeColorMeta = document.querySelector('meta[name="theme-color"]');
        if (themeColorMeta) {
            themeColorMeta.setAttribute("content", color);
        }

        const appleStatusBarMeta = document.querySelector(
            'meta[name="apple-mobile-web-app-status-bar-style"]'
        );
        if (appleStatusBarMeta) {
            appleStatusBarMeta.setAttribute(
                "content",
                theme === "dark" ? "black-translucent" : "default"
            );
        }
    }

    function applyTheme() {
        const saved = localStorage.getItem(THEME_KEY) || "auto";

        const themeToApply =
            saved === "auto" ? getSystemTheme() : saved;

        root.setAttribute("data-theme", themeToApply);
        setIcon(themeToApply, saved);
        syncBrowserChrome(themeToApply);

        // Optional: tooltip label
        if (toggleBtn) {
            toggleBtn.title = `Theme: ${saved.charAt(0).toUpperCase() + saved.slice(1)}`;
        }
    }

    function toggleTheme() {
        const current = localStorage.getItem(THEME_KEY) || "auto";
        let next;

        if (current === "auto") next = "light";
        else if (current === "light") next = "dark";
        else next = "auto";

        localStorage.setItem(THEME_KEY, next);
        applyTheme();
    }

    function registerServiceWorker() {
        if (!("serviceWorker" in navigator)) return;

        try {
            await navigator.serviceWorker.register("/static/js/sw.js", {
                scope: "/"
            });
        } catch (err) {
            console.error("Service worker registration failed:", err);
        }
    }


    toggleBtn?.addEventListener("click", toggleTheme);
    mediaQuery.addEventListener("change", () => {
        const saved = localStorage.getItem(THEME_KEY) || "auto";
        if (saved === "auto") {
            applyTheme();
        }
    });

    async function updateStatus() {
        const dot = document.getElementById("statusDot");
        const pill = dot?.parentElement;

        if (!dot || !pill) return;

        try {
            const res = await fetch("/", {
                method: "HEAD"
            });

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

    // init
    const yearEl = document.getElementById("year");
    if (yearEl) {
        yearEl.textContent = new Date().getFullYear();
    }

    // Hide page loader
    const loader = document.getElementById("loader");
    if (loader) {
        loader.classList.add("loader-hidden");
        setTimeout(() => loader.remove(), 380);
    }

    applyTheme();
    registerServiceWorker();
    updateStatus();

    setInterval(() => {
        updateStatus();
    }, STATUS_INTERVAL);


})();