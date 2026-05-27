
const CACHE_NAME = "starry-cloud-v4";
const ASSETS = [
  "/static/style.css",
  "/static/js/ui.js",
  "/static/js/theme.js",
  "/static/js/status.js",
  "/static/js/stats.js",
  "/static/js/clock.js",
  "/static/manifest.webmanifest",
  "/static/dragon.png"
];


self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(ASSETS))
  );
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(
        keys
          .filter((key) => key !== CACHE_NAME)
          .map((key) => caches.delete(key))
      )
    )
  );
  self.clients.claim();
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  const url = new URL(req.url);

  if (url.pathname.startsWith("/api/")) {
    event.respondWith(fetch(req));
    return;
  }

  if (req.mode === "navigate") {
    event.respondWith(fetch(req));
  } else {
    const jsModules = [
      "/static/js/ui.js",
      "/static/js/theme.js",
      "/static/js/status.js",
      "/static/js/stats.js",
      "/static/js/clock.js"
    ];
    const isLiveAsset = jsModules.includes(url.pathname) || url.pathname === "/static/style.css";

    if (isLiveAsset) {
      event.respondWith(
        fetch(req)
          .then((res) => {
            const copy = res.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(req, copy));
            return res;
          })
          .catch(() => caches.match(req))
      );
      return;
    }

    event.respondWith(
      caches.match(req).then((cached) => cached || fetch(req))
    );
  }
});
