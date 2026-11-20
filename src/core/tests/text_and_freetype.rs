use std::collections::HashSet;
use std::fs;
use std::path::Path;

use fujihime_core::gfx::surface::{Surface, GAME_HEIGHT, GAME_WIDTH};
use fujihime_core::text::{
    classify_cp932_kinsoku, layout_text, mode1_gray5, mode3_inverted_alpha, parse_caret_bytes,
    parse_caret_utf8, quantize_gray8_to_64, rasterize_glyph, render_layout_to_surface,
    scale_font_height, FontFamily, FontLibrary, FontSet, GaijiBank, GlyphRequest, KinsokuClass,
    RasterMode, TextAlign, TextEncoding, TextSpeed, TextStyle, TextToken, TypewriterState,
    KINSOKU_END_SJIS, KINSOKU_START_ASCII, KINSOKU_START_SJIS, TBOX00_TEXT_RECT,
};
use fujihime_core::vfs::{AssetCategory, AssetVfs, LanguageMode, VfsConfig};

const GAME_DIR: &str = "/workspace/game/腐姫ＤＬ";

fn require_real_assets_or_skip(test_name: &str) -> bool {
    if Path::new(GAME_DIR).is_dir() {
        return true;
    }
    if std::env::var("FUJIHIME_REQUIRE_REAL_ASSETS").ok().as_deref() == Some("1") {
        panic!("FUJIHIME_REQUIRE_REAL_ASSETS=1 but real assets missing at {GAME_DIR}");
    }
    eprintln!("[skip] {test_name}: real game assets not found at {GAME_DIR}");
    false
}

fn extract_gsc_strings(gsc: &[u8]) -> Vec<&[u8]> {
    let hdr_sz = u32::from_le_bytes(gsc[4..8].try_into().unwrap()) as usize;
    let code_sz = u32::from_le_bytes(gsc[8..12].try_into().unwrap()) as usize;
    let str_off_sz = u32::from_le_bytes(gsc[12..16].try_into().unwrap()) as usize;
    let str_data_sz = u32::from_le_bytes(gsc[16..20].try_into().unwrap()) as usize;

    let off_base = hdr_sz + code_sz;
    let data_base = off_base + str_off_sz;
    let str_data = &gsc[data_base..data_base + str_data_sz];
    let count = str_off_sz / 4;

    let mut out = Vec::with_capacity(count);
    for idx in 0..count {
        let off = u32::from_le_bytes(
            gsc[off_base + idx * 4..off_base + idx * 4 + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        if off >= str_data.len() {
            continue;
        }
        let end = str_data[off..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| off + p)
            .unwrap_or(str_data.len());
        out.push(&str_data[off..end]);
    }
    out
}

#[test]
fn real_all_341_orig_and_341_chs_gsc_caret_controls_and_11_gaiji() {
    if !require_real_assets_or_skip("real_all_341_orig_and_341_chs_gsc_caret_controls_and_11_gaiji") {
        return;
    }
    let jp_vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();
    let chs_vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::ChineseV1)).unwrap();

    let mut gaiji_bank = GaijiBank::new();
    let loaded_gaiji = gaiji_bank.preload_all_standard(&jp_vfs).unwrap();
    assert_eq!(loaded_gaiji, 11);
    for id in (1..=10u16).chain(std::iter::once(999u16)) {
        let surf = gaiji_bank.get(id).expect("gaiji cached");
        assert_eq!((surf.width(), surf.height()), (186, 34));
    }

    let palette = [0x00FF_FFFF; 10];
    let default_style = TextStyle::default();

    for (label, vfs, enc, exp_caret_total, exp_drops) in [
        ("orig", &jp_vfs, TextEncoding::Cp932, 4719usize, 1usize),
        ("chs", &chs_vfs, TextEncoding::Gbk, 4076usize, 0usize),
    ] {
        let scr = vfs.archive(AssetCategory::Scr).unwrap();
        assert_eq!(scr.len(), 341);

        let mut total_carets = 0usize;
        let mut total_drops = 0usize;
        let mut all_gaiji_ids = HashSet::new();

        for entry in scr.entries() {
            if !entry.name.to_ascii_lowercase().ends_with(".gsc") {
                continue;
            }
            let bytes = scr.read_entry(entry).unwrap();
            for raw_s in extract_gsc_strings(&bytes) {
                let parsed = parse_caret_bytes(raw_s, enc, default_style, 1, None, &palette);
                assert!(
                    parsed.stats.unsupported_seqs.is_empty(),
                    "{label}/{} had unsupported caret tokens: {:?}",
                    entry.name,
                    parsed.stats.unsupported_seqs
                );
                for gid in parsed.stats.gaiji_ids {
                    assert!(
                        gaiji_bank.get(gid).is_some(),
                        "{label}/{} references missing gaiji gf{gid:03}.wcg",
                        entry.name
                    );
                    all_gaiji_ids.insert(gid);
                }
                total_carets += parsed.stats.total_caret_seqs;
                total_drops += parsed.stats.default_drop_2b_count;
            }
        }

        assert_eq!(total_carets, exp_caret_total, "{label} caret count mismatch");
        assert_eq!(total_drops, exp_drops, "{label} default 2B drop mismatch");
        assert_eq!(all_gaiji_ids.len(), 11, "{label} should reference all 11 gaiji");
    }

    // DBCS trail-byte 0x5E ('^') safety check (e.g., CP932 lead 0x81 + trail 0x5E must NOT trigger caret)
    let trail_5e: &[u8] = &[0x81, 0x5E, b'A'];
    let parsed_safe = parse_caret_bytes(trail_5e, TextEncoding::Cp932, default_style, 1, None, &palette);
    assert_eq!(parsed_safe.stats.total_caret_seqs, 0);
    assert_eq!(parsed_safe.tokens.len(), 2);
}

