#!/usr/bin/env python3
"""
Phase 1C Verification Script (UNK-05 / UNK-07 / UNK-08 / UNK-09):
1. Audio (UNK-09): Full-frame Ogg Vorbis PCM decode of all 3,084 entries across
   bgm.xfl (21), voice.xfl (3,015), and wav.xfl (48), validating RIFF 0x6771
   headers, dummy 0xffffffff OggS padding-page stripping, and `fact` sample counts.
2. Text Caret Codes (UNK-05): DBCS-aware scan of all 341 ORIG (CP932) + 341 CHS (GBK)
   .gsc string tables, asserting 100% coverage by reversed handlers at 0x441180/0x4416d0
   and verifying all referenced `gfNNN.wcg` gaiji assets exist in grps.xfl.
3. Save Binary Round-Trip (UNK-08): Byte-level parse + rebuild SHA-256 verification on
   real `补丁/全CG档/KhDLsave.dat` (206,024 B, 2002 CD layout), plus full parser/serializer
   round-trip SHA-256 self-tests for 2007 DL `KhDLsave.dat` (0x41dcd0/0x41dd70) and
   `KhDLsave%02d.dat` (75,026 B, 0x41de70/0x41dff0).

Run inside fujihime-re container:
    python3 /workspace/re/verify_audio_save.py --json-out /tmp/phase1c_verify_out.json
"""

import argparse
import ctypes
import hashlib
import json
import os
import struct
import subprocess
import sys
import time
from collections import Counter
from typing import Any, Dict, List, Optional, Tuple

GAME_DIR = "/workspace/game/腐姫ＤＬ"
BGM_XFL = f"{GAME_DIR}/bgm.xfl"
VOICE_XFL = f"{GAME_DIR}/voice.xfl"
WAV_XFL = f"{GAME_DIR}/wav.xfl"
GRPS_XFL = f"{GAME_DIR}/grps.xfl"
ORIG_SCR_XFL = f"{GAME_DIR}/补丁/原版备份/scr.xfl"
CHS_SCR_XFL = f"{GAME_DIR}/scr.xfl"
REAL_FULLCG_SAVE = f"{GAME_DIR}/补丁/全CG档/KhDLsave.dat"

VORBIS_LIB_DIR = "/tmp/vorbis_sys/usr/lib/x86_64-linux-gnu"
VORBIS_INC_DIR = "/tmp/vorbis_sys/usr/include"
VORBIS_SO_PATH = "/tmp/vorbis_verify.so"
VORBIS_C_PATH = "/tmp/vorbis_verify.c"

VORBIS_VERIFY_C = r"""#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <vorbis/vorbisfile.h>

typedef struct {
    const uint8_t *data;
    size_t size;
    size_t pos;
} MemBuf;

static size_t mem_read(void *ptr, size_t size, size_t nmemb, void *datasource) {
    MemBuf *m = (MemBuf *)datasource;
    size_t want = size * nmemb;
    size_t avail = m->size - m->pos;
    if (want > avail) want = avail;
    if (size > 0) want = (want / size) * size;
    memcpy(ptr, m->data + m->pos, want);
    m->pos += want;
    return size ? (want / size) : 0;
}

static int mem_seek(void *datasource, ogg_int64_t offset, int whence) {
    MemBuf *m = (MemBuf *)datasource;
    ogg_int64_t np = 0;
    if (whence == SEEK_SET) np = offset;
    else if (whence == SEEK_CUR) np = (ogg_int64_t)m->pos + offset;
    else if (whence == SEEK_END) np = (ogg_int64_t)m->size + offset;
    else return -1;
    if (np < 0 || (size_t)np > m->size) return -1;
    m->pos = (size_t)np;
    return 0;
}

static int mem_close(void *datasource) { (void)datasource; return 0; }
static long mem_tell(void *datasource) { return (long)((MemBuf *)datasource)->pos; }

int verify_ogg_pcm(const uint8_t *buf, size_t len, int *out_ch, long *out_rate, int64_t *out_samples, int *out_holes) {
    MemBuf mb = { buf, len, 0 };
    ov_callbacks cb = { mem_read, mem_seek, mem_close, mem_tell };
    OggVorbis_File vf;
    int orc = ov_open_callbacks(&mb, &vf, NULL, 0, cb);
    if (orc < 0) {
        return orc;
    }
    vorbis_info *vi = ov_info(&vf, -1);
    if (!vi) {
        ov_clear(&vf);
        return -2;
    }
    *out_ch = vi->channels;
    *out_rate = vi->rate;
    int64_t total_frames = 0;
    int holes = 0;
    char pcm[4096];
    int bitstream = 0;
    for (;;) {
        long ret = ov_read(&vf, pcm, sizeof(pcm), 0, 2, 1, &bitstream);
        if (ret == 0) break;
        if (ret == OV_HOLE) { holes++; continue; }
        if (ret < 0) {
            ov_clear(&vf);
            *out_samples = total_frames;
            *out_holes = holes;
            return (int)ret;
        }
        total_frames += ret / (2 * vi->channels);
    }
    *out_samples = total_frames;
    *out_holes = holes;
    ov_clear(&vf);
    return 0;
}
"""


