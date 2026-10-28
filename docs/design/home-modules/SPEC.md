# Loadout Home — Modules, Slot Grid, Customize (spec v1)
Oct 27, 2026 · repo blackviperxiii-ui/GrokHub @ beta 8c0b6ca4 · Notion card 4073a07c…4530

Home = deck slide 9 "Mission Control": greeting + XP bar on top, module slots in the middle, quick chips + chat bar at the bottom (no chat tab bar). Send/chip → Chat, new tab, message sent, reply streams.

**Data key.** EXISTS = backed by code on beta today. PARTIAL = some fields exist. NEW = new data needed.
Repo sources: `local_governor` + `app/local_signals.rs` (thermal, memory, power, fullscreen, GPU-busy, game-process sampler); `agent/indexers/system_state.rs` (disk, failed services, pending updates); `agent/memory.rs` gaming-library intent (installed apps); `core/pulse` + `app/pulse_ui.rs`, `feed.rs`, `daily_drop_ui.rs`, `interests_ui.rs`; `personality_ui.rs` (presets, custom in `{config}/personalities/`); `core/playbook.rs`, `brain/playbook.rs`, `playbook_ui.rs`; `core/usage.rs` + `budget_ui.rs` + `route/cabin.rs` (tokens/day, cap, spend); `inbox_ui.rs` (decisions); `board_ui.rs` (workboard); `journal_ui.rs`; `systems_brief_ui.rs`; `connectivity_ui.rs`, `glance.rs`; `core/automation.rs`; `threads.rs`; `local_ai_ui.rs`; `audio_check_ui.rs`; `screen_record_ui.rs`; `core/chips.rs`; `mission_ui.rs` (missions/todos). Not present anywhere: XP/level, objectives-with-progress, streak days, cover art, GPU/CPU °C, driver version, network speed test, FPS, friends, achievements, deals.

## Module catalog (36) — sizes L / M / S, every offered size has a drawn look (gallery-1…5.png)

