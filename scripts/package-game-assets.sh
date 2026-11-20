#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GAME_SRC="${FUJIHIME_GAME_DIR:-${PROJECT_ROOT}/game/腐姫ＤＬ}"
OUT_TAR="${1:-${PROJECT_ROOT}/dist/fujihime-game-assets-v1.tar}"

if [ ! -d "${GAME_SRC}" ]; then
  echo "ERROR: Original game directory not found: ${GAME_SRC}" >&2
  exit 1
fi

ORIG_BACKUP_SRC="${GAME_SRC}/补丁/原版备份"
if [ ! -f "${ORIG_BACKUP_SRC}/scr.xfl" ] || [ ! -f "${ORIG_BACKUP_SRC}/grpo_tp.xfl" ]; then
  echo "ERROR: Original backup archives missing under: ${ORIG_BACKUP_SRC}" >&2
  exit 1
fi

STAGING_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "${STAGING_DIR}"
}
trap cleanup EXIT

mkdir -p "${STAGING_DIR}/game/mov" \
         "${STAGING_DIR}/game/orig_backup" \
         "${STAGING_DIR}/game/grpe" \
         "${STAGING_DIR}/game/grpo_ex" \
         "${STAGING_DIR}/game/grps" \
         "${STAGING_DIR}/Fonts" \
         "$(dirname "${OUT_TAR}")"

echo "[1/4] Copying base .xfl archives (read-only from ${GAME_SRC})..."
REQUIRED_XFLS=(
  bgm.xfl
  grpe.xfl
  grpf.xfl
  grpo.xfl
  grpo_ex.xfl
  grpo_tp.xfl
  grpp.xfl
  grps.xfl
  scr.xfl
  voice.xfl
  wav.xfl
)
for xfl in "${REQUIRED_XFLS[@]}"; do
  src_file="${GAME_SRC}/${xfl}"
  if [ ! -s "${src_file}" ]; then
    echo "ERROR: Missing or empty archive: ${src_file}" >&2
    exit 1
  fi
  cp -p "${src_file}" "${STAGING_DIR}/game/${xfl}"
done

echo "[2/4] Copying MPEG-1 movies, CHS V1.0 scattered directories, and original backup..."
for mpg in 0001.mpg 0002.mpg; do
  if [ ! -s "${GAME_SRC}/mov/${mpg}" ]; then
    echo "ERROR: Missing or empty movie: ${GAME_SRC}/mov/${mpg}" >&2
    exit 1
  fi
  cp -p "${GAME_SRC}/mov/${mpg}" "${STAGING_DIR}/game/mov/${mpg}"
done

for sub in grpe grpo_ex grps; do
  if [ ! -d "${GAME_SRC}/${sub}" ]; then
    echo "ERROR: Missing scattered CHS directory: ${GAME_SRC}/${sub}" >&2
    exit 1
  fi
  cp -r "${GAME_SRC}/${sub}/." "${STAGING_DIR}/game/${sub}/"
done

cp -p "${ORIG_BACKUP_SRC}/scr.xfl" "${STAGING_DIR}/game/orig_backup/scr.xfl"
cp -p "${ORIG_BACKUP_SRC}/grpo_tp.xfl" "${STAGING_DIR}/game/orig_backup/grpo_tp.xfl"

echo "[3/4] Resolving open-source Japanese & Simplified Chinese fonts..."
find_first_file() {
  for candidate in "$@"; do
    if [ -s "${candidate}" ]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done
  return 1
}

