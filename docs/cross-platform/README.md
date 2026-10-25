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
- **Web backend (Viper, 2026-10-25):** the web app is a remote for GrokHub
  running on your own computer, not `grokhub-hub` as a hosted service. The
  desktop runs the chats and keeps the key.
- **Still open:** AUR and Inno packaging (keep or replace), due before Phase 6.

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
`grokhub-desktop` (wayland, lib, apps) and stay desktop-only.
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

**3b, Automations.** An Automations page edits the cabin's own
`automations.json` (`src-tauri/src/automations.rs`), so a job made in either
app shows in both.

- Add takes plain text and uses the cabin's parser
  (`parse_nl_automation`): "every weekday at 9am …", "every day at 7am …" or
  "heartbeat every 30 min …". Pause, Resume and Remove work on one row and
  keep the rest. A file the shell cannot read is an error and is never
  overwritten.
- The UI sends its local weekday, hour and minute, so next-run times use the
  user's zone on every platform without a time-zone crate.
- Run now runs the job with `run_unattended` in Ask mode on a read-only
  session, as the cabin's scheduled Ask jobs do, so an allow rule cannot let a
  write through. It can read, every tool that needs an OK is refused, and the
  run is recorded on the row (run count, health). One job runs once at a time,
  and the page stays mounted so a run in flight is not lost on a tab change.
- Edits take a lock and reload the file first, and a cabin row with no id gets
  one written back, so Pause or Remove always finds the row it showed.
- The cabin's pulse still fires jobs on schedule. The shell runs no second
  scheduler, so a job never fires twice. Firing from the shell waits until
  the shell replaces the cabin.
- Tests: Rust tests for add, refusing text with no time, pause, resume,
  remove, a broken file, rows the cabin wrote, and a Run now whose write is
  refused. vitest covers the page. The smoke test adds a job through the UI,
  checks `automations.json`, runs it and waits for the reply.

**3c, Memory.** A Memory page shows and edits the same `MEMORY.md` files the
cabin and the agent loop read (`src-tauri/src/memory.rs`): global notes in the
config folder's `memory/MEMORY.md`, project notes in the work folder's
`MEMORY.md`.

- Remember adds one line the way `/remember` does; Global files it in the
  global notes. Each file can also be edited and saved whole.
  A save sends the text the editor started from; if the file changed since
  (chat remembered something), nothing is written and the page reloads.
- Secrets are redacted before anything is written, and the page says when that
  happened. Files over 1 MB or not plain text are refused.
- Search runs `grokhub_agent::search_memory`, the same index and scope recall
  uses: this work folder's notes and the global notes, never another folder's.
- When the cabin's AMR store is on, the page says recall does not read these
  files.
- Tests: Rust tests for add, edit with a redacted secret, a blank note, and a
  search that keeps other work folders out. vitest covers the page. The smoke
  test remembers a line through the UI, checks `MEMORY.md`, and finds it with
  search.

**3d, approval modes.** Settings has an Approvals choice, saved as
`approvals` in `shell.json`: Ask every time (the default), Auto, or Always
approve, the cabin's three permission modes.

- Chat builds its gate from it on every prompt (`shell_gate(mode)`), so a
  change applies to the next message in an open chat.
- Auto uses the cabin's judge (`auto_review`): routine steps run, the rest
  ask, and a judge failure asks.
- Always runs writes and commands without a card, but hard-class steps
  (delete, send, money, credentials, irreversible OS changes) still park for
  an OK in every mode. A test runs `rm notes.txt` under Always and checks it
  waits and the file survives a Deny.
- An unknown value in `shell.json` reads as Ask. Automations' Run now stays
  read-only whatever the mode.

**3e, Connectors.** A Connectors page lists the MCP servers in the same
`mcp.json` the cabin and chat use (`src-tauri/src/connectors.rs` over
`grokhub_agent::mcp`).

- Opening the page reads the entries without starting any. Check all starts
  each enabled server and pings it, off the main thread, and shows connected
  with its tool count or the error.