| # | Module | Purpose → what it shows | Sizes | Data | Interactions | Empty / offline |
|---|---|---|---|---|---|---|
| **System & Performance** |
| 1 | PC Loadout | Health at a glance: score %, GPU driver, GPU/CPU temp, storage, RAM, startup apps, Run Tune-up/Details | L M (6 tiles) · S (score + hottest temp + top flag) | PARTIAL: memory/thermal/power from local_governor, disk from system_state; °C, driver, startup apps NEW | Tile → Details; Run Tune-up starts tune-up playbook through normal approval gates | "Sensors unavailable" tiles greyed, tune-up still offered |
| 2 | Speeds | Down/up Mbps, ping, jitter, game-server ping, loss | L (4 tiles+rows+Run test) · M (2 tiles+ping/loss) · S (ping + ↓↑) | NEW (speed test + ping probe) | Run test; tap → history | "Not tested yet · Run test"; offline: "No connection" red |
| 3 | Frame Rate | Live avg FPS, 1% low, sparkline | M · S | NEW (PresentMon/overlay hook) | Tap → per-game history | "Start a game to see FPS" |
| 4 | Storage | Free per drive, big caches | M · S | EXISTS (system_state disk) | "Free up storage" chip → Chat | Single "—" bar if read fails |
| 5 | Updates & Drivers | Pending OS / GPU / game updates | M · S (count) | PARTIAL: pending updates EXISTS; GPU driver/game patch NEW | Row → approve via Decisions | "All up to date" ✓ |
| 6 | Game Mode | Governor state: game detected, fullscreen, assistant quiet, power, GPU busy | M · S | EXISTS (local_governor) | Toggle quiet; tap → Settings | "No game running" |
| 7 | Audio Check | Output/mic device, level, Audio check | M · S | EXISTS (audio_check_ui) | Run check | "No mic found" |
| 8 | Systems Brief | Weekly state-of-systems text + update/failed-service tiles | L · M | EXISTS (systems_brief_ui) | Read brief → Pulse digest | "First brief Sunday night" |
| 9 | Connection | Local/cloud, bytes sent, local model ready | M · S | EXISTS (connectivity, glance) | Tap → Privacy | Offline: red dot "Offline · local only" |
| **Gaming** |
| 10 | Up Next | Next games to play as cover cards | L (hero + 2) · M (3 stacked, slide 9) · S (1 cover) | PARTIAL: installed apps EXISTS; cover art + ordering NEW | Card → Play/launch; Library | Gradient covers with title when no art; "No games found · Scan" |
| 11 | Games · Now Playing | Current/recent games, session & weekly time | L · M (recent list) · S | PARTIAL: game process detection EXISTS; playtime log NEW | Tap → game page | "Nothing playing · last: X" |
| 12 | Game Library | Grid of installed games, store counts | L (4 covers + store chips) · M (2×2) · S (count) | PARTIAL (installed apps; covers/store split NEW) | Search, filter, launch | "Scan library" CTA |
| 13 | Objectives | Today's objectives w/ progress, streak + coach (L) | L (slide 9) · M (top 3) · S (top 1 + bar) | NEW (objective store; seed from mission_ui todos) | Check off, tap → Chat, add via chat bar ("Assign an objective…") | "No objectives · Assign one below" |
| 14 | Streak & XP | Level, XP bar, 7-day streak | M · S | NEW (XP ledger) | Tap → XP history | Day-1 blocks empty, "Start a streak today" |
| 15 | Achievements | Progress, new unlocks | M · S | NEW (Steam/Xbox APIs) | Tap → list | "Connect a store" |
| 16 | Coach Tip | One stat-backed tip | M · S | PARTIAL (proactive cards; stats NEW) | Add as objective; dismiss (counts toward proactive dismiss streak) | Hidden tip → "No tip today" |
| 17 | Patch Notes | Latest patch for your games, impact line | M · S | PARTIAL (Pulse feed sources) | Open → Pulse post; "new build?" → Chat | "No new patches" |
| 18 | Deals | Wishlist discounts | M · S | NEW (store price feeds) | Open store page | "No deals on your list" |
| 19 | What To Play | Random pick from library fitted to time | M · S | PARTIAL (library) | Re-roll, Play | "Scan library first" |
| **AI & Usage** |
| 20 | Usage & Budget | Today's % of cap, local vs cloud share, spend | M · S | EXISTS (core/usage, budget_ui, cabin) | Tap → Settings → Budget | "No usage yet today" 0% |
| 21 | Local AI | On-device model state, VRAM | M · S | EXISTS (local_ai_ui, local_sidecar) | Start/stop, download | "Not installed · Get model" |
| 22 | Personality | Active preset (Nyx), quick switch chips | M · S | EXISTS (personality_ui) | Switch = make default; + Custom | Default preset shown |
| 23 | Playbooks | Top playbooks, last run | M · S | EXISTS (playbook) | Run → Chat with playbook picked | "No playbooks yet" |
| 24 | Recent Chats | Last 3 threads | M · S | EXISTS (threads) | Resume → Chat tab | "No chats yet" |
| 25 | Automations | Active automations, next run | M · S | EXISTS (core/automation) | Toggle, open | "None set up" |
| 26 | Decisions | Pending approvals ("N things need a decision") | M · S (count) | EXISTS (inbox_ui) | Approve/Review inline (same gates) | "Everything is on track" |
| **Social & Streaming** |
| 27 | Squad | Friends online/in game, invite | M · S | NEW (Discord/Steam presence) | Invite → Chat ("Find a duo") | "Connect Discord/Steam" |
| 28 | Clips & Captures | Latest clips, share/record | L · M · S | PARTIAL (screen_record_ui; clip index NEW) | Share, Record, open | "No clips · Record" |
| 29 | Stream Check | Go-live readiness: mic, scene, upload | M · S | NEW (OBS ws) + audio_check EXISTS | Go-live check playbook | "OBS not found" |
| 30 | Pulse · For You | Ranked feed posts | L (4) · M (2) · S (top headline) | EXISTS (pulse, feed, interests) | Open, like/skip (moves interests) | "Pulse is warming up" |
| 31 | Daily Drop | Today's pick | M · S | EXISTS (daily_drop_ui) | Open, Skip | "Next drop tomorrow" |
| **Productivity & Fun** |
| 32 | Workboard | Active cards with status/progress ring | L · M · S (ring) | EXISTS (board_ui) | Open card in place | "No active work" |
| 33 | Journal | Experiments / decisions backlog | M · S | EXISTS (journal_ui) | Open entry | "Start an experiment" |
| 34 | Tonight | Upcoming sessions/events | M · S | NEW (calendar connector) | Tap → event | "Nothing scheduled" |
| 35 | Session Timer | Play-time timer + break nudge | M · S | NEW (local timer) | Start/pause/break | Idle "Start session" |
| 36 | Hype Line | Daily line from active personality | M · S | PARTIAL (personality + model) | Refresh | Static fallback line |

