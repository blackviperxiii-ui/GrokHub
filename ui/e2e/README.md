# UI e2e harnesses

## Shots + color-share (LO-UI-1)

Screenshot and palette-share check for the Tauri shell. Vite serves `ui/` with a stubbed `__TAURI_INTERNALS__` (no real desktop). Playwright writes PNGs; `colorshare` counts red / amber / gray pixels.

```sh
cd ui
npx playwright install chromium
npm run shots          # → e2e/shots/*.png + manifest.json
npm run colorshare     # table; exit 1 when thresholds fail
```

Sizes: 1600×900 and 1100×720. Themes: Ops Dark and OLED Black. Screens: Chat, Home, Pulse, Code, and every Settings sub-tab.

### Thresholds (locked palette, Nov 13 2026)

- red pixels ≥ amber pixels
- amber ≤ 10% of non-black pixels
- with `--baseline <dir>`, gray outline pixels ≤ 50% of that baseline’s gray count

LO-UI-2 / LO-UI-3 are expected to make color-share pass; on today’s amber-accent UI it will exit nonzero — that is the harness working.

```sh
SHOTS_OUT=e2e/shots npm run shots
npm run colorshare -- --dir e2e/shots
npm run colorshare -- --dir e2e/shots --baseline e2e/shots-before
```

Linux CI runs this as the non-required `ui-shots` job (`continue-on-error: true`).

## Other

- `npm run web` — Phase 5 web-app smoke against a mocked desktop
- `npm run smoke -- <binary>` — tauri-driver smoke against a built shell
