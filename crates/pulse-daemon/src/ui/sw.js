// Service worker mínimo: permite instalar o Pulse como app, sem cache.
// Dados do sistema nunca são guardados offline (passam direto para a rede).
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', (e) => {
  if (e.request.method !== 'GET') return;
  e.respondWith(fetch(e.request));
});
