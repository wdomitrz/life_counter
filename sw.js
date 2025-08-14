const CACHE = "life_counter-cache";
const FILES = [
  "./",
  "./app.js",
  "./icon.svg",
  "./index.html",
  "./manifest.json",
  "./style.css",
];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(FILES)));
  self.skipWaiting();
});

self.addEventListener("fetch", (event) => {
  event.respondWith(
    caches.match(event.request).then((res) => res || fetch(event.request))
  );
});
