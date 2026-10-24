# Cross-platform recode (Tauri 2 + React + TS)

Goal: GrokHub on Windows, macOS, Linux, iOS, Android and the web. The Rust
crates (`grokhub-core`, `grokhub-agent`, `grokhub-hub`) stay and sit behind
Tauri commands. Only the egui UI in `grokhub-app` is rewritten in React. The
egui cabin keeps shipping until the new shell reaches parity, and Jeremy decides
when it is dropped.

Phase 0 started from `beta` @ `f0a5c7a` (2026-10-23).

## Decisions (Jeremy, 2026-10-23)

- **Tauri is approved** for the new cross-platform shell. "No Electron. No
  Tauri." now describes only the egui cabin. Electron stays out.
- **Repo rules win** over the Notion card where they differ. Since #832
  (2026-10-24) those rules are:
  - pstack skills (vstack is retired).
  - Every PR targets `beta`. The agent squash-merges it once CI is green;
    only `beta` → `main` needs Jeremy.
  - `.github/`, `clippy.toml` and release-script changes still need his OK
    before they merge.
- **Still open:** the web backend (`grokhub-hub` as an HTTP service, or the
  web build as a remote control for a desktop instance). It is due before
  Phase 5. AUR and Inno packaging (keep or replace) is due before Phase 6.

## Phases

Each phase is one PR into `beta`, or smaller.

| Phase | Scope | Accept |
|-------|-------|--------|
| 0 | Decisions and baseline (this doc) | Docs-only PR, CI green |
| 1 | Portable core: feature-gate platform code and deps | `cargo check` green for x86_64/aarch64 apple-darwin, aarch64-apple-ios, aarch64-linux-android, and wasm32-unknown-unknown (core and hub-client parts). Existing Linux and Windows tests unchanged and green |
| 2 | Tauri desktop shell (`src-tauri/`, `ui/`) next to egui: chat, settings, tray | Linux, Windows and macOS bundles build. A tauri-driver smoke test sends a chat to a mocked model server and renders the reply |
| 3 | Desktop parity: Pulse, Automations, approvals and consent, memory, MCP, computer-use | Approval-gate tests show that risky actions block without an OK on all three desktop OSes |
| 4 | Android and iOS | Emulator and simulator smoke tests (sign-in mocked, chat round-trip) pass. Capability flags hide desktop-only features |
| 5 | Web, using the chosen backend | `vite build` passes, a Playwright chat round-trip against the mocked backend passes, and no secret is kept in browser storage |
| 6 | Release: `cargo xtask bump` and CHANGELOG artifact lines for the new platforms | A tag dry-run produces every artifact, signed where secrets exist |
| 7 | Rework the open GrokHub Notion cards for the new app | Each card has a decision (rewrite, merge or close) that Jeremy has OK'd before Notion changes |

Desktop-only features (computer-use, screen capture, the indexers, local MCP
stdio servers, D-Bus and systemd) sit behind a per-platform `Capabilities`
report, and the UI hides what a platform lacks. Mobile and web get chat,
memory, automations and HTTP MCP only. Every platform has its own sign-in and
secure storage, and never reads the Grok CLI credentials.

## Phase 1 baseline survey

`cargo check --locked -p <crate> --target <t>` on `f0a5c7a` (Linux host, no
cross C toolchains):

| Target | core / hub / agent | First blocker |
|--------|--------------------|---------------|
| aarch64-apple-darwin | fail | `ring` (via `reqwest` → `rustls`) needs Apple `cc`. This is a host toolchain gap: run it on a macOS runner |
| aarch64-apple-ios | fail | `ring` needs `xcrun`. Needs a macOS runner |
| aarch64-linux-android | fail | `ring` needs `aarch64-linux-android-clang`. Needs the NDK on the runner |
| wasm32-unknown-unknown | fail | `getrandom` 0.2 without the `js` feature. This is a real code gap |

Native-target failures say nothing about the Rust code yet, so the next check
belongs on the right runners. Wasm needs a real split: `rusqlite` (bundled C),
`reqwest`/`ureq` blocking I/O, `tiny_http`, `keyring` and the filesystem cannot
run in `wasm32-unknown-unknown`. The web client probably needs a small
types-and-protocol crate with no I/O, which both `grokhub-hub` and the web build
use.