def parse_xfl(path: str) -> List[Tuple[str, bytes]]:
    data = open(path, "rb").read()
    if data[:4] != b"LB\x01\x00":
        raise ValueError(f"{path}: bad XFL magic {data[:4]!r}")
    dir_size, count = struct.unpack_from("<II", data, 4)
    data_base = 12 + dir_size
    entries: List[Tuple[str, bytes]] = []
    pos = 12
    for _ in range(count):
        name = data[pos : pos + 32].split(b"\x00", 1)[0].decode("cp932", "replace")
        off, sz = struct.unpack_from("<II", data, pos + 32)
        entries.append((name, data[data_base + off : data_base + off + sz]))
        pos += 40
    return entries


# ---------------------------------------------------------------------------
# 1. Audio Verification (UNK-09)
# ---------------------------------------------------------------------------

def ensure_vorbis_verifier() -> ctypes.CDLL:
    need_sys = not (
        os.path.exists(f"{VORBIS_LIB_DIR}/libvorbisfile.so.3")
        and os.path.exists(f"{VORBIS_LIB_DIR}/libvorbis.so.0")
        and os.path.exists(f"{VORBIS_LIB_DIR}/libogg.so.0")
    )
    if need_sys:
        print("[audio] Bootstrapping libvorbis/libogg into /tmp/vorbis_sys ...", flush=True)
        os.makedirs("/tmp/apt/lists/partial", exist_ok=True)
        os.makedirs("/tmp/apt/archives/partial", exist_ok=True)
        os.makedirs("/tmp/vorbis_sys", exist_ok=True)
        env = os.environ.copy()
        env.pop("http_proxy", None)
        env.pop("https_proxy", None)
        env["TMPDIR"] = "/tmp"
        apt_opts = [
            "-o", "Dir::State::lists=/tmp/apt/lists",
            "-o", "Dir::Cache::archives=/tmp/apt/archives",
            "-o", "Debug::NoLocking=1",
        ]
        subprocess.run(["apt-get", *apt_opts, "update"], check=True, env=env)
        subprocess.run(
            [
                "apt-get", *apt_opts, "install", "-y", "--download-only",
                "libvorbisfile3", "libvorbis0a", "libogg0", "libvorbis-dev", "libogg-dev",
            ],
            check=True,
            env=env,
        )
        subprocess.run(
            'for f in /tmp/apt/archives/*.deb; do dpkg-deb -x "$f" /tmp/vorbis_sys; done',
            shell=True,
            check=True,
        )

    if not os.path.exists(VORBIS_SO_PATH):
        with open(VORBIS_C_PATH, "w", encoding="utf-8") as f:
            f.write(VORBIS_VERIFY_C)
        subprocess.run(
            [
                "gcc", "-O2", "-shared", "-fPIC",
                VORBIS_C_PATH,
                f"-I{VORBIS_INC_DIR}",
                f"-L{VORBIS_LIB_DIR}",
                "-lvorbisfile", "-lvorbis", "-logg",
                "-o", VORBIS_SO_PATH,
            ],
            check=True,
        )

    # Preload with RTLD_GLOBAL so no LD_LIBRARY_PATH is required
    ctypes.CDLL(f"{VORBIS_LIB_DIR}/libogg.so.0", mode=ctypes.RTLD_GLOBAL)
    ctypes.CDLL(f"{VORBIS_LIB_DIR}/libvorbis.so.0", mode=ctypes.RTLD_GLOBAL)
    ctypes.CDLL(f"{VORBIS_LIB_DIR}/libvorbisfile.so.3", mode=ctypes.RTLD_GLOBAL)
    lib = ctypes.CDLL(VORBIS_SO_PATH)
    lib.verify_ogg_pcm.argtypes = [
        ctypes.c_char_p,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_long),
        ctypes.POINTER(ctypes.c_int64),
        ctypes.POINTER(ctypes.c_int),
    ]
    lib.verify_ogg_pcm.restype = ctypes.c_int
    return lib


def parse_riff_ogg(blob: bytes) -> Dict[str, Any]:
    if len(blob) < 12 or blob[:4] != b"RIFF" or blob[8:12] != b"WAVE":
        raise ValueError("Not a RIFF WAVE container")
    pos = 12
    fmt_info = None
    fact_samples = None
    data_bytes = None
    while pos + 8 <= len(blob):
        cid = blob[pos : pos + 4]
        csz = struct.unpack_from("<I", blob, pos + 4)[0]
        cdata = blob[pos + 8 : pos + 8 + csz]
        if cid == b"fmt " and len(cdata) >= 16:
            wfmt, ch, rate, avg_bps, block_align, bps = struct.unpack_from("<HHIIHH", cdata, 0)
            fmt_info = (wfmt, ch, rate, avg_bps, block_align, bps)
        elif cid == b"fact" and len(cdata) >= 4:
            fact_samples = struct.unpack_from("<I", cdata, 0)[0]
        elif cid == b"data":
            data_bytes = cdata
        pos += 8 + csz + (csz & 1)
    if fmt_info is None or data_bytes is None:
        raise ValueError("Missing fmt or data chunk")
    return {"fmt": fmt_info, "fact_samples": fact_samples, "data": data_bytes}


