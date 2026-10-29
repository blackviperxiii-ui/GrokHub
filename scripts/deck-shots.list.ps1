# The deck-shots.ps1 shot list, dot-sourced after Shot is defined. One line
# (or block) per shot, so changes that add shots only append here; the file is
# merge=union in .gitattributes. Keep each Shot call on its own lines.
Shot 1600 900 '1600x900' 'ops-dark' $Slide
Shot 1100 720 '1100x720' 'ops-dark' $Slide
Shot 1600 900 '1600x900-slide20' 'ops-dark' '20'
Shot 1100 720 '1100x720-slide20' 'ops-dark' '20'
Shot 1600 900 'upgrade-1600x900' 'dark' $UpgradeSlide -upgrade
Shot 1600 900 'settings-1600x900' 'ops-dark' '07' -keys '^,'
Shot 1100 720 'settings-1100x720' 'ops-dark' '07' -clickAt 20, 695
Shot 1600 900 'dock-1600x900' 'ops-dark' '02' -dock
Shot 1100 720 'dock-1100x720' 'ops-dark' '02' -dock
Shot 1100 720 'dock-1100x720-scrolled' 'ops-dark' '02' -dock -wheel 10
# Chat tabs HARD DONE: 3 tabs (readable) and 18 tabs with overflow menu open.
Shot 1600 900 'chattabs-3-1600x900' 'ops-dark' '02' -dock -tabs 3
Shot 1100 720 'chattabs-3-1100x720' 'ops-dark' '02' -dock -tabs 3
Shot 1600 900 'chattabs-overflow-1600x900' 'ops-dark' '02' -dock -tabs 18 -overflow
Shot 1100 720 'chattabs-overflow-1100x720' 'ops-dark' '02' -dock -tabs 18 -overflow
# Chat rail last-used: Home (empty) then Chat restores Hangout.
Shot 1600 900 'lastchat-home-1600x900' 'ops-dark' '02' -dock -tabs 3 -lastChat 'home'
Shot 1100 720 'lastchat-home-1100x720' 'ops-dark' '02' -dock -tabs 3 -lastChat 'home'
Shot 1600 900 'lastchat-chat-1600x900' 'ops-dark' '02' -dock -tabs 3 -lastChat 'chat'
Shot 1100 720 'lastchat-chat-1100x720' 'ops-dark' '02' -dock -tabs 3 -lastChat 'chat'
Shot 1600 900 'pulse-1600x900' 'ops-dark' '14' -pulse
Shot 1100 720 'pulse-1100x720' 'ops-dark' '14' -pulse
# Release radar: the release-day post on top of Pulse.
Shot 1600 900 'radar-1600x900' 'ops-dark' '14' -library -radar -nav 'pulse'
Shot 1100 720 'radar-1100x720' 'ops-dark' '14' -library -radar -nav 'pulse'
# At 1100x720 the feed sits under Up next; scroll down to the post.
Shot 1100 720 'radar-1100x720-scrolled' 'ops-dark' '14' -library -radar -nav 'pulse' -wheel 12
# Deck-match 6 proof: same Pulse page beside slides 13 (loadout style) and 15
# (card close-up), at the HARD DONE sizes.
Shot 1600 900 'pulse-1600x900-slide13' 'ops-dark' '13' -pulse
Shot 1100 720 'pulse-1100x720-slide13' 'ops-dark' '13' -pulse
Shot 1600 900 'pulse-1600x900-slide15' 'ops-dark' '15' -pulse
Shot 1100 720 'pulse-1100x720-slide15' 'ops-dark' '15' -pulse
Shot 2560 1440 'pulse-2560x1440' 'ops-dark' '14' -pulse
Shot 1440 900 'pulse-1440x900' 'ops-dark' '14' -pulse
Shot 1024 720 'pulse-1024x720' 'ops-dark' '14' -pulse
Shot 1024 720 'pulse-1024x720-scrolled' 'ops-dark' '14' -pulse -wheel 10
Shot 720 720 'pulse-720x720' 'ops-dark' '14' -pulse
Shot 720 720 'pulse-720x720-ideas' 'ops-dark' '14' -pulse -clickAt 505, 81
# Internet drop log: the daily pattern card on Pulse, using the single
# GROKHUB_DECK_NAV handler in loadout_home.rs.
Shot 1600 900 'net-log-1600x900' 'ops-dark' '14' -netLog -nav 'pulse'
Shot 1100 720 'net-log-1100x720' 'ops-dark' '14' -netLog -nav 'pulse'
Shot 1100 720 'net-log-1100x720-scrolled' 'ops-dark' '14' -netLog -nav 'pulse' -wheel 12
# New local models: the catalog upgrade post on top of Pulse; at 1100x720
# the feed sits under Up next, so that shot scrolls down to it.
Shot 1600 900 'local-models-1600x900' 'ops-dark' '14' -pulse -localModels -nav 'pulse'
Shot 1100 720 'local-models-1100x720-scrolled' 'ops-dark' '14' -pulse -localModels -nav 'pulse' -wheel 12
# 3D model drop: the new-model post with its preview on top of Pulse.
Shot 1600 900 'model-drop-1600x900' 'ops-dark' '14' -pulse -modelDrop -nav 'pulse'
Shot 1100 720 'model-drop-1100x720' 'ops-dark' '14' -pulse -modelDrop -nav 'pulse'
# At 1100x720 the feed sits under Up next; scroll down to the post.
Shot 1100 720 'model-drop-1100x720-scrolled' 'ops-dark' '14' -pulse -modelDrop -nav 'pulse' -wheel 12
# Library is the fifth icon on the 40 px rail, below the chat tab strip.
Shot 1600 900 'library-1600x900' 'ops-dark' '11' -library -clickAt 20, 258
Shot 1100 720 'library-1100x720' 'ops-dark' '11' -library -clickAt 20, 258
# Baldur's Gate 3 picked, so the detail header shows its store art too. A
# synthetic tile click didn't register on the runner, so the seed picks it.
Shot 1600 900 'library-detail-1600x900' 'ops-dark' '11' -library -libraryPick '1086940' -clickAt 20, 258
Shot 1100 720 'library-detail-1100x720' 'ops-dark' '11' -library -libraryPick '1086940' -clickAt 20, 258
# Playbooks chat quality: joke replies show the character's playful response.
Shot 1600 900 'chat-joke-1600x900' 'ops-dark' '02' -dock -chat 'joke'
Shot 1100 720 'chat-joke-1100x720' 'ops-dark' '02' -dock -chat 'joke'
# Layout B: the chat with objectives and recent chats left, snapshot right.
Shot 1600 900 'chat-objectives-1600x900' 'ops-dark' '10' -dock -chatFirst
Shot 1100 720 'chat-objectives-1100x720' 'ops-dark' '10' -dock -chatFirst
# Home Mission Control (slide 09): Objectives+streak | PC Loadout | Up Next covers.
# Pin via GROKHUB_DECK_NAV=home (cabin apply_deck_shot_seed) — a first-icon click
# missed on windows-latest and captured Hangout instead. -library seeds Steam
# covers + dock outcomes (multi-day streak) and waits for PC sensors; Activity
# stays closed on Home. HARD DONE sizes plus narrow for the three-breakpoint proof.
Shot 1600 900 'home-mc-1600x900' 'ops-dark' '09' -nav 'home' -library
Shot 1100 720 'home-mc-1100x720' 'ops-dark' '09' -nav 'home' -library
Shot 900 700 'home-mc-900x700' 'ops-dark' '09' -nav 'home' -library
# Objectives (mission control) is the third icon on the rail.
Shot 1600 900 'objectives-1600x900' 'ops-dark' '09' -library -clickAt 20, 178
Shot 1100 720 'objectives-1100x720' 'ops-dark' '09' -library -clickAt 20, 178
# Privacy & data is the second entry in the Settings list; the dialog is centred.
# The gear at the foot of the rail opens Settings at 1100x720, where Ctrl+, from
# SendKeys missed the window; at 1600x900 the gear sits below the runner's
# screen, so Ctrl+, opens it there.
Shot 1600 900 'privacy-1600x900' 'ops-dark' '12' -openSettings 'privacy'
Shot 1100 720 'privacy-1100x720' 'ops-dark' '12' -openSettings 'privacy'

