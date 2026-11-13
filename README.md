# Loadout

[![CI](https://github.com/blackviperxiii-ui/GrokHub/actions/workflows/ci.yml/badge.svg)](https://github.com/blackviperxiii-ui/GrokHub/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/blackviperxiii-ui/GrokHub)](https://github.com/blackviperxiii-ui/GrokHub/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Local-first Grok desktop assistant for Windows and Linux with permission-gated tools and on-device memory.

Loadout (formerly GrokHub) runs natively on your computer from the system tray so Grok can help with coding, research, files, terminal commands, and desktop apps without giving up local control or privacy. Chats, memory, and credentials stay on your machine, and high-impact actions always wait at an approval gate for your confirmation.

## Table of Contents

- [Features](#features)
- [Getting Started](#getting-started)
  - [Install a Release](#install-a-release)
  - [Build from Source](#build-from-source)
- [Usage](#usage)
  - [First Launch](#first-launch)
  - [CLI and Launch Modes](#cli-and-launch-modes)
  - [Beta Channel](#beta-channel)
  - [Development Checks](#development-checks)
- [Configuration](#configuration)
- [Architecture](#architecture)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

## Features

- **Native agent loop and Code tab** — Multi-step coding, file editing, integrated PTY terminal, and research powered by Loadout's Rust engine ([`crates/grokhub-agent`](crates/grokhub-agent) and [`crates/grokhub-code`](crates/grokhub-code)).
- **Permission-gated desktop and shell tools** — Configurable approval modes (`Ask`, `Auto`, `Always`), OS-level command sandboxing, and mandatory confirmation cards before spending money, sending messages, deleting files, or touching credentials.
- **Pulse** — Personal feed and proactive cards surfacing local watch alerts, PC health checks, Steam game news, daily briefs, and one-tap suggestions.
- **Automations and loops** — Natural-language scheduled jobs (such as `"every weekday at 9am ..."`) and recurring interval checks driven by a background heartbeat.
- **Local memory and optional on-device AI** — Persistent notes (`MEMORY.md` and the `amr/` SQLite FTS5 graph store) plus optional local model fallback when offline or handling sensitive data.
- **Voice, Imagine, and device sync** — Open-mic voice sessions with spoken replies, Grok Imagine image and video generation, LAN sync via `grokhub-hub`, and paired browser web access.
- **Privacy first** — No telemetry; chats, memory, and keys stay on your computer. See [PRIVACY.md](PRIVACY.md).

## Getting Started

### Install a Release

Download the installer or bundle for your platform from the [latest GitHub release](https://github.com/blackviperxiii-ui/GrokHub/releases/latest):

- **Windows (x86_64):** download and run the `GrokHub-Shell-Setup` installer (`GrokHub-Shell-Setup-*.exe`).
- **Linux (x86_64):** download the `.AppImage` or `.deb` (`grokhub-shell-v*-linux-amd64.AppImage` or `.deb`).
- **macOS (Apple silicon):** download `grokhub-shell-v*-macos-arm64.dmg` from tagged or nightly releases (unsigned; right-click the app and choose **Open** on first launch).

Every release includes `SHA256SUMS` and `SHA256SUMS.minisig` signed with [`packaging/minisign.pub`](packaging/minisign.pub):

```bash
minisign -V -p packaging/minisign.pub -m SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
```

### Build from Source

Prerequisites:

- **Rust `stable` toolchain** (pinned in [`rust-toolchain.toml`](rust-toolchain.toml))
- **Node.js** (`>= 22.22.2`) and `npm` (for the shell UI in [`ui/`](ui))
- **Linux system libraries:**
  - Arch / CachyOS:
    ```bash
    sudo pacman -S --needed git rustup base-devel pkgconf nodejs npm webkit2gtk-4.1 gtk3 libxkbcommon libxkbcommon-x11 ffmpeg alsa-utils
    ```
  - Debian / Ubuntu:
    ```bash
    sudo apt-get install -y git pkg-config nodejs npm libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev libxkbcommon-dev libxkbcommon-x11-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libgl1-mesa-dev libwayland-dev libasound2-dev
    ```

Clone and install on Linux (`~/.local/bin/grokhub-shell` and `~/.local/bin/grokhub-hub`):

```bash
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
./scripts/install.sh --user
```

Build the Tauri desktop shell directly on Linux, Windows, or macOS:

```bash
node scripts/stage-hub-sidecar.mjs   # stages grokhub-hub on Windows and macOS; no-op on Linux
npm ci --prefix ui
ui/node_modules/.bin/tauri build -- --locked
```

## Usage

### First Launch

Launch `grokhub-shell` (or open **Loadout** from your system launcher) and connect your SuperGrok account in **Settings → Account** via device-code OAuth, or configure an xAI API key. Closing the window keeps Loadout running in the system tray so scheduled jobs, Pulse watches, and updates continue in the background.

### CLI and Launch Modes

```bash
grokhub-shell                                     # launch the Tauri desktop app
./scripts/install.sh --user --app cabin           # build and install the legacy egui cabin
cargo run -p grokhub-app                          # run the legacy egui cabin from source
cargo run -p grokhub-app -- --agent               # start the cabin hidden in the system tray
cargo run -p grokhub-app -- --oauth               # SuperGrok device-code sign-in
cargo run -p grokhub-app -- --doctor              # check sandbox, tools, and config health
GROKHUB_HUB_PORT=18766 cargo run -p grokhub-hub   # run the standalone LAN sync hub
```

### Beta Channel

- **In-app (Linux):** switch to **Beta** in **About → Updates** (Windows stays on stable releases unless switched to beta pre-releases). You can switch back to stable anytime.
- **From a Linux source clone:**
  ```bash
  ./scripts/install.sh --user --channel beta     # track origin/beta, build, and install
  ./scripts/install.sh --user --channel stable   # switch back to origin/main
  ```

### Development Checks

Run tests and linters with an isolated config directory so local runs do not touch `~/.config/GrokHub`:

```bash
GROKHUB_CONFIG="$(mktemp -d)" cargo test --workspace --locked
cargo clippy --workspace --all-targets -- -D warnings
npm ci --prefix ui
npm --prefix ui run lint
npm --prefix ui test
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

## Configuration

- **Config directory:** `~/.config/GrokHub` on Linux (override with `GROKHUB_CONFIG`), holding `shell.json`, `app.json`, `automations.json`, `loops.json`, `objectives.json`, `memory/MEMORY.md`, and `amr/`.
- **Credentials:** stored in the OS keychain (`GrokHub` service) and `secrets.json` (`0600` permissions on Unix, user-only DACL on Windows). Loadout never reads, copies, or modifies `~/.grok/auth.json`.
- **Feature switches:** optional `<name>.toml` files in the config directory (such as `shell_sandbox.toml`, `compaction.toml`, and `history_trim.toml`); malformed keys fall back to defaults and surface in `--doctor`.

## Architecture

| Path | Role |
|------|------|
| [`src-tauri/`](src-tauri) | Tauri 2 desktop and mobile shell workspace (`grokhub-shell`) |
| [`ui/`](ui) | React 19, TypeScript, and Vite frontend (`grokhub-shell-ui`) |
| [`crates/grokhub-core/`](crates/grokhub-core) | Shared config, `amr` memory store, model router, HTTP facade, and slash parser |
| [`crates/grokhub-agent/`](crates/grokhub-agent) | Native agent loop, tool execution, OS sandboxes, approval gate, and model clients |
| [`crates/grokhub-code/`](crates/grokhub-code) | Code tab backend (repository view, file operations, PTY terminal, and push guard) |
| [`crates/grokhub-desktop/`](crates/grokhub-desktop) | Desktop computer-use and MCP server tools |
| [`crates/grokhub-hub/`](crates/grokhub-hub) | Standalone LAN `/v1` sync and pairing hub (`grokhub-hub`) |
| [`crates/grokhub-proto/`](crates/grokhub-proto) | Portable wire types (`wasm32`-compatible, no I/O) |
| [`crates/grokhub-fps-probe/`](crates/grokhub-fps-probe) | Lightweight frame-rate probe binary (`loadout-fps-probe`) |
| [`crates/grokhub-app/`](crates/grokhub-app) | Legacy native `eframe`/`egui` cabin (`grokhub`) |
| [`xtask/`](xtask) | Release version lockstep and changelog tooling (`cargo xtask bump --check`) |

## Documentation

- [docs/REFERENCE.md](docs/REFERENCE.md) — Complete user, CLI, slash-command, sandbox, and settings reference
- [docs/cross-platform/README.md](docs/cross-platform/README.md) — Tauri 2 + React + TypeScript shell architecture and smoke tests
- [docs/compass/README.md](docs/compass/README.md) — Short module maps for each crate and subsystem
- [PRIVACY.md](PRIVACY.md) — Privacy policy and on-device model routing rules
- [docs/CODE_SIGNING.md](docs/CODE_SIGNING.md) — Windows SignPath Authenticode code signing policy
- [CHANGELOG.md](CHANGELOG.md) — Release notes history

## Contributing

Contributions and pull requests target the `beta` branch (`main` is promotion-only). Before opening a pull request:

1. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the module map in [`docs/compass/README.md`](docs/compass/README.md).
2. Add a release-note fragment under [`changelog.d/`](changelog.d/README.md) rather than editing `CHANGELOG.md` or version files.
3. Run the repository guards and personal-info check locally:
   ```bash
   bash scripts/ci-guards.sh
   python3 scripts/pii-guard/pii_guard.py origin/beta
   ```

## License

Licensed under the MIT License (`SPDX-License-Identifier: MIT`). See [LICENSE](LICENSE) for the full license text.