def strip_vorbis_acm_dummy_pages(ogg_data: bytes) -> Tuple[bytes, int, int, bool]:
    pos = 0
    out_chunks: List[bytes] = []
    dummy_pages = 0
    real_pages = 0
    eos_seen = False
    n = len(ogg_data)
    while pos < n:
        if pos + 27 > n or ogg_data[pos : pos + 4] != b"OggS":
            raise ValueError(f"OggS sync lost at offset {pos}/{n}")
        header_type = ogg_data[pos + 5]
        gran = struct.unpack_from("<Q", ogg_data, pos + 6)[0]
        serial = struct.unpack_from("<I", ogg_data, pos + 14)[0]
        nseg = ogg_data[pos + 26]
        if pos + 27 + nseg > n:
            raise ValueError(f"Truncated Ogg segment table at {pos}")
        body_len = sum(ogg_data[pos + 27 : pos + 27 + nseg])
        page_len = 27 + nseg + body_len
        if pos + page_len > n:
            raise ValueError(f"Truncated Ogg page body at {pos}")
        if serial == 0xFFFFFFFF and body_len == 0 and gran == 0:
            dummy_pages += 1
        else:
            out_chunks.append(ogg_data[pos : pos + page_len])
            real_pages += 1
            if header_type & 0x04:
                eos_seen = True
        pos += page_len
    return b"".join(out_chunks), dummy_pages, real_pages, eos_seen


def verify_all_audio() -> Dict[str, Any]:
    t0 = time.time()
    lib = ensure_vorbis_verifier()
    archives = [("bgm.xfl", BGM_XFL), ("voice.xfl", VOICE_XFL), ("wav.xfl", WAV_XFL)]

    out_ch = ctypes.c_int()
    out_rate = ctypes.c_long()
    out_samples = ctypes.c_int64()
    out_holes = ctypes.c_int()

    summary: Dict[str, Any] = {
        "total_entries": 0,
        "total_passed": 0,
        "total_failed": 0,
        "total_samples": 0,
        "total_duration_sec": 0.0,
        "total_dummy_pages_stripped": 0,
        "total_holes": 0,
        "total_fact_mismatches": 0,
        "total_missing_eos": 0,
        "archives": {},
        "failures": [],
    }

    for label, xfl_path in archives:
        entries = parse_xfl(xfl_path)
        arc_stats = {
            "entries": len(entries),
            "passed": 0,
            "failed": 0,
            "channels": Counter(),
            "sample_rates": Counter(),
            "format_tags": Counter(),
            "total_samples": 0,
            "duration_sec": 0.0,
            "dummy_pages_stripped": 0,
            "real_pages": 0,
            "holes": 0,
            "fact_mismatches": 0,
            "missing_eos": 0,
            "min_duration_sec": None,
            "max_duration_sec": 0.0,
        }
        if label == "bgm.xfl":
            arc_stats["entry_names"] = [name for name, _ in entries]

        for name, blob in entries:
            try:
                riff = parse_riff_ogg(blob)
                wfmt, hdr_ch, hdr_rate, _, _, _ = riff["fmt"]
                arc_stats["format_tags"][f"0x{wfmt:04x}"] += 1
                if wfmt != 0x6771:
                    raise ValueError(f"Unexpected fmt tag 0x{wfmt:04x}")
                clean_ogg, dummy_p, real_p, eos = strip_vorbis_acm_dummy_pages(riff["data"])
                arc_stats["dummy_pages_stripped"] += dummy_p
                arc_stats["real_pages"] += real_p
                if not eos:
                    arc_stats["missing_eos"] += 1

                buf_c = ctypes.create_string_buffer(clean_ogg, len(clean_ogg))
                rc = lib.verify_ogg_pcm(
                    ctypes.cast(buf_c, ctypes.c_char_p),
                    len(clean_ogg),
                    ctypes.byref(out_ch),
                    ctypes.byref(out_rate),
                    ctypes.byref(out_samples),
                    ctypes.byref(out_holes),
                )
                if rc != 0:
                    raise ValueError(f"libvorbisfile decode rc={rc}")
                ch = out_ch.value
                rate = out_rate.value
                samples = out_samples.value
                holes = out_holes.value
                if ch != hdr_ch or rate != hdr_rate:
                    raise ValueError(f"header ({hdr_ch}ch/{hdr_rate}) != vorbis ({ch}ch/{rate})")
                if samples <= 0:
                    raise ValueError("0 decoded PCM samples")
                if riff["fact_samples"] is not None and riff["fact_samples"] != samples:
                    arc_stats["fact_mismatches"] += 1
                    raise ValueError(f"fact_samples={riff['fact_samples']} != decoded={samples}")

                dur = samples / float(rate)
                arc_stats["passed"] += 1
                arc_stats["channels"][str(ch)] += 1
                arc_stats["sample_rates"][str(rate)] += 1
                arc_stats["total_samples"] += samples
                arc_stats["duration_sec"] += dur
                arc_stats["holes"] += holes
                if arc_stats["min_duration_sec"] is None or dur < arc_stats["min_duration_sec"]:
                    arc_stats["min_duration_sec"] = dur
                if dur > arc_stats["max_duration_sec"]:
                    arc_stats["max_duration_sec"] = dur
            except Exception as exc:
                arc_stats["failed"] += 1
                summary["failures"].append({"archive": label, "entry": name, "error": str(exc)})

        arc_stats["channels"] = dict(arc_stats["channels"])
        arc_stats["sample_rates"] = dict(arc_stats["sample_rates"])
        arc_stats["format_tags"] = dict(arc_stats["format_tags"])
        arc_stats["duration_sec"] = round(arc_stats["duration_sec"], 3)
        arc_stats["min_duration_sec"] = round(arc_stats["min_duration_sec"] or 0.0, 4)
        arc_stats["max_duration_sec"] = round(arc_stats["max_duration_sec"], 4)

        summary["archives"][label] = arc_stats
        summary["total_entries"] += arc_stats["entries"]
        summary["total_passed"] += arc_stats["passed"]
        summary["total_failed"] += arc_stats["failed"]
        summary["total_samples"] += arc_stats["total_samples"]
        summary["total_duration_sec"] += arc_stats["duration_sec"]
        summary["total_dummy_pages_stripped"] += arc_stats["dummy_pages_stripped"]
        summary["total_holes"] += arc_stats["holes"]
        summary["total_fact_mismatches"] += arc_stats["fact_mismatches"]
        summary["total_missing_eos"] += arc_stats["missing_eos"]

    summary["total_duration_sec"] = round(summary["total_duration_sec"], 3)
    summary["elapsed_sec"] = round(time.time() - t0, 2)
    return summary


