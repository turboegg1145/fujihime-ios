#!/usr/bin/env python3
"""Read-only full asset decoder & verifier for KhimeDL.exe graphics formats.

Verifies every .wcg, .lwg (including all embedded .wcg layers), .msk, and .bmp
across both the original Japanese game archives and the CHS V1.0 patch assets.
Strictly adheres to the reverse-engineered KhimeDL.exe specifications:
  - .xfl: FUN_00431e60
  - .wcg: FUN_0042f3b0 / FUN_0042f450 / FUN_0042f520 / FUN_0042f700 / FUN_00432ec0 /
          FUN_00433070 / FUN_00433380 / FUN_00433510 / FUN_004337f0 / FUN_00433b40
  - .lwg: FUN_0042b640 / FUN_0042b6e0 / FUN_0042bd00 / FUN_0042e420
  - .msk: FUN_00430700 / FUN_004307a0 / FUN_00430900..FUN_00430c10
  - .bmp: FUN_00430cd0 / FUN_00430d80 / FUN_00430f20..FUN_00431400
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import sys
import time
from typing import Dict, List, Tuple


DEFAULT_GAME_DIR = "/workspace/game/腐姫ＤＬ"


def read_xfl_entries(path: str) -> List[Tuple[str, int, int]]:
    with open(path, "rb") as f:
        hdr = f.read(12)
        if len(hdr) != 12:
            raise ValueError(f"Short XFL header in {path}")
        magic, dir_size, count = struct.unpack("<4sII", hdr)
        if magic != b"LB\x01\x00":
            raise ValueError(f"Bad XFL magic {magic!r} in {path}")
        entries = []
        base_off = 12 + dir_size
        for _ in range(count):
            rec = f.read(40)
            if len(rec) != 40:
                raise ValueError(f"Truncated XFL directory in {path}")
            name_raw, off, length = struct.unpack("<32sII", rec)
            name = name_raw.split(b"\x00", 1)[0].decode("cp932", errors="replace")
            entries.append((name, base_off + off, length))
        return entries


def decode_wcg_stream_fast(
    buf: bytearray,
    uncomp_bytes: int,
    comp_data: bytes,
    dict_bytes: bytes,
    dict_cnt: int,
    phase: int,
) -> Tuple[int, int]:
    """Decode one 16-bit half-pixel stream of a WGq\\x02 image (0x433070 / 0x433380)."""
    large = dict_cnt >= 0x1001
    prefix_bits = 4 if large else 3
    prefix_mask = (1 << prefix_bits) - 1
    dict_pairs = [dict_bytes[i * 2 : i * 2 + 2] for i in range(dict_cnt)]
    nbytes = len(comp_data)
    pos = 0
    acc = 0
    bits = 0
    idx = phase  # 1 -> byte offsets +2,+3 (R,A); 0 -> byte offsets +0,+1 (B,G)

    while idx < uncomp_bytes:
        while bits < prefix_bits:
            if pos >= nbytes:
                raise EOFError(f"WCG bitstream overrun at {pos}/{nbytes}")
            acc = (acc << 8) | comp_data[pos]
            pos += 1
            bits += 8
        bits -= prefix_bits
        c = (acc >> bits) & prefix_mask
        acc &= (1 << bits) - 1

        if c == 0:
            while bits < 4:
                if pos >= nbytes:
                    raise EOFError(f"WCG bitstream overrun at {pos}/{nbytes}")
                acc = (acc << 8) | comp_data[pos]
                pos += 1
                bits += 8
            bits -= 4
            run = ((acc >> bits) & 0xF) + 2
            acc &= (1 << bits) - 1

            while bits < prefix_bits:
                if pos >= nbytes:
                    raise EOFError(f"WCG bitstream overrun at {pos}/{nbytes}")
                acc = (acc << 8) | comp_data[pos]
                pos += 1
                bits += 8
            bits -= prefix_bits
            c2 = (acc >> bits) & prefix_mask
            acc &= (1 << bits) - 1
            if c2 == 0:
                raise ValueError("WCG dict code 0 inside RLE")
            code = c2
        else:
            run = 1
            code = c

        if not large:
            if code == 1:
                bcnt = 1
                base = 0
            elif code <= 6:
                bcnt = code - 1
                base = 1 << bcnt
            else:
                bcnt = -1
                for b in range(6, 12):
                    if bits < 1:
                        if pos >= nbytes:
                            raise EOFError(f"WCG bitstream overrun at {pos}/{nbytes}")
                        acc = (acc << 8) | comp_data[pos]
                        pos += 1
                        bits += 8
                    bits -= 1
                    bit1 = (acc >> bits) & 1
                    acc &= (1 << bits) - 1
                    if bit1 == 0:
                        bcnt = b
                        base = 1 << b
                        break
                if bcnt < 0:
                    raise ValueError("Invalid small WCG unary code")
        else:
            if code == 1:
                bcnt = 1
                base = 0
            elif code <= 14:
                bcnt = code - 1
                base = 1 << bcnt
            else:
                bcnt = -1
                for b in (14, 15):
                    if bits < 1:
                        if pos >= nbytes:
                            raise EOFError(f"WCG bitstream overrun at {pos}/{nbytes}")
                        acc = (acc << 8) | comp_data[pos]
                        pos += 1
                        bits += 8
                    bits -= 1
                    bit1 = (acc >> bits) & 1
                    acc &= (1 << bits) - 1
                    if bit1 == 0:
                        bcnt = b
                        base = 1 << b
                        break
                if bcnt < 0:
                    raise ValueError("Invalid large WCG unary code")

        while bits < bcnt:
            if pos >= nbytes:
                raise EOFError(f"WCG bitstream overrun at {pos}/{nbytes}")
            acc = (acc << 8) | comp_data[pos]
            pos += 1
            bits += 8
        bits -= bcnt
        di = ((acc >> bits) & ((1 << bcnt) - 1)) | base
        acc &= (1 << bits) - 1

        pair = dict_pairs[di]
        p0 = pair[0]
        p1 = pair[1]
        if run == 1:
            off = idx * 2
            buf[off] = p0
            buf[off + 1] = p1
            idx += 2
        else:
            for _ in range(run):
                if idx >= uncomp_bytes:
                    raise ValueError(f"WCG RLE output overflow: idx={idx} >= {uncomp_bytes}")
                off = idx * 2
                buf[off] = p0
                buf[off + 1] = p1
                idx += 2

    return idx, pos


def decode_wcg_ex(data: bytes) -> Tuple[int, int, bytearray, int]:
    """Decode a WGq\\x02 image (0x42f450 / 0x42f520 / 0x42f700) and return (w, h, buf, trailing_pad_bytes)."""
    if len(data) < 16:
        raise ValueError("Truncated WCG header")
    magic, flags, w, h = struct.unpack_from("<4sIII", data, 0)
    if magic != b"WGq\x02":
        raise ValueError(f"Unexpected WCG magic: {magic!r}")
    magic_hi = struct.unpack_from("<H", data, 2)[0]
    if (magic_hi & 0xF) != 1 or (magic_hi & 0x1C0) != 0x40:
        raise ValueError(f"Unexpected WCG magic_hi: 0x{magic_hi:04x}")
    if (flags & 0xFF) != 0x20:
        raise ValueError(f"Unexpected WCG bpp/flags: 0x{flags:08x}")
    if w == 0 or h == 0:
        raise ValueError(f"Invalid WCG dimensions: {w}x{h}")

    expected_stream_bytes = w * h * 2
    buf = bytearray(w * h * 4)
    pos = 16
    trailing_pad = 0
    for phase in (1, 0):
        if pos + 12 > len(data):
            raise ValueError("Truncated WCG stream header")
        uncomp_len, comp_len, dict_field = struct.unpack_from("<III", data, pos)
        pos += 12
        dict_cnt = dict_field & 0xFFFF
        if uncomp_len != expected_stream_bytes:
            raise ValueError(
                f"WCG stream uncomp_len={uncomp_len} != expected {expected_stream_bytes}"
            )
        dict_byte_len = dict_cnt * 2
        if pos + dict_byte_len + comp_len > len(data):
            raise ValueError("Truncated WCG dictionary or compressed payload")
        dict_bytes = data[pos : pos + dict_byte_len]
        pos += dict_byte_len
        comp_data = data[pos : pos + comp_len]
        pos += comp_len
        idx, bytes_consumed = decode_wcg_stream_fast(
            buf, uncomp_len, comp_data, dict_bytes, dict_cnt, phase
        )
        if idx != expected_stream_bytes + phase:
            raise ValueError(f"WCG stream phase {phase} ended at idx={idx}, expected {expected_stream_bytes + phase}")
        if bytes_consumed > comp_len:
            raise ValueError(f"WCG stream consumed {bytes_consumed} > comp_len {comp_len}")
        if bytes_consumed < comp_len:
            rem = comp_data[bytes_consumed:]
            if len(rem) > 1 or any(b != 0 for b in rem):
                raise ValueError(
                    f"WCG stream non-zero/excess trailing bytes: consumed {bytes_consumed} != comp_len {comp_len} ({rem.hex()})"
                )
            trailing_pad += len(rem)
    if pos != len(data):
        raise ValueError(f"Trailing bytes in WCG: {pos} != {len(data)}")
    return w, h, buf, trailing_pad


def decode_wcg(data: bytes) -> Tuple[int, int, bytearray]:
    w, h, buf, _ = decode_wcg_ex(data)
    return w, h, buf


def decode_msk(data: bytes) -> Tuple[int, int, bytearray]:
    """Decode .msk (0x4307a0 / 0x430900..0x430c10) into top-down 8-bit alpha mask."""
    if len(data) < 54 or data[:2] != b"BM":
        raise ValueError("Invalid BMP/MSK signature")
    bf_off = struct.unpack_from("<I", data, 10)[0]
    bi_size, w, h, planes, bpp, comp = struct.unpack_from("<IiiHHI", data, 14)
    if bi_size != 40 or planes != 1 or comp != 0 or w <= 0 or h <= 0:
        raise ValueError(f"Unsupported MSK header: size={bi_size} {w}x{h} planes={planes} bpp={bpp} comp={comp}")
    if bpp not in (1, 8, 24, 32):
        raise ValueError(f"Unsupported MSK bpp={bpp}")

    pos = 54
    palette = None
    if bpp <= 8:
        clr_used = struct.unpack_from("<I", data, 46)[0]
        if clr_used == 0:
            clr_used = 2 if bpp == 1 else 256
        palette = data[pos : pos + clr_used * 4]
        pos += clr_used * 4

    row_stride = ((w * bpp + 7) // 8 + 3) & ~3
    if pos + row_stride * h > len(data):
        raise ValueError(f"Truncated MSK pixel data: need {pos + row_stride * h}, have {len(data)}")

    mask_out = bytearray(w * h)
    for row in range(h):
        src_row = data[pos + row * row_stride : pos + (row + 1) * row_stride]
        dst_y = h - 1 - row
        dst_off = dst_y * w
        if bpp == 1:
            for x in range(w):
                mask_out[dst_off + x] = 0xFF if (src_row[x >> 3] & (0x80 >> (x & 7))) else 0x00
        elif bpp == 8:
            for x in range(w):
                mask_out[dst_off + x] = palette[src_row[x] * 4]
        elif bpp == 24:
            for x in range(w):
                mask_out[dst_off + x] = src_row[x * 3]  # Blue channel per FUN_00430b50
        elif bpp == 32:
            for x in range(w):
                mask_out[dst_off + x] = src_row[x * 4]  # Blue channel per FUN_00430c10
    return w, h, mask_out


def decode_bmp(data: bytes) -> Tuple[int, int, int, bytearray]:
    """Decode .bmp (0x430d80 / 0x430f20..0x431300) into top-down 32-bit BGRA."""
    if len(data) < 54 or data[:2] != b"BM":
        raise ValueError("Invalid BMP signature")
    bi_size, w, h, planes, bpp, comp = struct.unpack_from("<IiiHHI", data, 14)
    if bi_size != 40 or planes != 1 or comp != 0 or w <= 0 or h <= 0:
        raise ValueError(f"Unsupported BMP header: size={bi_size} {w}x{h} planes={planes} bpp={bpp} comp={comp}")
    if bpp not in (1, 8, 24, 32):
        raise ValueError(f"Unsupported BMP bpp={bpp}")

    pos = 54
    palette = None
    if bpp <= 8:
        clr_used = struct.unpack_from("<I", data, 46)[0]
        if clr_used == 0:
            clr_used = 2 if bpp == 1 else 256
        palette = data[pos : pos + clr_used * 4]
        pos += clr_used * 4

    row_stride = ((w * bpp + 7) // 8 + 3) & ~3
    if pos + row_stride * h > len(data):
        raise ValueError(f"Truncated BMP pixel data: need {pos + row_stride * h}, have {len(data)}")

    out = bytearray(w * h * 4)
    for row in range(h):
        src_row = data[pos + row * row_stride : pos + (row + 1) * row_stride]
        dst_y = h - 1 - row
        dst_base = dst_y * w * 4
        if bpp == 32:
            out[dst_base : dst_base + w * 4] = src_row[: w * 4]
        elif bpp == 24:
            for x in range(w):
                s = x * 3
                d = dst_base + x * 4
                out[d] = src_row[s]
                out[d + 1] = src_row[s + 1]
                out[d + 2] = src_row[s + 2]
                out[d + 3] = 0
        elif bpp == 8:
            for x in range(w):
                p = src_row[x] * 4
                d = dst_base + x * 4
                out[d] = palette[p]
                out[d + 1] = palette[p + 1]
                out[d + 2] = palette[p + 2]
                out[d + 3] = 0
        elif bpp == 1:
            for x in range(w):
                v = 0xFF if (src_row[x >> 3] & (0x80 >> (x & 7))) else 0x00
                d = dst_base + x * 4
                out[d] = v
                out[d + 1] = v
                out[d + 2] = v
                out[d + 3] = 0
    return w, h, bpp, out


def decode_lwg(data: bytes) -> Dict:
    """Parse .lwg (0x42b6e0) and decode all embedded .wcg layers."""
    if len(data) < 24:
        raise ValueError("Truncated LWG header")
    magic, h, w, cnt, flags, hdr_u16, idx_bytes = struct.unpack_from("<4sIIHHII", data, 0)
    if magic != b"LG\x01\x00":
        raise ValueError(f"Bad LWG magic: {magic!r}")
    pos = 24
    entries = []
    for _ in range(cnt):
        x, y, flg, eoff, elen = struct.unpack_from("<iiBII", data, pos)
        nlen = data[pos + 17]
        name = data[pos + 18 : pos + 18 + nlen].decode("cp932", errors="replace")
        pos += 18 + nlen
        entries.append((name, x, y, flg, eoff, elen))
    if pos - 24 != idx_bytes:
        raise ValueError(f"LWG index size mismatch: parsed {pos - 24} != {idx_bytes}")
    total_data_sz = struct.unpack_from("<I", data, pos)[0]
    pos += 4
    if pos + total_data_sz != len(data):
        raise ValueError(f"LWG payload size mismatch: {pos} + {total_data_sz} != {len(data)}")

    decoded_layers = []
    trailing_pad_total = 0
    for name, x, y, flg, eoff, elen in entries:
        chunk = data[pos + eoff : pos + eoff + elen]
        lw, lh, lbuf, tpad = decode_wcg_ex(chunk)
        trailing_pad_total += tpad
        if len(lbuf) != lw * lh * 4:
            raise AssertionError("Layer length mismatch")
        decoded_layers.append((name, x, y, flg, lw, lh, lbuf))

    return {
        "width": w,
        "height": h,
        "flags": flags,
        "entry_count": cnt,
        "trailing_pad_bytes": trailing_pad_total,
        "layers": decoded_layers,
    }


def _verify_single_asset(task: Tuple[str, str, bytes, bool]) -> Tuple[str, str, int, int, int, int, int, Dict | None]:
    """Worker helper for multiprocessing asset verification."""
    tag, name, raw, want_sample = task
    ext = os.path.splitext(name)[1].lower()
    sample = None
    if ext == ".wcg":
        w, h, buf, tpad = decode_wcg_ex(raw)
        assert len(buf) == w * h * 4
        if want_sample:
            alphas = buf[3::4]
            sample = {
                "width": w,
                "height": h,
                "bytes": len(buf),
                "sha256_prefix": hashlib.sha256(buf).hexdigest()[:16],
                "alpha_or_val_min": min(alphas),
                "alpha_or_val_max": max(alphas),
                "pixel0": list(buf[0:4]),
            }
        return (ext, name, 1, 0, tpad, w, h, sample)
    elif ext == ".lwg":
        lwg = decode_lwg(raw)
        if want_sample:
            first = lwg["layers"][0]
            buf = first[6]
            alphas = buf[3::4]
            sample = {
                "width": first[4],
                "height": first[5],
                "bytes": len(buf),
                "sha256_prefix": hashlib.sha256(buf).hexdigest()[:16],
                "alpha_or_val_min": min(alphas),
                "alpha_or_val_max": max(alphas),
                "pixel0": list(buf[0:4]),
                "canvas_w": lwg["width"],
                "canvas_h": lwg["height"],
                "layers": lwg["entry_count"],
                "first_layer": first[0],
            }
        return (ext, name, 1, lwg["entry_count"], lwg["trailing_pad_bytes"], lwg["width"], lwg["height"], sample)
    elif ext == ".msk":
        w, h, buf = decode_msk(raw)
        assert len(buf) == w * h
        if want_sample:
            sample = {
                "width": w,
                "height": h,
                "bytes": len(buf),
                "sha256_prefix": hashlib.sha256(buf).hexdigest()[:16],
                "alpha_or_val_min": min(buf),
                "alpha_or_val_max": max(buf),
                "pixel0": [buf[0]],
            }
        return (ext, name, 1, 0, 0, w, h, sample)
    elif ext == ".bmp":
        w, h, bpp, buf = decode_bmp(raw)
        assert len(buf) == w * h * 4
        if want_sample:
            alphas = buf[3::4]
            sample = {
                "width": w,
                "height": h,
                "bytes": len(buf),
                "sha256_prefix": hashlib.sha256(buf).hexdigest()[:16],
                "alpha_or_val_min": min(alphas),
                "alpha_or_val_max": max(alphas),
                "pixel0": list(buf[0:4]),
                "bpp": bpp,
            }
        return (ext, name, 1, 0, 0, w, h, sample)
    return (ext, name, 0, 0, 0, 0, 0, None)


def main() -> int:
    from concurrent.futures import ProcessPoolExecutor

    ap = argparse.ArgumentParser(description="Verify all WCG/LWG/MSK/BMP game assets.")
    ap.add_argument("--game-dir", default=DEFAULT_GAME_DIR)
    ap.add_argument("--json-out", default=None, help="Optional JSON report path")
    ap.add_argument("--workers", type=int, default=os.cpu_count() or 1)
    args = ap.parse_args()

    t0 = time.time()
    base = args.game_dir

    orig_xfls = [
        "grpe.xfl",
        "grpo.xfl",
        "grpp.xfl",
        "grps.xfl",
        "grpf.xfl",
        "grpo_ex.xfl",
        "补丁/原版备份/grpo_tp.xfl",
    ]
    chs_xfls = ["grpo_tp.xfl"]
    chs_dirs = ["grpe", "grpo_ex", "grps"]

    stats: Dict = {
        "orig_wcg_count": 0,
        "orig_lwg_count": 0,
        "orig_lwg_layers_count": 0,
        "orig_msk_count": 0,
        "orig_trailing_pad_bytes": 0,
        "chs_wcg_count": 0,
        "chs_bmp_count": 0,
        "chs_lwg_count": 0,
        "chs_lwg_layers_count": 0,
        "chs_trailing_pad_bytes": 0,
        "archives": {},
        "directories": {},
        "samples": {},
    }

    sample_targets = {
        ("grpe.xfl", "0001.wcg"),
        ("grps.xfl", "compane.lwg"),
        ("grps.xfl", "ef14.msk"),
        ("补丁/原版备份/grpo_tp.xfl", "8000.wcg"),
        ("grpo_tp.xfl", "8000.bmp"),
    }

    with ProcessPoolExecutor(max_workers=max(1, args.workers)) as pool:
        # 1. Original XFL archives
        for rel in orig_xfls:
            p = os.path.join(base, rel)
            entries = read_xfl_entries(p)
            tasks = []
            with open(p, "rb") as f:
                for name, off, length in entries:
                    f.seek(off)
                    raw = f.read(length)
                    if len(raw) != length:
                        raise IOError(f"Short read for {rel}/{name}")
                    tasks.append((rel, name, raw, (rel, name) in sample_targets))
            counts = {"wcg": 0, "lwg": 0, "lwg_layers": 0, "msk": 0, "other": 0, "trailing_pad_bytes": 0}
            for ext, name, cnt, layers, tpad, w, h, sample in pool.map(_verify_single_asset, tasks, chunksize=8):
                counts["trailing_pad_bytes"] += tpad
                stats["orig_trailing_pad_bytes"] += tpad
                if ext == ".wcg":
                    counts["wcg"] += cnt
                    stats["orig_wcg_count"] += cnt
                elif ext == ".lwg":
                    counts["lwg"] += cnt
                    counts["lwg_layers"] += layers
                    stats["orig_lwg_count"] += cnt
                    stats["orig_lwg_layers_count"] += layers
                elif ext == ".msk":
                    counts["msk"] += cnt
                    stats["orig_msk_count"] += cnt
                else:
                    counts["other"] += 1
                if sample is not None:
                    stats["samples"][f"{rel}/{name}"] = sample
            stats["archives"][rel] = counts
            print(f"[ORIG] {rel}: {counts}")

        # 2. CHS XFL archive(s)
        for rel in chs_xfls:
            p = os.path.join(base, rel)
            entries = read_xfl_entries(p)
            tasks = []
            with open(p, "rb") as f:
                for name, off, length in entries:
                    f.seek(off)
                    raw = f.read(length)
                    tasks.append((rel, name, raw, (rel, name) in sample_targets))
            counts = {"wcg": 0, "bmp": 0, "other": 0, "trailing_pad_bytes": 0}
            for ext, name, cnt, layers, tpad, w, h, sample in pool.map(_verify_single_asset, tasks, chunksize=8):
                counts["trailing_pad_bytes"] += tpad
                stats["chs_trailing_pad_bytes"] += tpad
                if ext == ".wcg":
                    counts["wcg"] += cnt
                    stats["chs_wcg_count"] += cnt
                elif ext == ".bmp":
                    counts["bmp"] += cnt
                    stats["chs_bmp_count"] += cnt
                else:
                    counts["other"] += 1
                if sample is not None:
                    stats["samples"][f"{rel}/{name}"] = sample
            stats["archives"][f"chs:{rel}"] = counts
            print(f"[CHS ] {rel}: {counts}")

        # 3. CHS scattered override directories
        for drel in chs_dirs:
            dp = os.path.join(base, drel)
            tasks = []
            for fn in sorted(os.listdir(dp)):
                fp = os.path.join(dp, fn)
                if not os.path.isfile(fp):
                    continue
                with open(fp, "rb") as f:
                    raw = f.read()
                tasks.append((drel, fn, raw, False))
            counts = {"wcg": 0, "lwg": 0, "lwg_layers": 0, "other": 0, "trailing_pad_bytes": 0}
            for ext, name, cnt, layers, tpad, w, h, sample in pool.map(_verify_single_asset, tasks, chunksize=8):
                counts["trailing_pad_bytes"] += tpad
                stats["chs_trailing_pad_bytes"] += tpad
                if ext == ".wcg":
                    counts["wcg"] += cnt
                    stats["chs_wcg_count"] += cnt
                elif ext == ".lwg":
                    counts["lwg"] += cnt
                    counts["lwg_layers"] += layers
                    stats["chs_lwg_count"] += cnt
                    stats["chs_lwg_layers_count"] += layers
                else:
                    counts["other"] += 1
            stats["directories"][drel] = counts
            print(f"[CHS ] {drel}/: {counts}")

    # Verify exact expected totals
    assert stats["orig_wcg_count"] == 1727, stats["orig_wcg_count"]
    assert stats["orig_lwg_count"] == 15, stats["orig_lwg_count"]
    assert stats["orig_msk_count"] == 12, stats["orig_msk_count"]
    assert stats["chs_bmp_count"] == 36, stats["chs_bmp_count"]

    # Alpha convention sanity assertions
    bg = stats["samples"]["grpe.xfl/0001.wcg"]
    assert bg["alpha_or_val_min"] == 0 and bg["alpha_or_val_max"] == 0, bg
    ui_orig = stats["samples"]["补丁/原版备份/grpo_tp.xfl/8000.wcg"]
    ui_bmp = stats["samples"]["grpo_tp.xfl/8000.bmp"]
    assert ui_orig["alpha_or_val_min"] == 0 and ui_orig["alpha_or_val_max"] == 255, ui_orig
    assert ui_bmp["alpha_or_val_min"] == 0 and ui_bmp["alpha_or_val_max"] == 255, ui_bmp

    elapsed = time.time() - t0
    stats["elapsed_sec"] = round(elapsed, 3)
    print(
        f"VERIFIED ALL ASSETS IN {elapsed:.2f}s: "
        f"orig_wcg={stats['orig_wcg_count']}, orig_lwg={stats['orig_lwg_count']} "
        f"({stats['orig_lwg_layers_count']} layers), orig_msk={stats['orig_msk_count']}, "
        f"chs_wcg={stats['chs_wcg_count']}, chs_bmp={stats['chs_bmp_count']}, "
        f"chs_lwg={stats['chs_lwg_count']} ({stats['chs_lwg_layers_count']} layers), "
        f"orig_pad={stats['orig_trailing_pad_bytes']}B, chs_pad={stats['chs_trailing_pad_bytes']}B"
    )

    if args.json_out:
        os.makedirs(os.path.dirname(os.path.abspath(args.json_out)), exist_ok=True)
        with open(args.json_out, "w", encoding="utf-8") as jf:
            json.dump(stats, jf, indent=2, ensure_ascii=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
