//! 2D software compositor (`KhimeDL.exe` `0x435b40`..`0x43b980`),
//! Porter-Duff `Over` (`0x43abc0` / `0x401890`), and in-place surface filters (`0x434c30`..`0x435700`).

use crate::gfx::cmath::{
    get_embos_table, get_white_table, lerp_rgb, porter_duff_over, sat_add_rgb, scale_rgb,
    ADD_TABLE, DEL_TABLE, DITHER_TABLE_12, DITHER_TABLE_13,
};
use crate::gfx::surface::{Surface, GAME_HEIGHT, GAME_WIDTH};

/// Integer rectangle `[left, top, right, bottom)` (exclusive right/bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// Construct a rectangle from bounds.
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Construct a full-surface rectangle `[0, 0, w, h)`.
    pub const fn from_size(width: u32, height: u32) -> Self {
        Self {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        }
    }
}

/// Clipped blit coordinates produced by [`clip_rects`] (`FUN_00435b40`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClippedRegion {
    pub dst_left: i32,
    pub dst_top: i32,
    pub src_left: i32,
    pub src_top: i32,
    pub copy_width: i32,
    pub copy_height: i32,
}

/// Parameters attached to a source surface during [`blit`] (`0x435770..0x4359e0`, `0x436430`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlitParams {
    /// Source intrinsic X offset (`+0x1C`).
    pub offset_x: i32,
    /// Source intrinsic Y offset (`+0x20`).
    pub offset_y: i32,
    /// Blend mode (`+0x28`).
    pub blend_mode: u32,
    /// Global alpha / step / threshold parameter (`+0x2C`, typically `0=opaque..=255=transparent`).
    pub global_alpha: i32,
    /// Sub-mode (`+0x30`, `alpha_channel_mode`).
    pub alpha_channel_mode: i32,
    /// Rule / cutoff threshold (`+0x34`).
    pub threshold: i32,
    /// `0x00RRGGBB` fill color or `(cy << 16) | cx` center coordinate (`+0x38`).
    pub color_or_center: u32,
}

/// Compute clipped destination and source rectangles matching `FUN_00435b40`.
///
/// Returns `None` if `copy_width < 0 || copy_height < 0`. When `copy_width == 0 || copy_height == 0`,
/// returns `Some(...)` with zero extent so callers perform zero pixel writes and return `true`.
#[allow(clippy::too_many_arguments)]
pub fn clip_rects(
    src_w: i32,
    src_h: i32,
    dst_w: i32,
    dst_h: i32,
    offset_x: i32,
    offset_y: i32,
    dst_pt: (i32, i32),
    clip_rect: Rect,
) -> Option<ClippedRegion> {
    let cl = clip_rect.left.max(0);
    let ct = clip_rect.top.max(0);
    let cr = clip_rect.right.min(dst_w);
    let cb = clip_rect.bottom.min(dst_h);

    let sx0 = offset_x + dst_pt.0;
    let sy0 = offset_y + dst_pt.1;

    let dst_l = cl.max(sx0);
    let dst_t = ct.max(sy0);
    let dst_r = cr.min(sx0 + src_w);
    let dst_b = cb.min(sy0 + src_h);

    let src_l = (cl - sx0).max(0);
    let src_t = (ct - sy0).max(0);

    let copy_width = dst_r - dst_l;
    let copy_height = dst_b - dst_t;
    if copy_width < 0 || copy_height < 0 {
        return None;
    }

    Some(ClippedRegion {
        dst_left: dst_l,
        dst_top: dst_t,
        src_left: src_l,
        src_top: src_t,
        copy_width,
        copy_height,
    })
}

#[inline]
fn read_px(buf: &[u8], stride_px: usize, x: usize, y: usize) -> [u8; 4] {
    let off = (y * stride_px + x) * 4;
    [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]
}

#[inline]
fn write_px(buf: &mut [u8], stride_px: usize, x: usize, y: usize, px: [u8; 4]) {
    let off = (y * stride_px + x) * 4;
    buf[off..off + 4].copy_from_slice(&px);
}