# ---------------------------------------------------------------------------
# 2. Text Caret Control Codes Coverage (UNK-05)
# ---------------------------------------------------------------------------

def is_cp932_lead(b: int) -> bool:
    return (0x81 <= b <= 0x9F) or (0xE0 <= b <= 0xFC)


def is_cp932_trail(b: int) -> bool:
    return (0x40 <= b <= 0x7E) or (0x80 <= b <= 0xFC)


def is_gbk_lead(b: int) -> bool:
    return 0x81 <= b <= 0xFE


def is_gbk_trail(b: int) -> bool:
    return (0x40 <= b <= 0x7E) or (0x80 <= b <= 0xFE)


def parse_gsc_strings(gsc_bytes: bytes) -> List[bytes]:
    # 36-byte header per docs/vm-opcodes-spec.md: fsize, hdr_sz, code_sz, soff_sz, sdat_sz
    _, hdr_sz, code_sz, str_off_sz, str_data_sz = struct.unpack_from("<5I", gsc_bytes, 0)
    str_off_base = hdr_sz + code_sz
    str_data_base = str_off_base + str_off_sz
    str_data = gsc_bytes[str_data_base : str_data_base + str_data_sz]
    count = str_off_sz // 4
    out: List[bytes] = []
    for idx in range(count):
        off = struct.unpack_from("<I", gsc_bytes, str_off_base + idx * 4)[0]
        if off >= len(str_data):
            continue
        end = str_data.find(b"\x00", off)
        if end == -1:
            end = len(str_data)
        out.append(str_data[off:end])
    return out


def scan_caret_tokens(raw: bytes, is_chs: bool) -> List[Tuple[str, str]]:
    """Simulate 0x441180 DBCS-safe caret escape scanner."""
    tokens: List[Tuple[str, str]] = []
    i = 0
    n = len(raw)
    lead_fn = is_gbk_lead if is_chs else is_cp932_lead
    trail_fn = is_gbk_trail if is_chs else is_cp932_trail
    while i < n:
        b = raw[i]
        if i + 1 < n and lead_fn(b) and trail_fn(raw[i + 1]):
            i += 2
            continue
        if b != 0x5E:  # '^'
            i += 1
            continue
        if i + 1 >= n:
            tokens.append(("TRUNCATED_^", "^"))
            break
        cmd = chr(raw[i + 1]) if 0x20 <= raw[i + 1] < 0x7F else f"\\x{raw[i + 1]:02x}"
        cl = cmd.lower()
        if cl in ("a", "g"):
            j = i + 2
            if j < n and raw[j] == 0x2D:
                j += 1
            while j < n and 0x30 <= raw[j] <= 0x39:
                j += 1
            tok = raw[i:j].decode("ascii", "replace")
            tokens.append((f"^{cl}", tok))
            i = j
        elif cl == "v":
            j = i + 2
            while j < n and (raw[j] == 0x2D or 0x30 <= raw[j] <= 0x39):
                j += 1
            tok = raw[i:j].decode("ascii", "replace")
            tokens.append(("^v", tok))
            i = j
        elif cl in ("b", "i", "l", "m", "n"):
            tokens.append((f"^{cl}", f"^{cmd}"))
            i += 2
        elif cl in ("c", "d", "f", "s"):
            if i + 2 >= n:
                tokens.append((f"TRUNCATED_^{cl}", f"^{cmd}"))
                break
            arg = chr(raw[i + 2]) if 0x20 <= raw[i + 2] < 0x7F else f"\\x{raw[i + 2]:02x}"
            tokens.append((f"^{cl}", f"^{cmd}{arg}"))
            i += 3
        else:
            # 0x441180 switchD_00441292_caseD_45 default drops 2 bytes ('^' + next)
            tokens.append(("DEFAULT_DROP_2B", f"^{cmd}"))
            i += 2
    return tokens


