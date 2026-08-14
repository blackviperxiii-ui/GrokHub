# Rust cabin — Electron visual parity

**Date:** 2026-08-14  
**Version:** 2.0.0 (do not bump)  
**Override:** Native Rust only. Look like the Electron shell. Do not restore Electron.

## Spec

Steal the Electron chrome, not the renderer.

- Surfaces: `#090909` bg, `#111111` rail, `#171717` panel, `#1f1f1f` elevated, `#f5f5f5` fg.
- Titlebar 40px: GrokHub mark, wordmark, `v2.0.0`, Live/Setup pill, Search, account.
- Left rail 15rem: New chat, Search Ctrl+K, Workspace (Agent / History / Imagine / Workboard / Settings), Tools, Recent.
- Stage header: title + subtitle.
- Chat: right-aligned user bubbles, left assistant bubbles, 12px radius, composer dock with white Send.
- No warm brown. No top-tab strip.

Extra Rust views (Devices, Memory, Eyes, Connectors) sit under Tools so the rail still reads as Electron.