#[inline]
fn half_blend_px(s: [u8; 4], d: [u8; 4]) -> [u8; 4] {
    let su = u32::from_le_bytes(s);
    let du = u32::from_le_bytes(d);
    let mixed = ((su >> 1) & 0x7f7f_7f7f) + ((du >> 1) & 0x7f7f_7f7f);
    mixed.to_le_bytes()
}

/// Dispatch `Blit` matching `FUN_00436430`.
///
/// Returns `true` if clipping succeeded (`copy_width >= 0 && copy_height >= 0`), `false` if clipped out.
pub fn blit(
    dst: &mut Surface,
    src: &Surface,
    params: &BlitParams,
    dst_pt: (i32, i32),
    clip_rect: Option<Rect>,
) -> bool {
    let clip = clip_rect.unwrap_or_else(|| Rect::from_size(dst.width(), dst.height()));
    let Some(reg) = clip_rects(
        src.width() as i32,
        src.height() as i32,
        dst.width() as i32,
        dst.height() as i32,
        params.offset_x,
        params.offset_y,
        dst_pt,
        clip,
    ) else {
        return false;
    };

    if reg.copy_width == 0 || reg.copy_height == 0 {
        return true;
    }

    let cw = reg.copy_width as usize;
    let ch = reg.copy_height as usize;
    let dst_w = dst.width() as usize;
    let src_w = src.width() as usize;
    let dst_l = reg.dst_left as usize;
    let dst_t = reg.dst_top as usize;
    let src_l = reg.src_left as usize;
    let src_t = reg.src_top as usize;

    let sub = params.alpha_channel_mode;
    let mode = params.blend_mode;

    // Modes with explicit `alpha_channel_mode == 1` no-op in `0x436430`.
    if sub == 1
        && matches!(
            mode,
            3 | 4 | 5 | 8 | 0xB | 0xC | 0xE | 0xF | 0x12 | 0x13 | 0x14 | 0x15
        )
    {
        return true;
    }
    if sub == 0 && matches!(mode, 0xB | 0xC) {
        return true;
    }

    // Mode 8 falls back to Mode 0 when `global_alpha < 5`.
    let effective_mode = if mode == 8 && params.global_alpha < 5 {
        0
    } else {
        mode
    };

    let src_bytes = src.as_bytes();
    let dst_bytes = dst.as_bytes_mut();

    match effective_mode {
        0 => blit_mode_0(
            dst_bytes,
            dst_w,
            dst_l,
            dst_t,
            src_bytes,
            src_w,
            src_l,
            src_t,
            cw,
            ch,
            sub,
            params.threshold,
        ),
        2 => blit_mode_2(
            dst_bytes,
            dst_w,
            dst_l,
            dst_t,
            src_bytes,
            src_w,
            src_l,
            src_t,
            cw,
            ch,
            sub,
            params.global_alpha as u8,
            params.threshold,
        ),
        3 => {
            let ga = params.global_alpha as u8;
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let scaled = scale_rgb(s, ga);
                    let out = if sub == 0 {
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        lerp_rgb(s[3], scaled, d)
                    } else {
                        scaled
                    };
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        4 => {
            let tbl = &DEL_TABLE[(params.global_alpha as u8) as usize];
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let whitened = [
                        tbl[s[0] as usize],
                        tbl[s[1] as usize],
                        tbl[s[2] as usize],
                        0,
                    ];
                    let out = if sub == 0 {
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        lerp_rgb(s[3], whitened, d)
                    } else {
                        whitened
                    };
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        5 => {
            let thr = ((params.global_alpha >> 4) & 0xFFFF) as usize;
            for ly in 0..ch {
                for lx in 0..cw {
                    if ((lx + ly) & 0xF) > thr {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        let out = if sub == 0 {
                            let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                            lerp_rgb(s[3], s, d)
                        } else {
                            s
                        };
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                    }
                }
            }
            true
        }
        6 => {
            let cx = (params.color_or_center & 0xFFFF) as i32;
            let cy = (params.color_or_center >> 16) as i32;
            let r = (params.global_alpha as i64) * 15;
            let r2 = (r * r) as u64;
            for ly in 0..ch {
                let dy = (reg.dst_top + ly as i32 - cy).unsigned_abs() as u64;
                let dy2 = dy * dy;
                for lx in 0..cw {
                    let dx = (reg.dst_left + lx as i32 - cx).unsigned_abs() as u64;
                    if dx * dx + dy2 > r2 {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s);
                    }
                }
            }
            true
        }
        7 => {
            let cx = (params.color_or_center & 0xFFFF) as i32;
            let cy = (params.color_or_center >> 16) as i32;
            let rx = (params.global_alpha as u32).wrapping_mul(22);
            let ry = (params.global_alpha as u32).wrapping_mul(15);
            if rx == 0 || ry == 0 {
                for ly in 0..ch {
                    for lx in 0..cw {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s);
                    }
                }
                return true;
            }
            for ly in 0..ch {
                let ny = ((reg.dst_top + ly as i32 - cy).unsigned_abs() * 1000) / ry;
                let ny2 = ny.saturating_mul(ny);
                for lx in 0..cw {
                    let nx = ((reg.dst_left + lx as i32 - cx).unsigned_abs() * 1000) / rx;
                    if nx.saturating_mul(nx).saturating_add(ny2) > 1_000_000 {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s);
                    }
                }
            }
            true
        }
        8 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let out = if sub == 0 {
                        if s[3] <= 128 {
                            half_blend_px(s, d)
                        } else {
                            lerp_rgb(s[3], s, d)
                        }
                    } else {
                        half_blend_px(s, d)
                    };
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        0xB | 0xC => {
            let fill = if effective_mode == 0xB {
                [0x00, 0x00, 0x00, 0x00]
            } else {
                [0xFF, 0xFF, 0xFF, 0x00]
            };
            let thr = (params.global_alpha as u16) as usize;
            for ly in 0..ch {
                for lx in 0..cw {
                    let out = if (lx + ly) % 10 < thr {
                        fill
                    } else {
                        read_px(src_bytes, src_w, src_l + lx, src_t + ly)
                    };
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        0xD => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let inv = [s[0] ^ 0xFF, s[1] ^ 0xFF, s[2] ^ 0xFF, s[3]];
                    let out = if sub == 0 {
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        lerp_rgb(s[3], inv, d)
                    } else {
                        inv
                    };
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        0xE => {
            let ga = params.global_alpha as u8;
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let a = if sub == 0 { s[3].max(ga) } else { ga };
                    let scaled = scale_rgb(s, a);
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, sat_add_rgb(d, scaled));
                }
            }
            true
        }
        0xF => {
            let ga = params.global_alpha as u8;
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let a = if sub == 0 { s[3].max(ga) } else { ga };
                    let scaled_d = scale_rgb(d, a);
                    write_px(
                        dst_bytes,
                        dst_w,
                        dst_l + lx,
                        dst_t + ly,
                        sat_add_rgb(scaled_d, s),
                    );
                }
            }
            true
        }
        0x12 | 0x13 => {
            let idx = ((params.global_alpha >> 4) & 0xFF) as usize;
            let mask = if idx < 16 {
                if effective_mode == 0x12 {
                    DITHER_TABLE_12[idx]
                } else {
                    DITHER_TABLE_13[idx]
                }
            } else {
                0
            };
            for ly in 0..ch {
                let row_shift = (ly & 3) << 2;
                for lx in 0..cw {
                    let bit = 1u16 << ((lx & 3) + row_shift);
                    if (mask & bit) != 0 {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        let out = if sub == 0 {
                            let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                            lerp_rgb(s[3], s, d)
                        } else {
                            s
                        };
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                    }
                }
            }
            true
        }
        0x14 => {
            let thr = ((params.global_alpha >> 4) & 0xFFFF) as usize;
            for ly in 0..ch {
                if (ly & 0xF) <= thr {
                    continue;
                }
                for lx in 0..cw {
                    if sub == 0 {
                        if ((lx + ly) & 0xF) > thr {
                            let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                            let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                            write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, lerp_rgb(s[3], s, d));
                        }
                    } else {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s);
                    }
                }
            }
            true
        }
        0x15 => {
            let thr = ((params.global_alpha >> 4) & 0xFFFF) as usize;
            for ly in 0..ch {
                for lx in 0..cw {
                    if (lx & 0xF) > thr {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        let out = if sub == 0 {
                            let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                            lerp_rgb(s[3], s, d)
                        } else {
                            s
                        };
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                    }
                }
            }
            true
        }
        0x16 => {
            let c = (params.color_or_center & 0x00FF_FFFF).to_le_bytes();
            let ga = params.global_alpha as u8;
            for ly in 0..ch {
                for lx in 0..cw {
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let t_c = lerp_rgb(ga, c, d);
                    let out = if sub == 0 {
                        let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                        lerp_rgb(s[3], t_c, d)
                    } else {
                        t_c
                    };
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        0x67 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    if s[2] == 0 {
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        write_px(
                            dst_bytes,
                            dst_w,
                            dst_l + lx,
                            dst_t + ly,
                            [d[0] ^ 0xFF, d[1] ^ 0xFF, d[2] ^ 0xFF, d[3]],
                        );
                    }
                }
            }
            true
        }
        0x68 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let out = [
                        get_embos_table(s[0])[d[0] as usize],
                        get_embos_table(s[1])[d[1] as usize],
                        get_embos_table(s[2])[d[2] as usize],
                        d[3],
                    ];
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        0x69 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let out = [
                        get_white_table(s[0])[d[0] as usize],
                        get_white_table(s[1])[d[1] as usize],
                        get_white_table(s[2])[d[2] as usize],
                        d[3],
                    ];
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        0x6A => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let out = [
                        ADD_TABLE[s[0] as usize][d[0] as usize],
                        ADD_TABLE[s[1] as usize][d[1] as usize],
                        ADD_TABLE[s[2] as usize][d[2] as usize],
                        d[3],
                    ];
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
                }
            }
            true
        }
        0x6B => {
            let col = (params.color_or_center & 0x00FF_FFFF).to_le_bytes();
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let a = (0xFF - (((s[2] as i32) * params.global_alpha) >> 8)).clamp(0, 255)
                        as u8;
                    let glow = scale_rgb(col, a);
                    write_px(
                        dst_bytes,
                        dst_w,
                        dst_l + lx,
                        dst_t + ly,
                        sat_add_rgb(d, glow),
                    );
                }
            }
            true
        }
        0x6C => {
            let tbl = &ADD_TABLE[(params.global_alpha as u8) as usize];
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    if s[2] != 0 {
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        write_px(
                            dst_bytes,
                            dst_w,
                            dst_l + lx,
                            dst_t + ly,
                            [
                                tbl[d[0] as usize],
                                tbl[d[1] as usize],
                                tbl[d[2] as usize],
                                d[3],
                            ],
                        );
                    }
                }
            }
            true
        }
        0x6D => {
            let tbl = &DEL_TABLE[(params.global_alpha as u8) as usize];
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    if s[2] != 0 {
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        write_px(
                            dst_bytes,
                            dst_w,
                            dst_l + lx,
                            dst_t + ly,
                            [
                                tbl[d[0] as usize],
                                tbl[d[1] as usize],
                                tbl[d[2] as usize],
                                d[3],
                            ],
                        );
                    }
                }
            }
            true
        }
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_mode_0(
    dst_bytes: &mut [u8],
    dst_w: usize,
    dst_l: usize,
    dst_t: usize,
    src_bytes: &[u8],
    src_w: usize,
    src_l: usize,
    src_t: usize,
    cw: usize,
    ch: usize,
    sub: i32,
    threshold: i32,
) -> bool {
    match sub {
        0 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    match s[3] {
                        0xFF => {}
                        0x00 => write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s),
                        sa => {
                            let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                            write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, lerp_rgb(sa, s, d));
                        }
                    }
                }
            }
        }
        1 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    if (s[3] as i32) < threshold {
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s);
                    }
                }
            }
        }
        2 => {
            let t = (threshold as u16) as u32;
            let u = (t + 31).min(254);
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let sa = s[3] as u32;
                    if sa > u {
                        continue;
                    }
                    if sa <= t {
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s);
                    } else {
                        let a = ((sa - t) * 8) as u8;
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, lerp_rgb(a, s, d));
                    }
                }
            }
        }
        3 => {
            let t = (threshold as u16) as u32;
            let u = (t + 31).min(254);
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let sa = s[3] as u32;
                    if sa > u {
                        continue;
                    }
                    if sa <= t {
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, [0, 0, 0, 0]);
                    } else {
                        let a = ((sa - t) * 8) as u8;
                        let tb = &ADD_TABLE[(!a) as usize];
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        write_px(
                            dst_bytes,
                            dst_w,
                            dst_l + lx,
                            dst_t + ly,
                            [tb[d[0] as usize], tb[d[1] as usize], tb[d[2] as usize], 0],
                        );
                    }
                }
            }
        }
        4 => {
            let t = (threshold as u16) as u32;
            let u = (t + 31).min(254);
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let sa = s[3] as u32;
                    if sa > u {
                        continue;
                    }
                    if sa <= t {
                        write_px(
                            dst_bytes,
                            dst_w,
                            dst_l + lx,
                            dst_t + ly,
                            [0xFF, 0xFF, 0xFF, 0],
                        );
                    } else {
                        let a = ((sa - t) * 8) as u8;
                        let w = ADD_TABLE[a as usize][255];
                        let tb = &ADD_TABLE[(!a) as usize];
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        write_px(
                            dst_bytes,
                            dst_w,
                            dst_l + lx,
                            dst_t + ly,
                            [
                                w.wrapping_add(tb[d[0] as usize]),
                                w.wrapping_add(tb[d[1] as usize]),
                                w.wrapping_add(tb[d[2] as usize]),
                                0,
                            ],
                        );
                    }
                }
            }
        }
        5 => {
            let t = (threshold as u16) as u32;
            let u = (t + 31).min(254);
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let sa = s[3] as u32;
                    if sa > u {
                        continue;
                    }
                    if sa <= t {
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, s);
                    } else {
                        let a = (((sa - t) << 3) & 0xFF) as u8;
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        let scaled = scale_rgb(s, a);
                        write_px(
                            dst_bytes,
                            dst_w,
                            dst_l + lx,
                            dst_t + ly,
                            sat_add_rgb(d, scaled),
                        );
                    }
                }
            }
        }
        _ => {
            // 0x437250: SRCCOPY
            let row_bytes = cw * 4;
            for ly in 0..ch {
                let s_off = ((src_t + ly) * src_w + src_l) * 4;
                let d_off = ((dst_t + ly) * dst_w + dst_l) * 4;
                dst_bytes[d_off..d_off + row_bytes]
                    .copy_from_slice(&src_bytes[s_off..s_off + row_bytes]);
            }
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn blit_mode_2(
    dst_bytes: &mut [u8],
    dst_w: usize,
    dst_l: usize,
    dst_t: usize,
    src_bytes: &[u8],
    src_w: usize,
    src_l: usize,
    src_t: usize,
    cw: usize,
    ch: usize,
    sub: i32,
    ga: u8,
    threshold: i32,
) -> bool {
    match sub {
        0 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    let eff_a = s[3].max(ga);
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, lerp_rgb(eff_a, s, d));
                }
            }
        }
        1 => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    if (s[3] as i32) < threshold {
                        let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                        write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, lerp_rgb(ga, s, d));
                    }
                }
            }
        }
        _ => {
            for ly in 0..ch {
                for lx in 0..cw {
                    let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
                    let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
                    write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, lerp_rgb(ga, s, d));
                }
            }
        }
    }
    true
}

