# QA walkthrough, 2026-11-02

Ship gate for the next `beta` → `main` promotion. `beta` was 484 commits and 845 files ahead of `main` (+112,784 / −9,569 lines).

Base: the walk ran on `beta` @ `e5f50cd6db82c6ceb3281d4abddc5501c798c231`. A parallel walk merged [#1370](https://github.com/blackviperxiii-ui/GrokHub/pull/1370) during this one, and this PR is based on `beta` @ `17beada799dc25ceb072b0b74ed5dcec864763fc`, after it. Findings that #1370 already closed are listed once, under "Closed by #1370".

Severity: **High** means a security hole, data loss, a broken flow, or a stall on a common path. **Medium** means wrong behavior with a workaround. **Low** means minor, or it needs an unusual setup.

Evidence tags:
- **run**: reproduced by running the code (a test that fails on the base, or a browser).
- **read**: the code path was traced by hand.
- **reported**: a reviewer traced it by reading, and it was not re-checked here.

## What was run

| Check | Result |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean |
| `cargo test --workspace --locked` (nextest isn't installable here; this runs the same tests one at a time) | 5,072 passed, 0 failed, 1 ignored (the `speed_bench` timing bench, ignored on purpose since 2026-10-13) |
| `cargo test --doc --workspace` | Clean |
| UI: eslint, `tsc`, vitest, `vite build` | Clean; 590 tests on the base, 597 after #1370 and this PR |
| Web smoke (`ui/e2e/web.mjs`: mocked desktop and Chromium) | Pass |
| Web UI walk: 11 tabs × dark/light × 1280/390 px, screenshots and an overflow check | On the base, the tabs after Connectors could not be tapped at 390 px (#1370 fixed this). After the fix: no overflow and no console errors. |
| Cabin (`target/debug/grokhub`) under Xvfb with a throwaway config | First-run setup (3 steps), the signed-out home, and the offline sign-in error |
| `pii_guard.py` (against `main` and over the full tree), `ci-guards.sh` | Clean |
| `npm audit` (prod and dev) | 0 vulnerabilities |
| Secret-pattern grep over the diff | Only the documented fake test key `sk-abcdefghijklmnopqrstuv` |
| Semgrep | `semgrep.dev` is blocked here, so registry rules can't load. A local ruleset (XSS sinks, `eval`, storage writes, shell spawns, TLS bypass, Actions injection, `pull_request_target`) found nothing exploitable. `retarget.yml` uses `pull_request_target` but checks out no PR code and passes inputs through env. |
| Workflows | Every action is pinned to a SHA. No untrusted `${{ }}` in `run:`. Permissions are least-privilege. |

## Fixed in this PR

Each fix has a test that fails on the base. The fail-before runs below swapped only the fixed code back.

| # | Sev | Where | What was wrong | Evidence |
|---|---|---|---|---|
| 1 | High (perf) | `crates/grokhub-app/src/app/home/updates.rs` | With system checks off, which is the default, a "not allowed" answer reset `os_at` to `None`. Every Home repaint after a read finished then started another `home-updates` thread, and each one loaded and decrypted the consent ledger. Now "not allowed" is asked again after 60 s, and a count holds for 6 h. | run: `a_not_allowed_answer_is_asked_again_in_a_minute_not_on_the_next_repaint` |
| 2 | Medium (data loss) | `crates/grokhub-core/src/mods/install.rs` `recover` | `load()` returns `None` both when `manifest.json` is missing and when it doesn't parse, and `recover()` ran `remove_dir_all` in both cases. A damaged record, or one from a newer Loadout (an unknown `State`), lost `backups/`, the only copies of the game files the mod replaced. Now the folder is kept and a line says so. | run: `an_unreadable_record_keeps_its_backups` |
| 3 | Medium | `ui/src/Chat.tsx` `answer`, `answerCard` | A permission or question card was marked answered before the answer was sent. If `answerPermission` or `answerElicit` failed (a dropped link from the web app, or a desktop error), the card read "Allowed once" with no buttons, while the agent sat parked until Stop. The card now gets its buttons back, and a failed plan approval returns to Plan. | run: `an answer that doesn't reach the agent gives the buttons back` |
| 4 | Medium (security, dev builds) | `src-tauri/src/remote.rs` `page` | In a dev build (`tauri dev`, `cargo run`), Tauri's `AssetResolver::get_for_scheme` reads `frontendDist.join(path)` from disk and drops only the root, so `..` survives. With Web access on, `GET /../../<any file>` on port 4620 from the LAN, with no pairing, returned the file. Release bundles embed their assets and were not affected. A path with an empty, `.` or `..` part, a backslash or a drive now gets the app page. | run: the server test with a dev-like resolver failed on `/../../home/user/.ssh/id_rsa` |
| 5 | Low (hard rule) | `crates/grokhub-app/src/app/oauth.rs`, `nav_palette_settings_tests.rs` | The maintainer's handle appeared as a signed-in account name in a test fixture, and in a doc comment crediting a request to a person. pii-guard's hash list doesn't cover it. Both now use a neutral name. | read |
| 6 | Low (slop) | `ui/tsconfig.tsbuildinfo` | A build artifact committed in #1266, and nothing generates it (`noEmit`, no `incremental`). Deleted; `*.tsbuildinfo` is now ignored. | read |

## Closed by #1370 (found by both walks)

- Phones and the web app: the views bar wrapped into rows that fell below the screen, so Workboard and Settings couldn't be tapped. #1367 caused it.
- Mod archives could write outside staging on Windows through a mid-path drive (`x/D:Games/a.dll`).
- Imagine videos were refused because neither CSP had `media-src`. Confirmed in Chromium.
- Sign in with Grok stopped polling once Settings was hidden.
- History "Delete all" fired on a double-click.

## Not fixed: needs a decision

| Sev | Where | What | Options |
|---|---|---|---|
| Medium | `crates/grokhub-agent/src/client.rs:551-555`, core `chat.rs` `accepts_reasoning_effort` vs `sends_reasoning_effort` | UP-11 (#1369) says "Fast sends grok-4.3 with reasoning effort none". That is true only for Chat Completions. Agent turns use `accepts_reasoning_effort`, which doesn't list grok-4.3, and the client drops `"none"` as well, so Fast and Balance send identical bodies. On routed quick asks the router's own effort also replaces the pinned `none`. The two core lists disagree: one comment says every other model answers 400 to an effort. (read, run of the code path not possible here) | (a) Check against the live xAI Responses API, then merge the two lists and send `none`. (b) If the API refuses it, reword the changelog: Fast is then the same as Balance. |
| Medium | `src-tauri/src/lib.rs` `send_chat`, `stage_attachment` | A paired browser can send to any chat id that has no live worker: the chat reopened at launch, one opened from History, or a cabin thread whose id appears on web Workboard cards. That resumes the full history in the browser and writes into the desktop's session. This breaks "a chat in the window stays in the window". (reported) | Let the server create web chat ids, or refuse ids that already have a session file. Leave `chat` out of the web board view. |
| Design | Web access | Web chats follow the desktop's approval setting. With Always on, anyone holding the pairing link can run commands with no prompt; only the hard classes still ask. (reported) | Force Ask for web chats, or say so where Always is turned on. |
| Low | `ui/src/theme.ts` | In a browser, a pinned theme is written to `localStorage`. The documented rule is "no secret in browser storage", which this keeps. The web smoke's stricter "storage is empty" check passes only because its mock sends no theme. (read) | Allow it and loosen the smoke to "no pairing code", or skip the cache outside the shell. |

## Not fixed: ranked

| Sev | Tag | Where | What | Evidence |
|---|---|---|---|---|
| High | perf | `crates/grokhub-app/src/app/mods_ui.rs` `results` → `jobs::cached_page` → core `mods/client.rs` `cached_search` | With Offline only on, the Mods page reads and deserializes the cache file on the UI thread every frame. For Thunderstore that is the whole package list (allowed up to 96 MB by `LIST_CAP`), re-searched every frame. Online, `jobs::page` memoizes per query. | read |
| Medium | perf | `library_ui/game_art.rs` `game_cover` | Clones the whole cover map from egui temp data, once per tile per frame. The grid isn't virtualized, so the cost grows with the square of the library size. | reported |
| Medium | code | `crates/grokhub-agent/src/route/providers.rs`, `local_http.rs` | Every non-xAI request builder drops the `ServerCompaction` item without a note. After server-side compaction, a turn routed to OpenRouter or the local model sees only the last ≤4k tokens. | reported |
| Medium | code | `crates/grokhub-core/src/mods/install.rs` `update` | Updating a mod overwrites files the user edited, with no backup. The plan is made before the uninstall marks them "kept". | reported |
| Medium | code | `crates/grokhub-app/src/app/board_triage.rs` | A queued card that fails to start (for example, a refresh that fails) makes a new empty chat every 30 s, because the thread is created before the start. `work_resume.rs` has the same order. | reported |
| Medium | privacy | `battlenet_ui.rs`, `gw2_ui.rs`, `destiny_ui.rs` | Disconnecting during a refresh: the in-flight result saves the snapshot again, and Battle.net's `exchange` stores the token. | reported |
| Medium | privacy | `threads.rs` save after Delete all | The shrinking save copies the full history into `threads.json.bak`, and nothing scrubs it. | reported |
| Medium | ui | `ui/src/main.tsx` | No error boundary. A failed lazy chunk (a web tab left open across an update) or a render error blanks the whole UI, including any open approval cards. | reported |
| Low-Med | security | `src-tauri/src/remote_tls.rs` | 64 idle TLS connections, with no pairing code, lock every browser out until web access is toggled off and on. tiny_http sets no read timeout. | reported |
| Low-Med | ui | `ui/src/Automations.tsx:202` | Edit shows in the web app, but `edit_automation` isn't in `WEB_COMMANDS`, so every Save fails. The `remote_call` arm for it is unreachable. | read |
| Low-Med | perf | `ui/src/App.tsx:175-183` | Every streamed text chunk re-renders `App`, and with it Chat and every opened hidden page. | reported |
| Low | perf | `battlenet_ui.rs`, `gw2_ui.rs`, `destiny_ui.rs`, `wow_ui.rs`, `work_log.rs` | Keychain reads, WoW folder scans, combat-log reads and progress-log writes run on the UI thread. | reported |
| Low | security | `crates/grokhub-agent/src/gate.rs:33-35` | `mods_search` and `mods_detail` are in `READONLY`, so they run in plan, read-only and unattended sessions where `web_search` is refused. | reported |
| Low | security | `crates/grokhub-agent/src/harness/hard.rs` `touches_mod_root` | Matches raw path text only, so relative paths, `~`, `$HOME`, escaped spaces and symlinks miss the hard card. | reported |
| Low | security | `crates/grokhub-agent/src/mcp/stdio.rs`, `mcp/http.rs` | The 8 MiB cap is checked after `read_line` has buffered the whole line. This is also on `main`. | reported |
| Low | security | `crates/grokhub-core/src/steam.rs:298-304` | The image host check splits on `/?#` only, so `https://evil.example\x.steamstatic.com/` passes. Needs a malicious Steam API answer. | reported |
| Low | security | `crates/grokhub-core/src/offline.rs` `host_is_local` | `.lan`, `.local`, `.internal` and `.home.arpa` pass Offline only by name, without resolving. | reported |
| Low | security | `crates/grokhub-core/src/mods/archive.rs` `RUNS` | The doc says "no installers or scripts", but `.lnk .url .hta .js .jse .vbe .wsf .reg .cpl .msp` aren't refused. | reported |
| Low | security | `mods_ui.rs:238-243` | `steam://url/…` links from sources skip `url_safe_to_open`. | reported |
| Low | privacy | `gw2_api.rs`, `battlenet_api.rs`, `destiny_api.rs` | Account snapshots are written with default permissions, not `create_private`. | reported |
| Low | code | `work_file.rs` | Stale-lock takeover can let two writers in at once. | reported |
| Low | code | `crates/grokhub-agent/src/session.rs`, `src-tauri/src/settings.rs`, `mods_ui/suggest.rs` | Read-modify-write without a lock, and a fixed `.tmp` name shared by writers. | reported |
| Low | code | `src-tauri/src/remote.rs` | Web chat workers are never cleaned up, and event streams use an unbounded channel. | reported |
| Low | code | `src-tauri/src/oauth.rs`, `pulse_feed.rs` | The loopback test-URL checks accept `http://localhost:1@evil.example`. Exploiting it needs control of the environment. | reported |
| Low | ux | Cabin sign-in, offline | The error shows the raw transport string: `https://auth.x.ai/.well-known/openid-configuration: Dns Failed: resolve dns name 'auth.x.ai:443'…`. | run |
| Low | a11y | `ui/src/Home.tsx:80-84`, `ConnectorAdd.tsx`, `Chat.tsx:960` | `aria-label` hides the Go buttons' contents, a Close button sits inside a `tablist`, and the log is keyed by index. | reported |
| Low | slop | various | Palette chat search is never wired. `mcp::set_enabled`, `tool_list` and `ToolInfo` are used only by tests. `redirect_target` is duplicated and resolves relative paths wrongly. `docs/compass/grokhub-core.md` still says "No HTTP". | reported |

## Verdict

**No-ship** for `beta` @ `e5f50cd`: the phone views bar and the Windows mod zip-slip were blockers. Both are closed by #1370.

**Ship** once this PR is merged, if the maintainer settles the UP-11 effort question (fix it or reword the changelog). The Offline-only Mods stall and the web-chat adoption issue should go in the next batch. Neither one is a regression for anyone who doesn't use those new features.

Not verified:
- Signed-in cabin screens: no credentials here, and the Grok CLI's are off-limits.
- The tauri-driver shell smoke: no tauri-driver here.
- Windows, macOS and phone builds.
- Pages that the web smoke's mock doesn't serve (Home, Agents, Skills, Workboard) were walked only in their error state.
- Findings tagged "reported" were not re-run.
