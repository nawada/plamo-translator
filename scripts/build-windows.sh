#!/usr/bin/env bash
# WSL から Windows 側にソースを同期し、Windows ネイティブの Rust / Node で Tauri アプリをビルドする。
#   使い方: scripts/build-windows.sh [check|test|build|dev]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WIN_USER="$(cmd.exe /c 'echo %USERNAME%' 2>/dev/null | tr -d '\r')"
WIN_DIR_WSL="/mnt/c/Users/${WIN_USER}/prj/plamo_translater"
WIN_DIR="C:\\Users\\${WIN_USER}\\prj\\plamo_translater"
MODE="${1:-build}"

mkdir -p "$WIN_DIR_WSL"
rsync -a --delete \
  --exclude node_modules --exclude dist --exclude 'src-tauri/target' --exclude 'src-tauri/gen' --exclude .git \
  "$ROOT/" "$WIN_DIR_WSL/"

run_ps() {
  (cd /mnt/c && powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "\$ErrorActionPreference='Stop'; Set-Location '$WIN_DIR'; $1")
}

case "$MODE" in
  check) run_ps "Set-Location src-tauri; cargo check --color never" ;;
  test)  run_ps "Set-Location src-tauri; cargo test --color never --lib" ;;
  dev)   run_ps "if (!(Test-Path node_modules)) { npm ci }; npx tauri dev" ;;
  build)
    run_ps "if (!(Test-Path node_modules)) { npm ci }; npx tauri build"
    echo "---- 成果物 ----"
    ls -la "$WIN_DIR_WSL/src-tauri/target/release/bundle/nsis" "$WIN_DIR_WSL/src-tauri/target/release/bundle/msi" 2>/dev/null || true
    ;;
  *) echo "unknown mode: $MODE" >&2; exit 1 ;;
esac