/// Independent Porter-Duff `Over` blit (`0x4371a0 -> 0x43abc0 -> 0x401890`) preserving destination alpha.
pub fn blit_porter_duff_over(
    dst: &mut Surface,
    src: &Surface,
    src_offset: (i32, i32),
    dst_pt: (i32, i32),
    clip_rect: Option<Rect>,
) -> bool {
    let clip = clip_rect.unwrap_or_else(|| Rect::from_size(dst.width(), dst.height()));
    let Some(reg) = clip_rects(
        src.width() as i32,
        src.height() as i32,
        dst.width() as i32,
        dst.height() as i32,
        src_offset.0,
        src_offset.1,
        dst_pt,
        clip,
    ) else {
        return false;
    };
    if reg.copy_width == 0 || reg.copy_height == 0 {
        return true;
    }

    let cw = reg.copy_width as usize;
    let ch = reg.copy_height as usize;
    let dst_w = dst.width() as usize;
    let src_w = src.width() as usize;
    let dst_l = reg.dst_left as usize;
    let dst_t = reg.dst_top as usize;
    let src_l = reg.src_left as usize;
    let src_t = reg.src_top as usize;

    let src_bytes = src.as_bytes();
    let dst_bytes = dst.as_bytes_mut();

    for ly in 0..ch {
        for lx in 0..cw {
            let s = read_px(src_bytes, src_w, src_l + lx, src_t + ly);
            let d = read_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly);
            let out = porter_duff_over(d, s);
            write_px(dst_bytes, dst_w, dst_l + lx, dst_t + ly, out);
        }
    }
    true
}