`target_os` cfgs: 113 across the workspace. Most are in
`grokhub-app/src/desktop_mcp/` (wayland, mod, apps) and stay desktop-only.
In `grokhub-agent` they sit in `sudo_pass.rs`, `tools/shell.rs`,
`indexers/{power,paths}.rs`, `harness/{backend,at_rest}.rs`,
`route/local_perf.rs` and `brain/playbook.rs`.

## Phase 1 status

- `crates/grokhub-proto` holds the hub wire types (`hub_sync`, `inhabit`,
  `HUB_KIND`) with no I/O and builds for `wasm32-unknown-unknown`.
  `grokhub-core` re-exports them at their old paths.
- With the C toolchains in place, the Rust code in `grokhub-core`,
  `grokhub-hub` and `grokhub-agent` already passes clippy for both macOS
  targets, `aarch64-apple-ios` and `aarch64-linux-android`. The one fix was for
  iOS: `apple-native-keyring-store` needs its `protected` feature there.
- CI job `cross-check` runs that clippy matrix on macOS and Ubuntu (NDK)
  runners.
- Known for Phase 4: keyring's `v1` feature has no Android store. On Android,
  `keyring::Entry::new` will fail until the app enables
  `android-native-keyring-store`.


## Phase 2 status

The shell lives in `src-tauri/` (Rust, package `grokhub-shell`) and `ui/`
(Vite + React + TS). It is its own Cargo workspace with its own
`src-tauri/Cargo.lock`, so the main workspace's jobs never need the webview
system libraries. `cargo xtask bump` keeps the grokhub-* versions in that
lockfile in step, and `bump --check` fails if they drift.

- Chat streams one reply per turn through `grokhub_agent::XaiClient` and shows
  each text piece as it arrives. Stop keeps the partial reply. (Phase 3a
  replaced this with the agent loop, below.)
- Settings: model and close-to-tray in `shell.json` in the GrokHub config
  folder. The xAI API key lives in the OS keychain (service `GrokHub`, account
  `shell-console-key`). Grok sign-in (OAuth) comes with Phase 3.
- Tray: Show GrokHub and Quit. A left click shows the window.
- `GROKHUB_SHELL_RESPONSES_URL` points chat at a local mock model. Only
  plain-http loopback URLs are accepted, so it cannot move real traffic.
- Tests: `npm test` in `ui/` (vitest), `cargo test` and clippy in
  `src-tauri/`, and `node ui/e2e/smoke.mjs <binary>`. The smoke test drives the
  built shell through tauri-driver, sends a chat to a mock model and waits for
  the reply. tauri-driver has no macOS backend, so macOS only builds.

Build locally (Linux needs `libwebkit2gtk-4.1-dev`,
`libayatana-appindicator3-dev` and `librsvg2-dev`):

```sh
npm ci --prefix ui
ui/node_modules/.bin/tauri build            # bundles in src-tauri/target/release/bundle
ui/node_modules/.bin/tauri build --debug --no-bundle
cargo install tauri-driver --locked
xvfb-run -a node ui/e2e/smoke.mjs src-tauri/target/debug/grokhub-shell
```

## Phase 3 status

Phase 3 lands in slices, each its own PR into `beta`.

**3a, agent loop with approvals.** Chat runs `grokhub_agent::NativeEngine`,
the loop the egui cabin uses, on one worker thread per conversation
(`src-tauri/src/agent.rs`).

- The gate is the cabin's attended Ask mode with computer-use off: reads run,
  while writes, commands and the other mutating tools emit a permission event and
  park until the UI answers. The answers are Allow once, Always allow (saved
  as a workspace rule, as in the cabin) or Deny. Stop cancels a parked tool
  without running it.
- Events reach the UI as `chat://event` (`text`, `tool`, `permission`,
  `error`, `done`). Tool calls show as cards.
- Settings has a work folder. When it is empty, the agent uses the cabin's
  default session folder.
- Gate tests in `src-tauri` run on Linux, Windows and macOS in the `shell` job.
  A write and a shell command each wait for an answer. Deny leaves no file.
  Allow writes it. Stop runs nothing. A read never asks. The tauri-driver smoke
  test does the same through the built app: nothing is written and the model
  is not called again until Allow once is clicked.

Next slices: Pulse, Automations, memory, MCP, computer-use, and the Auto
and Always permission modes.