## Slot grid (v2 — one box, three equal-height columns)
- **Size meanings:** **L = dashboard** (combines several things: tiles + list + actions). **M = a group** of related info (one panel, 2–4 facts or a small strip). **S = one stat**: label, big number, one short sub-line. No buttons, no right-side header tag (the ✎ corner button sits there).
- **Slots: 1 L + 2 M + 4 S** (a 5th S was tried; at ~84 px it cramped the number, so it was dropped). A module may only go in a size it declares; every module has an S look, and most have M too.
- **Wide (≥1500 px content):** a single row that fills the height (min 440, default 470). Columns **L : M : S = 1.5 : 1.85 : 1**. L is about 25% narrower than the old Objectives panel; the freed width goes to the M column, and S matches the old Up Next width. Column 2 = M1 above M2 (each ~229 px). Column 3 = S1–S4 stacked (each ~107 px). Gap 12 px; all columns share top and bottom edges so they read as one box.
- **Medium (1100–1499):** row 1 = L | M stack at 1.5 : 1.85. Row 2 = the 4 smalls in a 4-across strip (~107 px tall).
- **Narrow (<1100):** single column: L, then M1, M2, then the smalls as a **2×2**. Below 640 px, L uses its M look. The middle scrolls; the chat bar stays pinned.
- **Hidden slots:** hide an M and the other M fills the column; hide an S and the remaining S share the height; hide all S and the S column collapses so L and M widen at 1.5 : 1.85.
- Min sizes: L 440×440, M 340×210, S 220×96.
- Fixed chrome is unchanged: header (greeting, date·season, XP bar, which can be hidden), chips row, and the chat bar pinned at the bottom.

## Customize flow (✎ corner button → Edit Home dialog) — replaces the in-place "Edit Home mode"
1. A small ✎ icon button (22 px, red border, amber glyph, tooltip "Edit Home") sits in the **top-right corner of the module area**, above the top-right of the S column. It is always visible and doesn't move the layout. Also reachable from Settings → Home and `/home edit`.
2. Clicking it opens a **separate modal dialog, "Settings · Home — Edit Home modules"**, over dimmed Home. Home itself never enters an edit state.
3. Left column, **Your layout**: a mini slot map shaped like Home: Large on the left, Medium 1 and 2 stacked, S1–S4 stacked, each showing its module name and ⠿ drag · ↑↓ reorder · Hide. A hidden slot shows "+ Add module". Below it are the presets (Mission Control, Streamer/Social, Performance/Tuning).
4. Right column, **Catalog · picking for <slot>**: search across 36 modules, category tabs (System & Performance, Gaming, AI & Usage, Social & Streaming, Productivity & Fun), and a "Fits <size>" filter on by default. Cards show a live preview at the selected slot's size, the sizes each module supports, and an "IN <slot>" marker. Click a card to put it in the selected slot; a module already used elsewhere moves (its old slot empties).
5. Swap/reorder: drag slot tiles onto each other, or use ↑↓ within the same size class. Moving between size classes is only allowed when the module supports the target size.
6. Footer: **Reset to default** (with a confirm), **Cancel**, **Save**. Save writes the config and Home re-lays out at once. Esc/✕ = Cancel.