/// `0x434c30`: In-place grayscale filter (`Y = (B + G + R) / 3`), preserving Alpha.
pub fn filter_grayscale(surface: &mut Surface) {
    for px in surface.as_bytes_mut().as_chunks_mut::<4>().0 {
        let sum = (px[0] as u64) + (px[1] as u64) + (px[2] as u64);
        let y = ((sum * 0x5555_5556) >> 32) as u8;
        px[0] = y;
        px[1] = y;
        px[2] = y;
    }
}

/// `0x434da0`: In-place brightness attenuation (`pct` in `0..=100`), preserving Alpha.
pub fn filter_darken_pct(surface: &mut Surface, pct: u16) {
    let idx = ((((pct as u32) * 255) / 100) & 0xFF) as usize;
    let tbl = &ADD_TABLE[idx];
    for px in surface.as_bytes_mut().as_chunks_mut::<4>().0 {
        px[0] = tbl[px[0] as usize];
        px[1] = tbl[px[1] as usize];
        px[2] = tbl[px[2] as usize];
    }
}

/// `0x434fc0`: In-place grayscale embos tinting against `color` (`0x00RRGGBB`), preserving Alpha.
pub fn filter_embos_tint(surface: &mut Surface, color: u32) {
    let c = (color & 0x00FF_FFFF).to_le_bytes();
    for px in surface.as_bytes_mut().as_chunks_mut::<4>().0 {
        let y = (((px[0] as u32) + (px[1] as u32) + (px[2] as u32)) / 3) as u8;
        let tbl = get_embos_table(y);
        px[0] = tbl[c[0] as usize];
        px[1] = tbl[c[1] as usize];
        px[2] = tbl[c[2] as usize];
    }
}

