# Improvements and optimization sweep, 2026-11-02

This sweep followed the QA walkthrough in `qa-walkthrough-2026-11-02.md`. Six finders looked for wins in six areas:
- cabin runtime
- shell UI and bundle
- agent and core hot paths
- startup
- build, CI, tests and repo size
- code quality

For each area, one adversarial verifier then re-derived every claim, and a planner ranked the results. Each change that shipped was reviewed again before merge, by one reviewer for correctness and one for whether the claimed gains hold. For #1374 that review raised 12 findings and confirmed 8, all fixed before merge.

Base: `beta` @ `901ccdd0`. Numbers come from this machine (4 CPUs, Linux, debug builds unless noted) or from CI logs.

## Baselines

| What | Measured |
|---|---|
| Clean debug build of `grokhub` and `grokhub-hub` | 184 s wall, 519 units, 568 s of CPU in total. The critical path runs serially: `grokhub-core` 40 s → `grokhub-agent` 26 s → `grokhub-app` 33 s. |
| Slowest dependency units | naga 17 s, zbus 17 s, x11rb-protocol 15 s, ash 15 s, wgpu-core 13 s, egui 12 s |
| Dependencies built twice | 20 crates (getrandom ×3; thiserror, syn, serde, hashbrown, rustix, …) |
| CI wall time on a PR | shell 12.5 min, windows 8.8 min, phone (android) 7.8 min, linux 5.7 min |
| UI tests (`npm test`) | about 40 s; jsdom is built 59 times, about 42% of the run |
| Repo | 249 MB packed. 87% of the tracked bytes are `docs/design` PNGs (226 MB), which only the docs and the Windows deck-shots scripts read. |
| Cabin idle CPU (first run, Xvfb, software GL) | 0.4–0.7% of one core |
| Cabin startup, fresh profile | `Cabin::new` 41 ms, of which 16 ms is a `date` process and about 19 ms is `hostname` processes. First frame 280–350 ms. |
| Settings open at launch | 4.0 s to the first window in debug. A gdb sample put most of it in Nyx thumbnail decoding on the UI thread. |

## Shipped