SUPPORTED_CARET_HANDLERS = {"^a", "^b", "^c", "^d", "^f", "^g", "^i", "^l", "^m", "^n", "^s", "^v"}
SUPPORTED_COLORS = set("bgkoprsvwy0123456789")


def verify_caret_controls() -> Dict[str, Any]:
    grps_names = {name.lower() for name, _ in parse_xfl(GRPS_XFL)}
    gaiji_wcg_dims: Dict[str, Tuple[int, int]] = {}
    for name, blob in parse_xfl(GRPS_XFL):
        if name.lower().startswith("gf") and name.lower().endswith(".wcg"):
            w, h = struct.unpack_from("<II", blob, 8)
            gaiji_wcg_dims[name.lower()] = (w, h)

    results: Dict[str, Any] = {
        "gaiji_assets_in_grps": sorted(gaiji_wcg_dims.keys()),
        "gaiji_dimensions": {k: list(v) for k, v in sorted(gaiji_wcg_dims.items())},
        "corpora": {},
    }

    for label, xfl_path, is_chs in [
        ("orig", ORIG_SCR_XFL, False),
        ("chs", CHS_SCR_XFL, True),
    ]:
        entries = parse_xfl(xfl_path)
        handler_counts: Counter = Counter()
        token_counts: Counter = Counter()
        gaiji_refs: Counter = Counter()
        default_hits: List[Dict[str, Any]] = []
        unsupported_hits: List[Dict[str, Any]] = []
        missing_gaiji: List[str] = []

        for name, gsc in entries:
            if not name.lower().endswith(".gsc"):
                continue
            for sidx, raw_s in enumerate(parse_gsc_strings(gsc)):
                for handler, tok in scan_caret_tokens(raw_s, is_chs):
                    handler_counts[handler] += 1
                    token_counts[tok] += 1
                    if handler == "DEFAULT_DROP_2B":
                        default_hits.append({"script": name, "str_idx": sidx, "token": tok})
                    elif handler not in SUPPORTED_CARET_HANDLERS:
                        unsupported_hits.append({"script": name, "str_idx": sidx, "handler": handler, "token": tok})
                    elif handler == "^c":
                        ch = tok[2:].lower()
                        if ch not in SUPPORTED_COLORS:
                            unsupported_hits.append({"script": name, "str_idx": sidx, "handler": "^c", "token": tok})
                    elif handler == "^g":
                        gid = int(tok[2:] or "0")
                        wcg = f"gf{gid:03d}.wcg"
                        gaiji_refs[wcg] += 1
                        if wcg not in grps_names:
                            missing_gaiji.append(wcg)

        results["corpora"][label] = {
            "scripts_scanned": len(entries),
            "total_caret_tokens": sum(token_counts.values()),
            "unique_tokens": len(token_counts),
            "handler_counts": dict(handler_counts.most_common()),
            "token_counts": dict(token_counts.most_common()),
            "gaiji_refs": dict(gaiji_refs.most_common()),
            "default_drop_2b_hits": default_hits,
            "unsupported_hits": unsupported_hits,
            "missing_gaiji_assets": sorted(set(missing_gaiji)),
        }
        if unsupported_hits or missing_gaiji:
            raise AssertionError(f"{label}: unsupported caret hits={unsupported_hits}, missing_gaiji={missing_gaiji}")
        # Verify the only default hit in ORIG is the known 2058.gsc ^ﾝ (\xdd) typo
        for hit in default_hits:
            if not (label == "orig" and hit["script"].lower() == "2058.gsc" and hit["token"] == "^\\xdd"):
                raise AssertionError(f"Unexpected default caret hit: {hit}")

    return results


# ---------------------------------------------------------------------------
# 3. Save File Parsing & Round-Trip Verification (UNK-08)
# ---------------------------------------------------------------------------

def parse_2002_cd_global_save(blob: bytes) -> Dict[str, Any]:
    if len(blob) != 206024:
        raise ValueError(f"Expected 206024 B for 2002 CD KhDLsave.dat, got {len(blob)}")
    header_u16 = list(struct.unpack_from("<12H", blob, 0))
    vars_7000_9999 = list(struct.unpack_from("<3000h", blob, 24))
    kidoku_rows = [blob[6024 + i * 20 : 6024 + (i + 1) * 20] for i in range(10000)]
    return {
        "format": "2002_cd_global",
        "header_u16": header_u16,
        "vars_7000_9999": vars_7000_9999,
        "kidoku_rows": kidoku_rows,
    }


def build_2002_cd_global_save(parsed: Dict[str, Any]) -> bytes:
    out = bytearray()
    out += struct.pack("<12H", *parsed["header_u16"])
    out += struct.pack("<3000h", *parsed["vars_7000_9999"])
    for row in parsed["kidoku_rows"]:
        if len(row) != 20:
            raise ValueError("kidoku row must be 20 bytes")
        out += row
    return bytes(out)