fetch_apt_fonts_into_staging() {
  local dl_dir="${STAGING_DIR}/_font_debs"
  mkdir -p "${dl_dir}/extract"
  if command -v apt-get >/dev/null 2>&1 && command -v dpkg-deb >/dev/null 2>&1; then
    (
      cd "${dl_dir}"
      apt-get download fonts-ipafont-gothic fonts-ipafont-mincho fonts-wqy-zenhei >/dev/null 2>&1 || true
      for deb in ./*.deb; do
        [ -f "${deb}" ] && dpkg-deb -x "${deb}" "${dl_dir}/extract"
      done
    )
  fi
}

FONT_SEARCH_ROOTS=(
  "${FUJIHIME_FONT_DIR:-/nonexistent}"
  "/tmp/test_fonts/usr/share/fonts"
  "/usr/share/fonts"
)

JP_GOTHIC="$(find_first_file \
  "${FONT_SEARCH_ROOTS[0]}/jp_gothic.ttf" \
  "${FONT_SEARCH_ROOTS[0]}/ipag.ttf" \
  "${FONT_SEARCH_ROOTS[1]}/opentype/ipafont-gothic/ipag.ttf" \
  "${FONT_SEARCH_ROOTS[2]}/opentype/ipafont-gothic/ipag.ttf" || true)"
JP_MINCHO="$(find_first_file \
  "${FONT_SEARCH_ROOTS[0]}/jp_mincho.ttf" \
  "${FONT_SEARCH_ROOTS[0]}/ipam.ttf" \
  "${FONT_SEARCH_ROOTS[1]}/opentype/ipafont-mincho/ipam.ttf" \
  "${FONT_SEARCH_ROOTS[2]}/opentype/ipafont-mincho/ipam.ttf" || true)"
CHS_HEI="$(find_first_file \
  "${FONT_SEARCH_ROOTS[0]}/chs_hei.ttf" \
  "${FONT_SEARCH_ROOTS[0]}/wqy-zenhei.ttc" \
  "${FONT_SEARCH_ROOTS[1]}/truetype/wqy/wqy-zenhei.ttc" \
  "${FONT_SEARCH_ROOTS[2]}/truetype/wqy/wqy-zenhei.ttc" || true)"

if [ -z "${JP_GOTHIC}" ] || [ -z "${JP_MINCHO}" ] || [ -z "${CHS_HEI}" ]; then
  echo "Some fonts not found locally; attempting apt-get download into temporary staging..."
  fetch_apt_fonts_into_staging
  EXTRACT_FONTS="${STAGING_DIR}/_font_debs/extract/usr/share/fonts"
  JP_GOTHIC="${JP_GOTHIC:-$(find_first_file "${EXTRACT_FONTS}/opentype/ipafont-gothic/ipag.ttf" || true)}"
  JP_MINCHO="${JP_MINCHO:-$(find_first_file "${EXTRACT_FONTS}/opentype/ipafont-mincho/ipam.ttf" || true)}"
  CHS_HEI="${CHS_HEI:-$(find_first_file "${EXTRACT_FONTS}/truetype/wqy/wqy-zenhei.ttc" || true)}"
fi

for pair in \
  "jp_gothic.ttf:${JP_GOTHIC}" \
  "jp_mincho.ttf:${JP_MINCHO}" \
  "chs_hei.ttf:${CHS_HEI}"; do
  dst_name="${pair%%:*}"
  src_path="${pair#*:}"
  if [ -z "${src_path}" ] || [ ! -s "${src_path}" ]; then
    echo "ERROR: Missing font for Fonts/${dst_name}" >&2
    exit 1
  fi
  cp "${src_path}" "${STAGING_DIR}/Fonts/${dst_name}"
  echo "  Fonts/${dst_name} <- ${src_path}"
done

rm -rf "${STAGING_DIR}/_font_debs"
chmod -R u+rwX,go+rX,go-w "${STAGING_DIR}/game" "${STAGING_DIR}/Fonts"

echo "[4/4] Writing lossless tar archive to ${OUT_TAR}..."
TMP_TAR="${OUT_TAR}.tmp"
rm -f "${TMP_TAR}"
tar -C "${STAGING_DIR}" -cf "${TMP_TAR}" game Fonts
mv "${TMP_TAR}" "${OUT_TAR}"

echo "Verifying archive layout..."
tar -tf "${OUT_TAR}" | grep -E '^(game/(scr\.xfl|orig_backup/scr\.xfl|orig_backup/grpo_tp\.xfl|mov/000[12]\.mpg)|Fonts/(jp_gothic|jp_mincho|chs_hei)\.ttf)$' | sort
ls -lh "${OUT_TAR}"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "${OUT_TAR}"
elif command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "${OUT_TAR}"
fi
