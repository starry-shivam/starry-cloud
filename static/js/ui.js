(function() {
    const THEME_KEY = "theme";
    const STATUS_ENDPOINT = "/";
    const STATUS_INTERVAL = 5000;

    const root = document.documentElement;
    const icon = document.getElementById("themeIcon");

    function setIcon(theme) {
        if (!icon) return;

        if (theme === "dark") {
            // Moon
            icon.innerHTML = `
      <path d="M21 12.79A9 9 0 1111.21 3
               7 7 0 0021 12.79z" />
    `;
        } else {
            // Sun
            icon.innerHTML = `
      <circle cx="12" cy="12" r="5" />
      <line x1="12" y1="1" x2="12" y2="3" />
      <line x1="12" y1="21" x2="12" y2="23" />
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
      <line x1="1" y1="12" x2="3" y2="12" />
      <line x1="21" y1="12" x2="23" y2="12" />
      <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
      <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
    `;
        }
    }


    function applyTheme() {
        const saved = localStorage.getItem(THEME_KEY);
        if (saved) root.setAttribute("data-theme", saved);
        setIcon(saved || "light");
    }

    function toggleTheme() {
        const current = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
        root.setAttribute("data-theme", current);
        localStorage.setItem(THEME_KEY, current);
        setIcon(current);
    }

    document.getElementById("themeToggle")?.addEventListener("click", toggleTheme);

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

    document.getElementById("year").textContent = new Date().getFullYear();

    applyTheme();
    updateStatus();
    setInterval(updateStatus, STATUS_INTERVAL);
})();