def _read_bitmap_map(blob: bytes, pos: int) -> Tuple[Dict[int, bytes], int]:
    (count,) = struct.unpack_from("<I", blob, pos)
    pos += 4
    mp: Dict[int, bytes] = {}
    for _ in range(count):
        key, byte_len = struct.unpack_from("<HI", blob, pos)
        pos += 6
        mp[key] = blob[pos : pos + byte_len]
        pos += byte_len
    return mp, pos


def _write_bitmap_map(mp: Dict[int, bytes]) -> bytes:
    out = bytearray(struct.pack("<I", len(mp)))
    for key in sorted(mp.keys()):
        raw = mp[key]
        out += struct.pack("<HI", key, len(raw))
        out += raw
    return bytes(out)


def parse_2007_dl_global_save(blob: bytes) -> Dict[str, Any]:
    """Parser matching KhimeDL.exe 0x41dcd0 (write) / 0x41dd70 (read)."""
    if len(blob) < 0x814 + 6000 + 8:
        raise ValueError(f"Too short for 2007 DL KhDLsave.dat: {len(blob)}")
    cfg_raw = blob[:0x814]
    u16_fields = struct.unpack_from("<28H", cfg_raw, 0)  # +0x00..+0x37 (0x38 bytes)
    global_strs = [cfg_raw[0x3A + i * 201 : 0x3A + (i + 1) * 201] for i in range(10)]
    vars_7000_9999 = list(struct.unpack_from("<3000h", blob, 0x814))
    kidoku_map, pos = _read_bitmap_map(blob, 0x814 + 6000)
    global_flags_map, pos = _read_bitmap_map(blob, pos)
    if pos != len(blob):
        raise ValueError(f"Trailing bytes in 2007 DL KhDLsave.dat: {pos} != {len(blob)}")
    return {
        "format": "2007_dl_global",
        "cfg_raw": cfg_raw,
        "fields": {
            "fullscreen": u16_fields[0],           # +0x00 (0x471378)
            "text_speed": u16_fields[1],           # +0x02 (0x47137a)
            "bgm_enable": u16_fields[2],           # +0x04 (0x47137c)
            "se_enable": u16_fields[3],            # +0x06 (0x47137e)
            "voice_enable": u16_fields[4],         # +0x08 (0x471380)
            "bgm_volume": u16_fields[5],           # +0x0a (0x471382)
            "se_volume": u16_fields[6],            # +0x0c (0x471384)
            "voice_volume": u16_fields[7],         # +0x0e (0x471386)
            "effect_enable": u16_fields[9],        # +0x12 (0x47138a)
            "skip_unread": u16_fields[10],         # +0x14 (0x47138c)
            "voice_cut_on_click": u16_fields[11],  # +0x16 (0x47138e)
            "const_0x20": u16_fields[12],          # +0x18 (0x471390)
            "const_1": u16_fields[13],             # +0x1a (0x471392)
            "auto_wait_ms": u16_fields[14],        # +0x1c (0x471394)
            "msg_win_style": u16_fields[15],       # +0x1e (0x471396)
            "confirm_dialog_enable": u16_fields[16], # +0x20 (0x471398)
            "last_save_page": u16_fields[27],      # +0x36 (0x4713ae)
        },
        "global_strs": global_strs,
        "vars_7000_9999": vars_7000_9999,
        "kidoku_map": kidoku_map,
        "global_flags_map": global_flags_map,
    }


def build_2007_dl_global_save(parsed: Dict[str, Any]) -> bytes:
    out = bytearray(parsed["cfg_raw"])
    # Re-encode verified u16 fields and 10 global string slots cleanly
    f = parsed["fields"]
    struct.pack_into("<H", out, 0x00, f["fullscreen"])
    struct.pack_into("<H", out, 0x02, f["text_speed"])
    struct.pack_into("<H", out, 0x04, f["bgm_enable"])
    struct.pack_into("<H", out, 0x06, f["se_enable"])
    struct.pack_into("<H", out, 0x08, f["voice_enable"])
    struct.pack_into("<H", out, 0x0A, f["bgm_volume"])
    struct.pack_into("<H", out, 0x0C, f["se_volume"])
    struct.pack_into("<H", out, 0x0E, f["voice_volume"])
    struct.pack_into("<H", out, 0x12, f["effect_enable"])
    struct.pack_into("<H", out, 0x14, f["skip_unread"])
    struct.pack_into("<H", out, 0x16, f["voice_cut_on_click"])
    struct.pack_into("<H", out, 0x18, f["const_0x20"])
    struct.pack_into("<H", out, 0x1A, f["const_1"])
    struct.pack_into("<H", out, 0x1C, f["auto_wait_ms"])
    struct.pack_into("<H", out, 0x1E, f["msg_win_style"])
    struct.pack_into("<H", out, 0x20, f["confirm_dialog_enable"])
    struct.pack_into("<H", out, 0x36, f["last_save_page"])
    for i, slot in enumerate(parsed["global_strs"]):
        out[0x3A + i * 201 : 0x3A + (i + 1) * 201] = slot
    out += struct.pack("<3000h", *parsed["vars_7000_9999"])
    out += _write_bitmap_map(parsed["kidoku_map"])
    out += _write_bitmap_map(parsed["global_flags_map"])
    return bytes(out)


