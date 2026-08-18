#!/usr/bin/env bash
# Cloud Agent install for GrokHub.
# Idempotent: refreshes the native GUI build toolchain and warms the cargo
# build cache. Safe to run repeatedly and against a cached target/ directory.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Native GUI build dependencies (eframe/egui + GTK tray). Mirrors the apt list
# used by .github/workflows/ci.yml so the environment matches CI exactly.
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  pkg-config \
  libgtk-3-dev \
  libxkbcommon-dev \
  libxkbcommon-x11-dev \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libgl1-mesa-dev \
  libwayland-dev \
  libasound2-dev

# Pull the pinned dependency set, then warm the workspace build cache so the
# first `cargo run`/`cargo test` in an agent session is fast.
cargo fetch --locked
cargo build --workspace --locked