# Nyx is the default personality: Settings -> Personality on a profile with no
# pick, and on one still saved on the retired 'default' preset.
Shot 1600 900 'personality-fresh-1600x900' 'ops-dark' '07' -keys '^,' -clickAt 110, 234
Shot 1100 720 'personality-fresh-1100x720' 'ops-dark' '07' -clickAt 20, 695, 110, 234
Shot 1600 900 'personality-upgrade-1600x900' 'ops-dark' '07' -personality 'default' -keys '^,' -clickAt 110, 234
Shot 1100 720 'personality-upgrade-1100x720' 'ops-dark' '07' -personality 'default' -clickAt 20, 695, 110, 234
# Personality picker beside slide 19. GROKHUB_DECK_SETTINGS_SEC opens Settings at
# boot (no rail click / Ctrl+, — a miss left Hangout on screen in #958).
Shot 1600 900 'personality-1600x900' 'ops-dark' '19' -pinPersonality
Shot 1100 720 'personality-1100x720' 'ops-dark' '19' -pinPersonality
Shot 1920 1080 'personality-1920x1080' 'ops-dark' '19' -pinPersonality
# Narrow: stacked Selected panel above the wrapping grid (body under TWO_COL_MIN_W).
Shot 900 720 'personality-900x720' 'ops-dark' '19' -pinPersonality
# Playbooks 8: the one-time 18+ check, with adults-only still off.
Shot 1600 900 'age-check-1600x900' 'ops-dark' '19' -pinPersonality -ageCheck -personality 'nyx' -nav 'settings'
Shot 1100 720 'age-check-1100x720' 'ops-dark' '19' -pinPersonality -ageCheck -personality 'nyx' -nav 'settings'
# Nyx's corner box in each mode, beside the approved pictures in
# docs/design/loadout-baseline-v1/nyx-modes/.
foreach ($m in 'working', 'gaming', 'doctor', 'reading', 'hanging-out', 'viper') {
    Shot 1600 900 "nyx-$m-1600x900" 'ops-dark' '02' -dock -mood $m
    Shot 1100 720 "nyx-$m-1100x720" 'ops-dark' '02' -dock -mood $m
}
# Game launch companion: the card on top of Pulse once Steam reports the launch.
Shot 1600 900 'game-launch-1600x900' 'ops-dark' '14' -library -pulse -gameLaunch -nav 'pulse'
Shot 1100 720 'game-launch-1100x720' 'ops-dark' '14' -library -pulse -gameLaunch -nav 'pulse'
# At 1100x720 the feed sits under Up next; scroll down to the post.
Shot 1100 720 'game-launch-1100x720-scrolled' 'ops-dark' '14' -library -pulse -gameLaunch -nav 'pulse' -wheel 12
Set-ItemProperty -Path 'HKCU:\Software\Valve\Steam' -Name 'RunningAppID' -Value 0 -Type DWord
# Card 49: Screenshot explain — composer chip (Hangout) and Settings → Behavior.
# Settings shots rely on GROKHUB_DECK_SETTINGS_SEC=behavior opening Nav::Settings
# in apply_deck_shot_seed (SendKeys/gear clicks miss on 1600×900 in CI).
Shot 1600 900 'screenshot-chip-1600x900' 'ops-dark' '02' -screenshotOffer
Shot 1100 720 'screenshot-chip-1100x720' 'ops-dark' '02' -screenshotOffer
Shot 1600 900 'screenshot-settings-1600x900' 'ops-dark' '07' -pinBehavior
Shot 1100 720 'screenshot-settings-1100x720' 'ops-dark' '07' -pinBehavior