| PR | What | Measured gain |
|---|---|---|
| [#1373](https://github.com/blackviperxiii-ui/GrokHub/pull/1373) | `read_file` sends an image once; streamed UTF-8 stays whole; the reply guard scans linearly; one write and one sync per turn; no remove-before-rename | See below |
| [#1374](https://github.com/blackviperxiii-ui/GrokHub/pull/1374) | Cabin frame rate and startup; shell re-render storms; test and CI time | See below |
| this PR | Voice pictures decode on one worker; the measured Windows test time; a flaky test | See below |

**#1373, agent hot paths:**
- **Images:** each 300 KiB image read stops adding about 102k estimated tokens (409,622 bytes of base64 text) to every later request.
- **Reply guard:** a 31.5 KB reply in 4-character deltas takes 1.07 ms, against 1.23 s before (release build).
- **Session writes:** a 10-tool turn goes from 23 or more fsyncs to 1.
- **Intentions file:** a reader no longer finds it missing mid-save. Before, it was missing 1,314 times in 500 rewrites.

**#1374 and this PR, the cabin:**
- **Frame rate during a turn:** a running turn asks for frames about 30 times a second, not every frame. The fix adds egui's predicted frame time back to the delay. Without it, a 33 ms request ran at about 50 fps, and that applied to the existing run dot and glow too.
- **Startup, fresh profile (3 runs each):** `Cabin::new` goes from 40.8–42.4 ms to 6.5–7.7 ms. Its config step goes from 20–23 ms to 3–4.5 ms. The first frame goes from 282–349 ms to 209–241 ms.
- **Settings at launch:** 4.0–4.5 s → 1.5–2.0 s to the first window. The pictures decode on one worker. One thread per tile had cost 22–27 MB more heap (RssAnon) with Settings open; one worker costs 3 MB.
- **Ranking and folding:** the outcome fold is linear, and Pulse ranking tokenizes each title once per frame.
- **Turn streams:** host output and `grok -p` replies are read in full each frame. Before, they drained one message a frame, so at 30 fps a long host log held up the end of the job.

**#1374, the shell:**
- **Streamed text:** 100 chunks caused 100 `App` renders, now at most 2.
- **Phone scrolling:** a 60-step swipe caused 59 renders, now at most 2.
- **Settings:** `get_settings` reads the keychain on the blocking pool, off the main thread. A first version ran it as an async command; on Linux the Secret Service client then panicked on the tokio worker. Both the review and the CI shell smoke caught it before merge.

**#1374, tests and CI:**
- **Deny-rule tests:** the three `gb_` tests take 48 s → 6 s.
- **UI tests:** `npm test` takes about 40 s → 17 s, with 597/597 passing in each of 5 runs.
- **Windows nextest:** the serial tests start first. On CI, nextest took 220 s on the Windows run before #1374 and 191 s on #1374's (one run each). Before, the 20 serial `channel_tip_tests` were its last ~41 s.

## Needs decision

| Item | Options | Recommendation |
|---|---|---|
| **Shell job builds release bundles on every PR** (330 of 632 s: 255 s release compile with thin LTO and one codegen unit, 71 s AppImage) | (a) Debug `.deb` on PRs, with release bundles after merge or nightly. (b) Release build with `--bundles deb` only (saves 71 s). (c) Keep. | (a): about 5 min off every UI or shell PR. |
| **Windows `cargo test --doc`** (about 60 s, 0 doctests in the workspace) | (a) Run doctests on Linux only, and update CLAUDE.md, which lists the command. (b) Find out why it rebuilds on Windows. (c) Keep. | (a), since CLAUDE.md names the gate. |
| **`docs/design` PNGs** (226 MB, 87% of the checkout) | (a) Sparse CI checkouts (about 5 s per job). (b) New design PNGs go to release assets or LFS, and history is not rewritten. (c) Keep. | (b). Lossless recompression saves little; the verifier measured it. |
| **UP-11 Fast effort** | Check grok-4.3 `reasoning.effort: none` against the live xAI Responses API. Then merge `accepts_reasoning_effort` with `sends_reasoning_effort`, or reword the changelog. | Check live first (see the QA report). |
| **Web assets have no compression** (387 kB → 119 kB with gzip) | (a) gzip the web-access responses. (b) gzip plus `immutable` caching. (c) Keep. | (a). `immutable` doesn't help behind the self-signed certificate. |
| **History store** (one `threads.json`, parsed whole at boot at about 1.2 ms per MB) | (a) One file per chat. (b) Skip failed quarantine copies, keyed by app version. (c) Keep. | (b) now; (a) only if History grows large. |
| **`fx` (wgpu) on by default** (29 crates, 12.5% of clean-build CPU) | (a) Port the glow to egui_glow. (b) Make `fx` opt-in. (c) Keep. | (c): it saves 13 s on a cold build and nothing on a warm cache. |

## Next, ranked (verified, not done here)

1. **Settings lays out every section every frame.** It is still 2.5 s to the first window in debug, and 3–4% of a core at idle. Lay out only the visible sections (`ScrollArea::show_viewport`).
2. **The Mods page re-reads and parses its cache file on the UI thread every frame** while Offline only is on. Keep parsed pages in `Jobs`.
3. **Each keystroke re-renders every chat bubble (shell-8).** Each `LogItem` gets a fresh menu closure, and `useAttachments` returns a new object every render. Needs a bubble-level render test.
4. **History lists every native session by parsing each file in full** on the UI thread every 2 s (cabin-3). Read only the header off-thread, keyed by generation.
5. **Workboard and idea card chats rebuild their transcript every frame** (cabin-6).
6. **On Windows, Local AI starts two PowerShell probes per view** (shell-7), costing 0.6–2 s.
7. **Providers get a new HTTP client per step** (agent-2), with a fresh TLS handshake each time (60–160 ms per step).
8. **`note_title` parses the whole session on every turn,** and `scrub` checks every user message on every call.
9. **`dock_texture` decodes a full-size JPEG on the UI thread** when the pose changes. It can use the thumbnail worker.
10. **Splitting `grokhub-core`** would save about 7 s of clean build (repo-9).
11. **Cleanup:**
    - About 1,300 dead lines (the Grok Build ACP client, plus 13 public functions with no callers).
    - Three URL openers in the shell (which would also fix the iOS sign-in button).
    - 59 byte-identical helper copies.
    - Raw transport errors shown to users.
    - Moving the `cards.rs` tests out, so its line budget (4,400 of 4,400) has room again.

## Flaky test seen during this work

- `app::tests::chat_threads_tests::main_tab_clear_chat_asks_first_then_wipes_only_main_for_good` failed once in five local runs of a 108-test subset (plain `cargo test`, one test at a time). `threads.json` or `threads.json.bak` still held the cleared Main chat. It passed in every other run, alone and in the full suite. That could be a persist write landing after the clear, which would leave a cleared chat on disk, so it is worth a look of its own.

## Refuted or overstated

- **The Imagine idle repaint** saves 62–67% of frames, not 97%: the tile crossfade needs frames.
- **The Mods rail clone** costs under 1% of a frame.
- **The empty-chat rescans** never run in the same frame.
- **The shell keystroke re-render** has no effect in the web app.
- **A shared HTTP agent** doesn't help per turn on xAI.
- **The keychain read cost on Linux:** the D-Bus connection is reused.
- **The CI cache freeze** costs 0–5 s, not minutes.
- **PNG recompression** saves little time on checkout.