def parse_2007_dl_slot_save(blob: bytes) -> Dict[str, Any]:
    """Parser matching KhimeDL.exe 0x41de70 / 0x41dff0 (75,026 B)."""
    if len(blob) != 75026:
        raise ValueError(f"Expected 75026 B for KhDLsave%02d.dat, got {len(blob)}")
    hdr_u16 = list(struct.unpack_from("<8H", blob, 0))
    title_raw = blob[16:42]
    primary_snap = blob[42 : 42 + 0x5BC4]
    primary_vars = list(struct.unpack_from("<7000h", blob, 42 + 0x5BC4))
    rollback_snap = blob[42 + 0x5BC4 + 14000 : 42 + 2 * 0x5BC4 + 14000]
    rollback_vars = list(struct.unpack_from("<7000h", blob, 42 + 2 * 0x5BC4 + 14000))
    return {
        "format": "2007_dl_slot",
        "timestamp_and_vars123": hdr_u16,
        "title_raw_26b": title_raw,
        "primary_snap_23492b": primary_snap,
        "primary_vars_0_6999": primary_vars,
        "rollback_snap_23492b": rollback_snap,
        "rollback_vars_0_6999": rollback_vars,
    }


def build_2007_dl_slot_save(parsed: Dict[str, Any]) -> bytes:
    out = bytearray()
    out += struct.pack("<8H", *parsed["timestamp_and_vars123"])
    out += parsed["title_raw_26b"]
    out += parsed["primary_snap_23492b"]
    out += struct.pack("<7000h", *parsed["primary_vars_0_6999"])
    out += parsed["rollback_snap_23492b"]
    out += struct.pack("<7000h", *parsed["rollback_vars_0_6999"])
    return bytes(out)