- Reconnect restarts one server. Sign out forgets a server's sign-in.
- Disconnect asks "Disconnect {name}?" on the row first. Only a yes takes the
  entry out of `mcp.json`, through the same change ledger the cabin uses, and
  deletes its sign-in and saved token. Computer-use servers can't be
  disconnected here.
- Not yet: signing in to an HTTP server and adding new connectors. The shell
  has no browser opener, so the page points to the cabin for both.
- Rust tests run a real stdio server (`ui/e2e/fake-mcp.mjs` under node) and a
  missing command. The smoke test writes an entry for that server, checks it
  connects with one tool, and checks the entry leaves `mcp.json` only after
  the question is answered.

**3f, Pulse.** A Pulse page shows the cabin's Feed and Ideas from the same
`updates.json`, ranked by the same rules (`grokhub_core::pulse`), with the
MEMORY.md ledger as input (`src-tauri/src/pulse.rs`).

- Feed opens first: source, time, headline, summary, the source link as
  text (the shell has no browser opener yet), and Like, Snooze, Not this,
  Dismiss.
- Ideas groups rows under the cabin's category headers. Run notes the idea
  as accepted and puts its action in the chat box; nothing is sent until the
  person sends it, so the chat's approvals still apply.
- Dismiss, Not this, Like and Accept write the cabin's one-line ledger entry
  to the global MEMORY.md, so the next ranking learns from them. Snooze hides
  a card until the next 9 AM local.
- Not yet: generating new ideas, Feed instructions, Undo on done-for-you
  posts, and source images. The cabin still makes the cards. A dismissed
  idea is not added to the cabin's turned-down list (that lives in its
  `config.json`, which the shell does not write), so the cabin's next idea
  pass can offer a reworded copy.
- Rust tests cover the split, each act, the ledger lines and snooze. The
  smoke test writes a post and an idea, checks Not this writes the ledger
  line, and checks Run fills the chat box without a model call.

