const cacheName = 'pocketcam-offline-v2';
const offlinePage = '/offline.html';
const offlineAssets = ['/style.css', '/icons/pocketcam-192.png'];

self.addEventListener('install', (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(cacheName);
    await cache.addAll([offlinePage, ...offlineAssets]);
    await self.skipWaiting();
  })());
});

self.addEventListener('activate', (event) => {
  event.waitUntil((async () => {
    const entries = await caches.keys();
    await Promise.all(entries
      .filter((entry) => entry.startsWith('pocketcam-offline-') && entry !== cacheName)
      .map((entry) => caches.delete(entry)));
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  const fallback = request.mode === 'navigate' && url.pathname === '/'
    ? offlinePage
    : offlineAssets.find((asset) => url.pathname === asset);
  if (!fallback) return;
  event.respondWith(fetch(request).catch(async (error) => {
    const cached = await (await caches.open(cacheName)).match(fallback);
    if (cached) return cached;
    throw error;
  }));
});