fn find_test_fonts() -> (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>) {
    let jp_gothic_candidates = [
        "/tmp/test_fonts/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ];
    let jp_mincho_candidates = [
        "/tmp/test_fonts/usr/share/fonts/opentype/ipafont-mincho/ipam.ttf",
        "/usr/share/fonts/opentype/ipafont-mincho/ipam.ttf",
    ];
    let chs_candidates = [
        "/tmp/test_fonts/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    ];

    let gothic = jp_gothic_candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|p| fs::read(p).unwrap())
        .expect("test Gothic TTF font must exist in container (/tmp/test_fonts/.../ipag.ttf)");
    let mincho = jp_mincho_candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|p| fs::read(p).unwrap());
    let chs = chs_candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|p| fs::read(p).unwrap());
    (gothic, mincho, chs)
}

#[test]
fn freetype_all_6_modes_monospace_vertical_and_kinsoku() {
    let (gothic_bytes, mincho_bytes, chs_bytes) = find_test_fonts();
    let lib = FontLibrary::new().unwrap();
    let gothic = lib.load_face_from_bytes(gothic_bytes, 0).unwrap();
    let mincho = mincho_bytes.map(|b| lib.load_face_from_bytes(b, 0).unwrap());
    let fonts = FontSet::new(gothic, mincho);

    // 1. Scale formula 0x441b00
    assert_eq!(scale_font_height(18, 0), 9);
    assert_eq!(scale_font_height(18, 1), 18);
    assert_eq!(scale_font_height(18, 2), 27);
    assert_eq!(scale_font_height(18, 3), 36);
    assert_eq!(scale_font_height(18, 4), 54);

    // 2. Mode 3 inverted alpha & forced DBCS/halfwidth monospace override (0x44fca0)
    let g_jp = rasterize_glyph(
        fonts.select(FontFamily::Gothic),
        &GlyphRequest {
            ch: '腐',
            is_dbcs: true,
            pixel_height: 18,
            color_rgb: 0x00B7_3333,
            bold: false,
            italic: false,
            mode: RasterMode::Mode3,
            vertical: false,
        },
    )
    .unwrap();
    assert_eq!(g_jp.cell_inc_x, 18);
    assert_eq!(g_jp.cell_inc_y, 18);
    assert_eq!(g_jp.blend_params.blend_mode, 0);
    // All RGB pixels match requested color and inverted alpha spans opaque..transparent
    let mut seen_opaque_or_semi = false;
    let mut seen_transparent = false;
    for px in g_jp.surface.as_bytes().as_chunks::<4>().0 {
        assert_eq!(px[0], 0x33); // B
        assert_eq!(px[1], 0x33); // G
        assert_eq!(px[2], 0xB7); // R
        if px[3] < 0x80 {
            seen_opaque_or_semi = true;
        }
        if px[3] == 0xFF {
            seen_transparent = true;
        }
    }
    assert!(seen_opaque_or_semi && seen_transparent);

    let g_ascii = rasterize_glyph(
        fonts.select(FontFamily::Gothic),
        &GlyphRequest {
            ch: 'W',
            is_dbcs: false,
            pixel_height: 18,
            color_rgb: 0x00FF_FFFF,
            bold: true,
            italic: true,
            mode: RasterMode::Mode3,
            vertical: false,
        },
    )
    .unwrap();
    assert_eq!(g_ascii.cell_inc_x, 9); // font_h >> 1
    assert_eq!(g_ascii.cell_inc_y, 18);

    // 3. Mode 0, Mode 1, Mode 2, Mode 4, Mode 5
    let m0 = rasterize_glyph(
        fonts.select(FontFamily::Gothic),
        &GlyphRequest {
            ch: '姫',
            is_dbcs: true,
            pixel_height: 20,
            color_rgb: 0x00FF_DE00,
            bold: false,
            italic: false,
            mode: RasterMode::Mode0,
            vertical: false,
        },
    )
    .unwrap();
    assert!(m0.surface.as_bytes().as_chunks::<4>().0.contains(&[0x00, 0xDE, 0xFF, 0x00]));
    assert!(m0.surface.as_bytes().as_chunks::<4>().0.iter().all(|p| p[3] == 0x00 || p[3] == 0xFF));

    let m1 = rasterize_glyph(
        fonts.select(FontFamily::Gothic),
        &GlyphRequest {
            ch: '姫',
            is_dbcs: true,
            pixel_height: 20,
            color_rgb: 0x007F_DFA5,
            bold: false,
            italic: false,
            mode: RasterMode::Mode1,
            vertical: false,
        },
    )
    .unwrap();
    assert_eq!(m1.blend_params.blend_mode, 0x16);
    assert_eq!(m1.blend_params.color_or_center, 0x007F_DFA5);
    assert!(m1
        .surface
        .as_bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| p[0] == p[1] && p[1] == p[2] && p[0] <= 31 && p[3] == 0x00));
    assert_eq!(mode1_gray5(quantize_gray8_to_64(255)), 31);
    assert_eq!(mode3_inverted_alpha(64), 0x00);
    assert_eq!(mode3_inverted_alpha(0), 0xFF);

    let m2 = rasterize_glyph(
        fonts.select(FontFamily::Gothic),
        &GlyphRequest {
            ch: '姫',
            is_dbcs: true,
            pixel_height: 20,
            color_rgb: 0x00FF_FFFF,
            bold: false,
            italic: false,
            mode: RasterMode::Mode2,
            vertical: false,
        },
    )
    .unwrap();
    assert_eq!(m2.surface.width(), m0.surface.width() + 3);
    assert_eq!(m2.surface.height(), m0.surface.height() + 3);
    assert!(m2.surface.as_bytes().as_chunks::<4>().0.contains(&[0, 0, 0, 0x00])); // black shadow
    assert!(m2.surface.as_bytes().as_chunks::<4>().0.contains(&[0xFF, 0xFF, 0xFF, 0x00])); // fg

    let m4 = rasterize_glyph(
        fonts.select(FontFamily::Gothic),
        &GlyphRequest {
            ch: 'A',
            is_dbcs: false,
            pixel_height: 20,
            color_rgb: 0x00FA_A25A,
            bold: false,
            italic: false,
            mode: RasterMode::Mode4,
            vertical: false,
        },
    )
    .unwrap();
    let m5 = rasterize_glyph(
        fonts.select(FontFamily::Gothic),
        &GlyphRequest {
            ch: 'A',
            is_dbcs: false,
            pixel_height: 20,
            color_rgb: 0x00FA_A25A,
            bold: false,
            italic: false,
            mode: RasterMode::Mode5,
            vertical: false,
        },
    )
    .unwrap();
    assert_eq!(m5.surface.width(), m4.surface.width() + 3);
    assert_eq!(m5.surface.height(), m4.surface.height() + 3);

    // 4. @ Vertical 90° CCW glyph + column layout (0x442370)
    let v_parsed = parse_caret_utf8("あいう^nえお", TextStyle::default(), 1, None, &[0; 10]);
    let mut gaiji_bank = GaijiBank::new();
    let v_layout = layout_text(&v_parsed, &fonts, None, &mut gaiji_bank, 300, 200, true).unwrap();
    assert_eq!(v_layout.line_count, 2);
    // Column 0 is on the right side of box_width, column 1 moves left by col_max_w + 5
    let col0_x = v_layout.items[0].x;
    let col1_x = v_layout.items[3].x;
    assert!(col1_x < col0_x, "vertical columns must progress right-to-left");

    // 5. Kinsoku verification: 48 start + 12 end CP932 table + wrapping behavior
    assert_eq!(KINSOKU_START_ASCII.len(), 10);
    assert_eq!(KINSOKU_START_SJIS.len(), 38);
    assert_eq!(KINSOKU_END_SJIS.len(), 12);
    assert_eq!(classify_cp932_kinsoku(0x8142), KinsokuClass::LineStartForbidden); // 。
    assert_eq!(classify_cp932_kinsoku(0x8175), KinsokuClass::LineEndForbidden);   // 「

    // Box width = 36 px (2 fullwidth 18px chars):
    // - "あい。" -> '。' is LineStartForbidden, allowed 1 hanging overhang on line 0!
    // - "あ「い" -> '「' is LineEndForbidden at pos 1, pushed to line 1 together with 'い'!
    let k1 = parse_caret_utf8("あい。", TextStyle::default(), 1, None, &[0; 10]);
    let k1_layout = layout_text(&k1, &fonts, None, &mut gaiji_bank, 36, 108, false).unwrap();
    assert_eq!(k1_layout.line_count, 1, "LineStartForbidden allows 1 hanging char");
    assert_eq!(k1_layout.items[2].line_index, 0);

    let k2 = parse_caret_utf8("あ「い", TextStyle::default(), 1, None, &[0; 10]);
    let k2_layout = layout_text(&k2, &fonts, None, &mut gaiji_bank, 36, 108, false).unwrap();
    assert_eq!(k2_layout.line_count, 2);
    assert_eq!(k2_layout.items[0].line_index, 0);
    assert_eq!(k2_layout.items[1].line_index, 1, "LineEndForbidden must wrap to next line");
    assert_eq!(k2_layout.items[2].line_index, 1);

    // 6. Real gaiji + caret colors + center align + Mincho + Typewriter 50ms + CHS wqy-zenhei
    if require_real_assets_or_skip("freetype_all_6_modes_monospace_vertical_and_kinsoku(real_gaiji)") {
        let vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();
        let rich = parse_caret_utf8(
            "^m^cr赤^cg緑^cw白^n^fm^d0瞬^d3遅^v0^g001",
            TextStyle::default(),
            TextSpeed::Normal.as_u16(),
            Some(&[42]),
            &[0x00FF_FFFF; 10],
        );
        let rich_layout = layout_text(
            &rich,
            &fonts,
            Some(&vfs),
            &mut gaiji_bank,
            TBOX00_TEXT_RECT.2,
            TBOX00_TEXT_RECT.3,
            false,
        )
        .unwrap();
        assert_eq!(rich_layout.line_count, 2);
        assert!(rich_layout.items.last().unwrap().is_gaiji);
        assert!(
            rich_layout.items[0].x > (TBOX00_TEXT_RECT.2 / 4),
            "^m center alignment should indent short first line toward box center (TextAlign::{:?})",
            TextAlign::Center
        );

        // Verify ^v0 produced fullwidth '４' and '２'
        assert!(rich.tokens.iter().any(|t| matches!(t, TextToken::Char { ch: '４', .. })));
        assert!(rich.tokens.iter().any(|t| matches!(t, TextToken::Char { ch: '２', .. })));

        // Typewriter 50ms timing check
        let mut tw = TypewriterState::from_layout(&rich_layout);
        assert_eq!(tw.revealed_count(), 0);
        tw.advance_ms(50); // reveals '赤' (delay 1)
        assert_eq!(tw.revealed_count(), 1);
        tw.advance_ms(50); // reveals '緑' (delay 1)
        assert_eq!(tw.revealed_count(), 2);
        tw.advance_ms(50); // reveals '白' (delay 1)
        assert_eq!(tw.revealed_count(), 3);
        tw.advance_ms(50); // ^d0 '瞬' (delay 0) -> loops and reveals '遅' (delay 1 - 2 + 6 = 5) in same tick!
        assert_eq!(tw.revealed_count(), 5);
        tw.advance_ms(200); // 4 ticks waiting (delay 5 = 250ms total)
        assert_eq!(tw.revealed_count(), 5);
        tw.advance_ms(50); // 5th tick reveals '４'
        assert_eq!(tw.revealed_count(), 6);

        let mut canvas = Surface::new(GAME_WIDTH, GAME_HEIGHT).unwrap();
        render_layout_to_surface(&mut canvas, &rich_layout, None, (TBOX00_TEXT_RECT.0, TBOX00_TEXT_RECT.1));
        assert!(canvas.as_bytes().as_chunks::<4>().0.iter().any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0));
    }

    // Verify CHS TTC font loading & rendering if present
    if let Some(chs_font_bytes) = chs_bytes {
        let chs_face = lib.load_face_from_bytes(chs_font_bytes, 0).unwrap();
        let chs_set = FontSet::new(chs_face, None);
        let chs_parsed = parse_caret_utf8("简体中文测试^fm字体切换", TextStyle::default(), 1, None, &[0; 10]);
        let chs_layout = layout_text(&chs_parsed, &chs_set, None, &mut gaiji_bank, 630, 108, false).unwrap();
        assert_eq!(chs_layout.items.len(), 10);
    }
}