**3g, computer-use.** The desktop tools moved into a shared
`crates/grokhub-desktop` crate (#859). The shell's chat now uses them through
the same hard gate as the cabin (`src-tauri/src/agent.rs`).

- Settings → Let Grok control the desktop is off by default. While it's off,
  the desktop tools aren't offered, and a call is refused with the cabin's
  off message. Turning it on also adds the cabin's desktop line to the
  system prompt.
- Risky steps (delete, send, money, credentials) wait for an OK in every
  approvals mode, Always included. Ordinary steps follow the approvals
  setting. A halt stamp (the cabin's Halt) stops a running prompt.
- The shell is the crate's host: its config folder, its switch, and on Linux
  the lock-screen and click-target readers in `grokhub-desktop/src/screen.rs`
  (the AT-SPI walk now lives in `grokhub_core::ATSPI_PY` for both apps).
- Not yet: desktop episodes (the cabin's observe, act, verify loop), the
  Cua driver, and a Halt button in the shell. macOS has no desktop backend
  yet, so desktop calls fail there with the no-display error.
- Rust tests cover the switch, a refused call while it's off, a typed delete
  that waits in Always and is denied, and an allowed step that runs. The
  smoke test turns the switch on with approvals at Always, has the model
  type a delete, and checks that the card waits, nothing runs, and Deny
  reaches the model as a refusal.


Phase 3 met its accept line: risky desktop steps wait for an OK in the Rust
gate tests on all three desktop OSes, and the smoke test confirms it on
Linux and Windows. tauri-driver has no macOS backend, so the smoke test
doesn't run there.

## Phase 4 status

**4a, capability flags.** `src-tauri/src/capabilities.rs` reports what the
running OS supports (`get_capabilities`). Linux, Windows and macOS get
everything. Any other OS (Android, iOS) gets chat, memory, automations and
connectors only.

- The UI hides what the report leaves off: the Pulse tab, Settings → Local
  AI, Let Grok control the desktop, and the tray setting. Until the report
  loads, or if it fails, only the core features show.
- The backend refuses those features too: the Pulse and Local AI commands
  answer "… isn't available on this device.", and the desktop and tray
  switches are saved as off and never reach the agent.

**4b, phone builds.** The shell builds for Android and iOS.

- `run()` is the mobile entry point. The tray, the window restore and
  computer-use are `cfg(desktop)`, and phones never build `grokhub-desktop`.
  A phone app has no usable home folder (Android sets none), so at startup
  `HOME` and `GROKHUB_CONFIG` point into the app's data folder.
- Smoke build: `VITE_PHONE_SMOKE=1` and `--features phone-smoke`, which points
  chat at the loopback mock (`PHONE_SMOKE_URL`, a constant because Xcode's
  build drops the job's environment). WebDriver can't reach a
  phone's webview, so the app drives itself (`ui/src/phoneSmoke.ts`). It
  checks that the report hides the desktop features and Pulse isn't shown,
  sends one chat, waits for the mock's reply, and writes `phone-smoke.txt`
  in its data folder. `ui/e2e/phone.mjs` serves the mock, installs and
  launches the app, and reads the report back with `adb run-as` or
  `simctl get_app_container`. Normal builds leave the driver out, and their
  `phone_smoke_report` command refuses.
- CI jobs `phone (android)` (an API 34 x86_64 emulator) and `phone (ios)` (a
  booted iPhone simulator) run this on every PR.
- Not yet: saving an API key on Android, because keyring has no Android store
  until the app enables `android-native-keyring-store`. Sign-in on phones is
  only mocked so far.

## Phase 5 status

The web app is the same `ui/` build, opened in a browser as a remote for the
desktop shell. The desktop runs the chats, the approvals and the keychain, and
the browser holds no key.

**5a, the web client.**

- `ui/src/transport.ts` is the one seam. Inside the shell it calls Tauri's
  `invoke` and `listen`. In a browser it sends the same commands to the
  paired desktop over HTTP. `api.ts` goes through it, so the pages don't
  change.
- The browser first shows the pairing screen (`ui/src/Pair.tsx`). A pairing
  link `…/#code=<code>` pairs without typing, and the code is taken off the
  address at once.
- The code lives in memory only, never in localStorage, sessionStorage,
  IndexedDB or a cookie. Requests use `credentials: "omit"`, and a reload
  asks to pair again.
- The web app gets chat (with tool approvals), automations, memory and
  connectors (`WEB` in `api.ts`). Settings shows the desktop's model, folder,
  approvals and whether a key is saved, read-only. Settings and the key are
  changed on the desktop.
- `ui/e2e/web.mjs` is the smoke test. A mocked desktop serves `ui/dist` and
  the API on one origin. Chromium opens the pairing link, sends a chat,
  allows the tool the mock asks for, and gets the reply. It then checks the
  browser stores are empty and that a reload asks to pair again. The shell
  CI job runs it on Linux.

**Remote protocol v1.** The desktop serves the built UI and this API on one
origin, so there's no CORS.

| Request | Auth | Reply |
|---------|------|-------|
| `GET /remote/v1/hello` | none | `{"kind": "grokhub-remote", "version": 1}` |
| `POST /remote/v1/call/<command>`, a JSON body of the command's arguments (the same names as `invoke`) | `Authorization: Bearer <code>` | `{"ok": true, "value": …}` or `{"ok": false, "error": "<message>"}`; 401 for a wrong code |
| `GET /remote/v1/events` | `Authorization: Bearer <code>` | `text/event-stream`; each frame is `data: {"event": "chat://event", "payload": …}` |

The desktop answers only the web app's commands: chat (`send_chat`,
`answer_permission`, `cancel_chat`, `close_chat`), `get_settings`,
automations, memory and connectors. Everything else gets `ok: false`: the
key, settings changes, Pulse, Local AI and desktop control.

**5b, the desktop side (next).** The shell gets Settings → Web access. That
switch serves `ui/dist` and the API above on the LAN, behind a random pairing
code and the command allowlist, and shows the pairing link.