def verify_saves() -> Dict[str, Any]:
    # 1. Real 2002 CD full-CG KhDLsave.dat
    real_bytes = open(REAL_FULLCG_SAVE, "rb").read()
    real_sha = hashlib.sha256(real_bytes).hexdigest()
    parsed_2002 = parse_2002_cd_global_save(real_bytes)
    rebuilt_2002 = build_2002_cd_global_save(parsed_2002)
    rebuilt_2002_sha = hashlib.sha256(rebuilt_2002).hexdigest()
    assert rebuilt_2002_sha == real_sha, "2002 CD KhDLsave.dat round-trip SHA-256 mismatch!"

    nz_vars = [(7000 + i, v) for i, v in enumerate(parsed_2002["vars_7000_9999"]) if v != 0]
    nz_kidoku_scripts = [
        (sid, sum(bin(b).count("1") for b in row))
        for sid, row in enumerate(parsed_2002["kidoku_rows"])
        if any(row)
    ]

    # 2. 2007 DL KhDLsave.dat round-trip self-test (exact defaults from 0x41d9d0)
    cfg = bytearray(0x814)
    struct.pack_into("<H", cfg, 0x00, 0)
    struct.pack_into("<H", cfg, 0x02, 1)
    struct.pack_into("<H", cfg, 0x04, 1)
    struct.pack_into("<H", cfg, 0x06, 1)
    struct.pack_into("<H", cfg, 0x08, 1)
    struct.pack_into("<H", cfg, 0x0A, 255)
    struct.pack_into("<H", cfg, 0x0C, 255)
    struct.pack_into("<H", cfg, 0x0E, 255)
    struct.pack_into("<H", cfg, 0x12, 1)
    struct.pack_into("<H", cfg, 0x14, 0)
    struct.pack_into("<H", cfg, 0x16, 0)
    struct.pack_into("<H", cfg, 0x18, 0x20)
    struct.pack_into("<H", cfg, 0x1A, 1)
    struct.pack_into("<H", cfg, 0x1C, 1500)
    struct.pack_into("<H", cfg, 0x36, 3)
    slot0 = b"TestGlobalStr\x00" + b"\x00" * (201 - 14)
    cfg[0x3A : 0x3A + 201] = slot0
    dl_vars = [0] * 3000
    dl_vars[0] = 1     # vars[7000] = extra omake unlock
    dl_vars[2000] = 1  # vars[9000] = CG flag
    dl_kidoku = {1102: bytes([0xFF, 0x0F, 0, 0]), 2058: bytes([0x55, 0xAA])}
    dl_gflags = {1: bytes([0xFF] * 16)}
    dl_global_raw = (
        bytes(cfg)
        + struct.pack("<3000h", *dl_vars)
        + _write_bitmap_map(dl_kidoku)
        + _write_bitmap_map(dl_gflags)
    )
    dl_global_sha = hashlib.sha256(dl_global_raw).hexdigest()
    dl_global_rebuilt = build_2007_dl_global_save(parse_2007_dl_global_save(dl_global_raw))
    assert hashlib.sha256(dl_global_rebuilt).hexdigest() == dl_global_sha

    # 3. 2007 DL KhDLsave%02d.dat (75,026 B) round-trip self-test
    snap = bytearray(0x5BC4)
    struct.pack_into("<i", snap, 0x00, 4)        # DAT_00471b8c bgm_id
    struct.pack_into("<i", snap, 0x04, 0)        # DAT_00471b90 se_id
    struct.pack_into("<H", snap, 0x12, 1102)     # DAT_00471b9e script_id[0]
    struct.pack_into("<I", snap, 0x28, 0x0124)   # DAT_00471bb4 return_pc[0]
    struct.pack_into("<H", snap, 0x50, 1)        # DAT_00471bdc call_depth
    title = "テスト".encode("cp932") + b"\x00" * (26 - 6)
    dl_slot_raw = (
        struct.pack("<8H", 2026, 11, 20, 17, 30, 1, 2, 3)
        + title
        + bytes(snap)
        + struct.pack("<7000h", *([42] * 7000))
        + bytes(snap)
        + struct.pack("<7000h", *([7] * 7000))
    )
    assert len(dl_slot_raw) == 75026
    dl_slot_sha = hashlib.sha256(dl_slot_raw).hexdigest()
    dl_slot_rebuilt = build_2007_dl_slot_save(parse_2007_dl_slot_save(dl_slot_raw))
    assert hashlib.sha256(dl_slot_rebuilt).hexdigest() == dl_slot_sha

    return {
        "real_fullcg_save": {
            "path": "game/腐姫ＤＬ/补丁/全CG档/KhDLsave.dat",
            "size_bytes": len(real_bytes),
            "format": "2002_cd_global",
            "sha256": real_sha,
            "roundtrip_sha256": rebuilt_2002_sha,
            "roundtrip_match": rebuilt_2002_sha == real_sha,
            "header_u16": parsed_2002["header_u16"],
            "nonzero_vars_count": len(nz_vars),
            "nonzero_vars_range": [nz_vars[0][0], nz_vars[-1][0]] if nz_vars else [],
            "nonzero_vars_head": nz_vars[:8],
            "nonzero_kidoku_script_count": len(nz_kidoku_scripts),
            "nonzero_kidoku_max_script_id": max((s for s, _ in nz_kidoku_scripts), default=None),
        },
        "dl_2007_global_selftest": {
            "size_bytes": len(dl_global_raw),
            "sha256": dl_global_sha,
            "roundtrip_match": True,
        },
        "dl_2007_slot_selftest": {
            "size_bytes": len(dl_slot_raw),
            "sha256": dl_slot_sha,
            "roundtrip_match": True,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify Phase 1C audio, caret codes, and saves.")
    parser.add_argument("--json-out", help="Optional path to write JSON verification report")
    args = parser.parse_args()

    print("=== [1/3] Audio Full-Frame Ogg Vorbis Verification (3,084 entries) ===", flush=True)
    audio_res = verify_all_audio()
    print(
        f"Audio: {audio_res['total_passed']}/{audio_res['total_entries']} PASSED, "
        f"{audio_res['total_failed']} failed, {audio_res['total_samples']} PCM samples "
        f"({audio_res['total_duration_sec']}s), {audio_res['total_dummy_pages_stripped']} dummy OggS pages stripped, "
        f"{audio_res['total_holes']} holes, {audio_res['total_fact_mismatches']} fact mismatches "
        f"({audio_res['elapsed_sec']}s)",
        flush=True,
    )
    if audio_res["total_failed"] != 0 or audio_res["total_entries"] != 3084:
        print("AUDIO VERIFICATION FAILED:", audio_res["failures"], file=sys.stderr)
        return 1

    print("=== [2/3] Text Caret (^) Control Code Scan (341 ORIG + 341 CHS) ===", flush=True)
    caret_res = verify_caret_controls()
    for lbl in ("orig", "chs"):
        c = caret_res["corpora"][lbl]
        print(
            f"Caret [{lbl.upper()}]: {c['scripts_scanned']} scripts, {c['total_caret_tokens']} tokens "
            f"({c['unique_tokens']} unique), handlers={c['handler_counts']}, "
            f"unsupported={len(c['unsupported_hits'])}, missing_gaiji={len(c['missing_gaiji_assets'])}",
            flush=True,
        )

    print("=== [3/3] Save File Byte-Level Parse & Round-Trip SHA-256 ===", flush=True)
    save_res = verify_saves()
    r = save_res["real_fullcg_save"]
    print(
        f"Save [real 2002 CD 全CG档]: {r['size_bytes']} B, sha256={r['sha256'][:16]}..., "
        f"roundtrip_match={r['roundtrip_match']}, nonzero_vars={r['nonzero_vars_count']} "
        f"({r['nonzero_vars_range']}), nonzero_kidoku_scripts={r['nonzero_kidoku_script_count']} "
        f"(max script_id={r['nonzero_kidoku_max_script_id']})",
        flush=True,
    )
    print(
        f"Save [2007 DL self-tests]: global={save_res['dl_2007_global_selftest']['size_bytes']} B "
        f"(match={save_res['dl_2007_global_selftest']['roundtrip_match']}), "
        f"slot={save_res['dl_2007_slot_selftest']['size_bytes']} B "
        f"(match={save_res['dl_2007_slot_selftest']['roundtrip_match']})",
        flush=True,
    )

    report = {
        "audio_verification": audio_res,
        "caret_verification": caret_res,
        "save_verification": save_res,
    }
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as f:
            json.dump(report, f, indent=2, ensure_ascii=False)
        print(f"Wrote JSON report -> {args.json_out}", flush=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