/// `0x4351b0`: In-place RGB inversion (`^ 0x00FFFFFF`), preserving Alpha.
pub fn filter_invert_rgb(surface: &mut Surface) {
    for px in surface.as_bytes_mut().as_chunks_mut::<4>().0 {
        px[0] ^= 0xFF;
        px[1] ^= 0xFF;
        px[2] ^= 0xFF;
    }
}

/// `0x435300`: In-place RGB fill (`0x00RRGGBB`), preserving Alpha.
pub fn filter_fill_rgb_keep_alpha(surface: &mut Surface, color: u32) {
    let c = (color & 0x00FF_FFFF).to_le_bytes();
    for px in surface.as_bytes_mut().as_chunks_mut::<4>().0 {
        px[0] = c[0];
        px[1] = c[1];
        px[2] = c[2];
    }
}

/// `0x435700`: In-place 32-bit BGRA clear (`0xAARRGGBB`), overwriting Alpha.
pub fn filter_fill_argb(surface: &mut Surface, color: u32) {
    surface.fill_u32(color);
}

/// Nearest-neighbor `StretchBlt(..., SRCCOPY)` matching Win32 GDI scaling used by `0x446440` and `0x446590`.
pub fn stretch_blit_srccopy(
    dst: &mut Surface,
    dst_rect: (i32, i32, i32, i32),
    src: &Surface,
    src_rect: (i32, i32, i32, i32),
) {
    let (dx, dy, dw, dh) = dst_rect;
    let (sx, sy, sw, sh) = src_rect;
    if dw <= 0 || dh <= 0 || sw <= 0 || sh <= 0 {
        return;
    }
    let dst_surf_w = dst.width() as i32;
    let dst_surf_h = dst.height() as i32;
    let src_surf_w = src.width() as i32;
    let src_surf_h = src.height() as i32;

    let src_bytes = src.as_bytes();
    let dst_bytes = dst.as_bytes_mut();
    let dst_stride = dst_surf_w as usize;
    let src_stride = src_surf_w as usize;

    let x_start = (-dx).max(0);
    let x_end = dw.min(dst_surf_w - dx);
    if x_start >= x_end {
        return;
    }
    let x_map: Vec<(usize, usize)> = (x_start..x_end)
        .map(|px| {
            let out_x = (dx + px) as usize;
            let sample_x = (sx + ((px as i64 * sw as i64) / dw as i64) as i32)
                .clamp(0, src_surf_w - 1) as usize;
            (out_x * 4, sample_x * 4)
        })
        .collect();

    for py in 0..dh {
        let out_y = dy + py;
        if out_y < 0 || out_y >= dst_surf_h {
            continue;
        }
        let sample_y = (sy + ((py as i64 * sh as i64) / dh as i64) as i32)
            .clamp(0, src_surf_h - 1) as usize;
        let src_row_off = sample_y * src_stride * 4;
        let dst_row_off = (out_y as usize) * dst_stride * 4;
        for &(dst_x_off, src_x_off) in &x_map {
            let d = dst_row_off + dst_x_off;
            let s = src_row_off + src_x_off;
            dst_bytes[d..d + 4].copy_from_slice(&src_bytes[s..s + 4]);
        }
    }
}