**Default (Mission Control):** L = Objectives dashboard (objectives + weekly streak + coach), M1 = Up Next, M2 = Pulse · For You, S = GPU temp (PC Loadout) · Ping (Speeds) · AI usage % (Usage) · Streak. XP bar on. Chips = Fix my stutter / Find a duo tonight / Free up storage / Plan my week.
**Presets:** Streamer/Social (L Clips, M Squad + Pulse, S Upload (Stream) · Ping · Sent-to-cloud (Connection) · Daily Drop). Performance/Tuning (L PC Loadout, M Speeds + Game Mode, S FPS · Storage · AI usage · Updates).

**Persistence:** `{config}/home_layout.json` next to `card_prefs.json` (per OS user/profile; synced with profile if sync is on): `{ "version":2, "preset":"mission_control", "large":"objectives", "medium":["up_next","pulse"], "small":["pc_loadout","speeds","usage","streak"]  (exactly 4; null = hidden), "xp_bar":true, "module_cfg":{ "<id>":{…} } }`. Unknown/removed ids → slot falls back to the default module; v1 files: keep large/medium, take the first 4 smalls, pad with defaults; bad file → default + toast, file backed up `.bak`.

## Dev plan
1. **Module trait + registry** (`app/home/modules/mod.rs`): `trait HomeModule { fn id(&self)->&'static str; fn name(); fn category()->Category; fn sizes()->&'static [Size]; fn data_status()->DataStatus; fn ui(&mut self, ui:&mut egui::Ui, size:Size, ctx:&HomeCtx)->ModuleResponse; fn empty_state(); }` + `inventory`/static `REGISTRY: &[fn()->Box<dyn HomeModule>]`. Adding a module = one file + one registry line. `ModuleResponse` returns actions (OpenChat(msg), Nav(page), RunPlaybook, Approve) so modules never mutate app state directly.
2. **Panel chrome** shared: red border, header label + amber right label, size-specific padding (one fn, matches deck).
3. **Slot layout engine** (`home/layout.rs`): pure fn `(window rect, HomeLayout) -> Vec<(SlotId, Rect, Size)>` implementing the three breakpoints; unit-tested with fixed rects.
4. **Persisted config** (`home/config.rs`): serde struct above, load/save via persist_service, migration by version, fallback rules, tests.
5. **Edit Home UI**: the ✎ corner button plus an `egui::Modal`/Window dialog: slot map with drag/↑↓/Hide, a catalog with category tabs, search, Fits-size filter and live previews (modules render in preview mode, no actions), presets, Reset/Cancel/Save. The dialog edits a draft copy of HomeLayout and commits only on Save.
6. **Ship existing-data modules first** (1-partial, 4, 6, 8, 9, 20–26, 30–33), then NEW-data ones behind `DataStatus::New` (shown in picker as "coming soon" until wired).
7. Home chat bar + chips → Chat (new tab, sent, streams).
Done criteria unchanged (card): Windows shots at three sizes beside slide 9, customize-flow shots with a module turned off, Send-from-Home recording, PR into beta, green CI, no Co-authored-by.

## Mockups
gallery-1 System & Performance · gallery-2 Gaming · gallery-3 AI & Usage · gallery-4 Social & Streaming · gallery-5 Productivity & Fun · layout-default · layout-streamer · layout-performance · edit-home-dialog (all PNG, HTML source in src/, rendered with headless Chrome).
