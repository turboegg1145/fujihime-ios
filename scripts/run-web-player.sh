#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${PORT:-8080}"
CACHE_DIR="${PROJECT_ROOT}/re/.cache"
FONT_CACHE="${CACHE_DIR}/fonts"
SAVE_DIR="${FUJIHIME_SAVE_DIR:-${CACHE_DIR}/web_save}"

mkdir -p "${FONT_CACHE}" "${SAVE_DIR}"

if [ ! -s "${FONT_CACHE}/ipag.ttf" ] || [ ! -s "${FONT_CACHE}/ipam.ttf" ] || [ ! -s "${FONT_CACHE}/wqy-zenhei.ttc" ]; then
  echo "[1/3] Caching Japanese & Simplified Chinese fonts into ${FONT_CACHE}..."
  docker exec fujihime-re bash -c '
    mkdir -p /workspace/re/.cache/fonts
    for src in \
      /tmp/test_fonts/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf \
      /usr/share/fonts/opentype/ipafont-gothic/ipag.ttf \
      /tmp/test_fonts/usr/share/fonts/opentype/ipafont-mincho/ipam.ttf \
      /usr/share/fonts/opentype/ipafont-mincho/ipam.ttf \
      /tmp/test_fonts/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc \
      /usr/share/fonts/truetype/wqy/wqy-zenhei.ttc; do
      [ -f "$src" ] && cp -n "$src" /workspace/re/.cache/fonts/ || true
    done
  ' >/dev/null 2>&1 || true
fi

echo "[2/3] Building fujihime-core web_player (--release --offline) inside fujihime-re..."
docker exec fujihime-re bash -c '
  cd /workspace/src/core && CARGO_BUILD_JOBS=2 cargo build --offline --release --bin web_player
'

BIN="${PROJECT_ROOT}/src/core/target/release/web_player"
if [ ! -x "${BIN}" ]; then
  echo "ERROR: Built web_player binary not found at ${BIN}" >&2
  exit 1
fi

echo "[3/3] Starting Fujihime Web Player at http://127.0.0.1:${PORT}"
export PORT
export FUJIHIME_GAME_DIR="${FUJIHIME_GAME_DIR:-${PROJECT_ROOT}/game/腐姫ＤＬ}"
export FUJIHIME_SAVE_DIR="${SAVE_DIR}"
export FUJIHIME_FONT_DIR="${FUJIHIME_FONT_DIR:-${FONT_CACHE}}"
exec "${BIN}"