/// Offscreen `800x600` canvas group mirroring `KhimeDL.exe` screen buffers
/// (`front` `[esi+0x9c]`, `staging` `[esi+0xa0]`, `snapshot` `[esi+0xa4]`, `new_scene`).
#[derive(Debug, Clone)]
pub struct Canvas800x600 {
    pub front: Surface,
    pub staging: Surface,
    pub snapshot: Surface,
    pub new_scene: Surface,
}

impl Canvas800x600 {
    /// Allocate a clean `800x600` offscreen canvas set (black opaque `0x00000000`).
    pub fn new() -> Self {
        Self {
            front: Surface::new(GAME_WIDTH, GAME_HEIGHT).expect("800x600 surface valid"),
            staging: Surface::new(GAME_WIDTH, GAME_HEIGHT).expect("800x600 surface valid"),
            snapshot: Surface::new(GAME_WIDTH, GAME_HEIGHT).expect("800x600 surface valid"),
            new_scene: Surface::new(GAME_WIDTH, GAME_HEIGHT).expect("800x600 surface valid"),
        }
    }

    /// Blit `src` onto `new_scene` using `params` at `dst_pt`.
    pub fn compose_to_new_scene(
        &mut self,
        src: &Surface,
        params: &BlitParams,
        dst_pt: (i32, i32),
        clip_rect: Option<Rect>,
    ) -> bool {
        blit(&mut self.new_scene, src, params, dst_pt, clip_rect)
    }

    /// Instantaneously present `new_scene` onto `front` (`SRCCOPY`, `0x4461b0`).
    pub fn commit_new_scene(&mut self) {
        self.front
            .as_bytes_mut()
            .copy_from_slice(self.new_scene.as_bytes());
    }
}

impl Default for Canvas800x600 {
    fn default() -> Self {
        Self::new()
    }
}
