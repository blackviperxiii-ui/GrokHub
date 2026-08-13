/**
 * Content-Security-Policy for the packaged Electron session.
 * Vite :8080 is skipped in main.mjs (needs eval). Production Nitro HTML
 * still has inline TanStack hydration scripts — 'self' alone blanks the window.
 */
function packagedSessionCsp() {
  return [
    "default-src 'self'",
    "script-src 'self' 'unsafe-inline'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob: https:",
    "media-src 'self' blob: data:",
    "font-src 'self' data:",
    "connect-src 'self' http: https: ws: wss:",
    "worker-src 'self' blob:",
    "frame-src 'none'",
  ].join("; ");
}

module.exports = { packagedSessionCsp };
