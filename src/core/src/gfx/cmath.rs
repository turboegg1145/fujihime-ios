//! `CMath` lookup tables, ordered dither masks, and pixel math helpers (`KhimeDL.exe`).

/// `CMath::m_ppAddTable[256][256]` (`0x483820`), initialized at `0x43d6c1..0x43d754`.
///
/// `ADD_TABLE[i][j] = ((j * (256 - i)) >> 8) as u8`.
pub static ADD_TABLE: [[u8; 256]; 256] = {
    let mut table = [[0u8; 256]; 256];
    let mut i = 0usize;
    while i < 256 {
        let weight = 256 - i;
        let mut j = 0usize;
        while j < 256 {
            table[i][j] = ((j * weight) >> 8) as u8;
            j += 1;
        }
        i += 1;
    }
    table
};

/// `CMath::m_ppDelTable[256][256]` (`0x493820`), initialized at `0x43d6c1..0x43d754`.
///
/// `DEL_TABLE[i][j] = (j + (((256 - j) * i) >> 8)) as u8`.
pub static DEL_TABLE: [[u8; 256]; 256] = {
    let mut table = [[0u8; 256]; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut j = 0usize;
        while j < 256 {
            table[i][j] = (j + (((256 - j) * i) >> 8)) as u8;
            j += 1;
        }
        i += 1;
    }
    table
};

/// 4x4 ordered dither bitmask table for `blend_mode = 0x12` (`0x46e100`).
pub const DITHER_TABLE_12: [u16; 16] = [
    0xffdf, 0xefdf, 0x6fdf, 0x6fd7, 0x6bd7, 0x6bd6, 0x6b56, 0x6a56, 0x2a56, 0x2a52, 0x0a52, 0x0a12,
    0x0812, 0x0810, 0x0800, 0x0000,
];

/// 4x4 ordered dither bitmask table for `blend_mode = 0x13` (`0x46e120`).
pub const DITHER_TABLE_13: [u16; 16] = [
    0xfffe, 0xfffc, 0xfff8, 0xfff0, 0xff70, 0xf770, 0x7770, 0x3770, 0x1770, 0x0770, 0x0670, 0x0660,
    0x0640, 0x0600, 0x0200, 0x0000,
];

/// `CMath::GetWhiteTable(p)` (`0x4015f0`): `&DEL_TABLE[p]`.
#[inline]
pub fn get_white_table(p: u8) -> &'static [u8; 256] {
    &DEL_TABLE[p as usize]
}

/// `CMath::GetEmbosTable(p)` (`0x401630`):
/// - `p < 128` -> `&ADD_TABLE[254 - 2 * p]`
/// - `p >= 128` -> `&DEL_TABLE[(p - 128) * 2 + 1]`
#[inline]
pub fn get_embos_table(p: u8) -> &'static [u8; 256] {
    if p < 128 {
        &ADD_TABLE[254 - 2 * (p as usize)]
    } else {
        &DEL_TABLE[((p as usize) - 128) * 2 + 1]
    }
}

/// `0x4016b0`: Scale RGB channels of `color` (BGRA `[b, g, r, _]`) via `ADD_TABLE[alpha]` with `A = 0`.
#[inline]
pub fn scale_rgb(color: [u8; 4], alpha: u8) -> [u8; 4] {
    let tbl = &ADD_TABLE[alpha as usize];
    [
        tbl[color[0] as usize],
        tbl[color[1] as usize],
        tbl[color[2] as usize],
        0,
    ]
}

/// `0x401800`: Linear RGB interpolation `ADD_TABLE[a][fg] + ADD_TABLE[!a][bg]`, packing `A = 0`.
#[inline]
pub fn lerp_rgb(a: u8, fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    let tf = &ADD_TABLE[a as usize];
    let tb = &ADD_TABLE[(!a) as usize];
    [
        tf[fg[0] as usize].wrapping_add(tb[bg[0] as usize]),
        tf[fg[1] as usize].wrapping_add(tb[bg[1] as usize]),
        tf[fg[2] as usize].wrapping_add(tb[bg[2] as usize]),
        0,
    ]
}

/// Saturating per-channel RGB addition preserving `base[3]` (alpha).
#[inline]
pub fn sat_add_rgb(base: [u8; 4], add: [u8; 4]) -> [u8; 4] {
    [
        base[0].saturating_add(add[0]),
        base[1].saturating_add(add[1]),
        base[2].saturating_add(add[2]),
        base[3],
    ]
}

/// `0x401700`: Scale `src` RGB via `ADD_TABLE[alpha]` then saturating-add onto `dst`, preserving `dst[3]`.
#[inline]
pub fn scale_add_rgb(dst: [u8; 4], src: [u8; 4], alpha: u8) -> [u8; 4] {
    let scaled = scale_rgb(src, alpha);
    sat_add_rgb(dst, scaled)
}

/// `0x401890`: Inverted-alpha non-premultiplied Porter-Duff `Over` (`src` over `dst`).
#[inline]
pub fn porter_duff_over(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let s_a = src[3];
    if s_a == 0xFF {
        return dst;
    }
    if s_a == 0x00 {
        return src;
    }

    let sa = (!s_a) as u32;
    let da = (!dst[3]) as u32;
    let rem = ((da * (s_a as u32)) + 0xFF) >> 8;
    let out_opacity = sa + rem;
    let src_w = ((sa * 255) / out_opacity) as u8;
    let t_src = !src_w;
    let t_dst = src_w;

    let ts = &ADD_TABLE[t_src as usize];
    let td = &ADD_TABLE[t_dst as usize];

    [
        ts[src[0] as usize].wrapping_add(td[dst[0] as usize]),
        ts[src[1] as usize].wrapping_add(td[dst[1] as usize]),
        ts[src[2] as usize].wrapping_add(td[dst[2] as usize]),
        !(out_opacity as u8),
    ]
}
