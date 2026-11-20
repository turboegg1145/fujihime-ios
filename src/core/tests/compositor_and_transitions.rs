use fujihime_core::gfx::cmath::{
    get_embos_table, get_white_table, lerp_rgb, porter_duff_over, sat_add_rgb, ADD_TABLE,
    DEL_TABLE, DITHER_TABLE_12, DITHER_TABLE_13,
};
use fujihime_core::gfx::compositor::{
    blit, blit_porter_duff_over, clip_rects, filter_darken_pct, filter_embos_tint,
    filter_fill_argb, filter_fill_rgb_keep_alpha, filter_grayscale, filter_invert_rgb,
    BlitParams, Canvas800x600, Rect,
};
use fujihime_core::gfx::decode::decode_msk;
use fujihime_core::gfx::transition::{
    focal_grid_point, zoom_crop_rect, TransitionKind, TransitionRunner,
};
use fujihime_core::gfx::{Surface, GAME_HEIGHT, GAME_WIDTH};
use fujihime_core::vfs::{AssetVfs, LanguageMode, VfsConfig};

const GAME_DIR: &str = "/workspace/game/腐姫ＤＬ";

fn require_real_assets_or_skip(test_name: &str) -> bool {
    if std::path::Path::new(GAME_DIR).is_dir() {
        return true;
    }
    if std::env::var("FUJIHIME_REQUIRE_REAL_ASSETS").ok().as_deref() == Some("1") {
        panic!("FUJIHIME_REQUIRE_REAL_ASSETS=1 but real assets missing at {GAME_DIR}");
    }
    eprintln!("[skip] {test_name}: real game assets not found at {GAME_DIR}");
    false
}

#[test]
fn cmath_luts_and_porter_duff_boundaries() {
    // ADD_TABLE boundaries
    for j in 0..=255u8 {
        assert_eq!(ADD_TABLE[0][j as usize], j);
        assert_eq!(ADD_TABLE[255][j as usize], 0);
        assert_eq!(DEL_TABLE[0][j as usize], j);
        assert_eq!(DEL_TABLE[255][j as usize], 255);
        assert_eq!(get_white_table(j)[j as usize], DEL_TABLE[j as usize][j as usize]);
    }
    assert_eq!(ADD_TABLE[128][200], ((200u32 * 128) >> 8) as u8);
    assert_eq!(DEL_TABLE[128][100], (100u32 + ((156u32 * 128) >> 8)) as u8);

    // Embos table boundary at 127 / 128
    assert_eq!(get_embos_table(0)[255], ADD_TABLE[254][255]);
    assert_eq!(get_embos_table(127)[200], ADD_TABLE[0][200]);
    assert_eq!(get_embos_table(128)[100], DEL_TABLE[1][100]);
    assert_eq!(get_embos_table(255)[100], DEL_TABLE[255][100]);

    // lerp_rgb and sat_add_rgb
    assert_eq!(lerp_rgb(0, [10, 20, 30, 99], [200, 210, 220, 77]), [10, 20, 30, 0]);
    assert_eq!(
        sat_add_rgb([200, 100, 50, 0x44], [100, 200, 10, 0xFF]),
        [255, 255, 60, 0x44]
    );

    // Porter-Duff Over (`0x401890`)
    let dst = [20, 40, 60, 0x80];
    let src_opaque = [100, 120, 140, 0x00];
    let src_trans = [100, 120, 140, 0xFF];
    assert_eq!(porter_duff_over(dst, src_opaque), src_opaque);
    assert_eq!(porter_duff_over(dst, src_trans), dst);

    // Semi-transparent over transparent background should preserve source RGB and reduce alpha.
    let transparent_bg = [0, 0, 0, 0xFF];
    let semi = [120, 80, 40, 0x80];
    let res = porter_duff_over(transparent_bg, semi);
    assert_eq!(res[3], 0x80);
    assert_eq!(res[0], ADD_TABLE[0][120]);
    assert_eq!(res[1], ADD_TABLE[0][80]);
    assert_eq!(res[2], ADD_TABLE[0][40]);

    // Semi-transparent over opaque background yields opaque output (`A = 0x00`).
    let opaque_bg = [200, 200, 200, 0x00];
    let res2 = porter_duff_over(opaque_bg, semi);
    assert_eq!(res2[3], 0x00);
}

#[test]
fn clipping_negative_and_oob_coordinates() {
    // Negative offset and dst_pt partially off top-left
    let reg = clip_rects(100, 80, 800, 600, -10, -20, (-5, 5), Rect::from_size(800, 600)).unwrap();
    assert_eq!(reg.dst_left, 0);
    assert_eq!(reg.dst_top, 0);
    assert_eq!(reg.src_left, 15);
    assert_eq!(reg.src_top, 15);
    assert_eq!(reg.copy_width, 85);
    assert_eq!(reg.copy_height, 65);

    // Completely off right edge -> None
    assert!(clip_rects(50, 50, 800, 600, 0, 0, (801, 0), Rect::from_size(800, 600)).is_none());
    // Touching edge exactly -> zero width, returns Some
    let touch = clip_rects(50, 50, 800, 600, 0, 0, (800, 0), Rect::from_size(800, 600)).unwrap();
    assert_eq!(touch.copy_width, 0);

    // Blit with negative coordinate safely updates only visible pixels
    let mut dst = Surface::filled(4, 4, 0x0010_2030).unwrap();
    let src = Surface::filled(4, 4, 0x0040_5060).unwrap();
    assert!(blit(
        &mut dst,
        &src,
        &BlitParams {
            blend_mode: 0,
            alpha_channel_mode: 6,
            ..Default::default()
        },
        (-2, -2),
        None
    ));
    assert_eq!(dst.get_pixel(0, 0), [0x60, 0x50, 0x40, 0x00]);
    assert_eq!(dst.get_pixel(2, 2), [0x30, 0x20, 0x10, 0x00]);
}

#[test]
fn compositor_all_modes_and_filters() {
    let mut src = Surface::new(4, 4).unwrap();
    for y in 0..4u32 {
        for x in 0..4u32 {
            src.set_pixel(x, y, [100, 150, 200, ((x + y * 4) * 16) as u8]);
        }
    }
    let dst_init = Surface::filled(4, 4, 0x2040_6080).unwrap();

    // Mode 0 sub 0..=6
    for sub in 0..=6 {
        let mut dst = dst_init.clone();
        assert!(blit(
            &mut dst,
            &src,
            &BlitParams {
                blend_mode: 0,
                alpha_channel_mode: sub,
                threshold: 100,
                ..Default::default()
            },
            (0, 0),
            None
        ));
        if sub == 6 {
            assert_eq!(dst, src);
        }
    }

    // Mode 0 sub 2 soft feather check: sA <= t copies src; sA > t+31 skips
    {
        let mut s = Surface::new(3, 1).unwrap();
        s.set_pixel(0, 0, [10, 20, 30, 50]); // <= 50 -> copy
        s.set_pixel(1, 0, [10, 20, 30, 66]); // 50 < 66 <= 81 -> blend
        s.set_pixel(2, 0, [10, 20, 30, 82]); // > 81 -> skip
        let mut d = Surface::filled(3, 1, 0x5588_8888).unwrap();
        blit(
            &mut d,
            &s,
            &BlitParams {
                blend_mode: 0,
                alpha_channel_mode: 2,
                threshold: 50,
                ..Default::default()
            },
            (0, 0),
            None,
        );
        assert_eq!(d.get_pixel(0, 0), [10, 20, 30, 50]);
        assert_eq!(d.get_pixel(1, 0)[3], 0);
        assert_eq!(d.get_pixel(2, 0), [0x88, 0x88, 0x88, 0x55]);
    }

    // Modes 2, 3, 4, 5, 6, 7, 8, B, C, D, E, F, 12, 13, 14, 15, 16, 67..=6D
    let modes: &[u32] = &[
        2, 3, 4, 5, 6, 7, 8, 0xB, 0xC, 0xD, 0xE, 0xF, 0x12, 0x13, 0x14, 0x15, 0x16, 0x67, 0x68,
        0x69, 0x6A, 0x6B, 0x6C, 0x6D,
    ];
    for &mode in modes {
        for sub in [0, 1, 6] {
            let mut dst = dst_init.clone();
            assert!(blit(
                &mut dst,
                &src,
                &BlitParams {
                    blend_mode: mode,
                    alpha_channel_mode: sub,
                    global_alpha: 64,
                    threshold: 120,
                    color_or_center: 0x0011_2233,
                    ..Default::default()
                },
                (0, 0),
                None
            ));
            // Verify `sub == 1` no-op contract on gated modes
            if sub == 1
                && matches!(
                    mode,
                    3 | 4 | 5 | 8 | 0xB | 0xC | 0xE | 0xF | 0x12 | 0x13 | 0x14 | 0x15
                )
            {
                assert_eq!(dst, dst_init, "mode 0x{mode:x} sub=1 must be no-op");
            }
        }
    }

    // Verify DITHER_TABLE_12 & 13 exact bitmask application
    {
        let mut d12 = Surface::filled(4, 4, 0x0000_0000).unwrap();
        let s_white = Surface::filled(4, 4, 0x00FF_FFFF).unwrap();
        blit(
            &mut d12,
            &s_white,
            &BlitParams {
                blend_mode: 0x12,
                alpha_channel_mode: 6,
                global_alpha: 0x10,
                ..Default::default()
            },
            (0, 0),
            None,
        );
        let mask12 = DITHER_TABLE_12[1];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let expected = if (mask12 & (1u16 << (x + (y << 2)))) != 0 {
                    [0xFF, 0xFF, 0xFF, 0x00]
                } else {
                    [0, 0, 0, 0]
                };
                assert_eq!(d12.get_pixel(x, y), expected);
            }
        }

        let mut d13 = Surface::filled(4, 4, 0x0000_0000).unwrap();
        blit(
            &mut d13,
            &s_white,
            &BlitParams {
                blend_mode: 0x13,
                alpha_channel_mode: 6,
                global_alpha: 0x10,
                ..Default::default()
            },
            (0, 0),
            None,
        );
        let mask13 = DITHER_TABLE_13[1];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let expected = if (mask13 & (1u16 << (x + (y << 2)))) != 0 {
                    [0xFF, 0xFF, 0xFF, 0x00]
                } else {
                    [0, 0, 0, 0]
                };
                assert_eq!(d13.get_pixel(x, y), expected);
            }
        }
    }

    // Mode 8 global_alpha < 5 falls back to Mode 0; mode 7 with global_alpha = 0 does not panic
    {
        let mut d8 = dst_init.clone();
        blit(
            &mut d8,
            &src,
            &BlitParams {
                blend_mode: 8,
                alpha_channel_mode: 6,
                global_alpha: 4,
                ..Default::default()
            },
            (0, 0),
            None,
        );
        assert_eq!(d8, src);

        let mut d7 = dst_init.clone();
        blit(
            &mut d7,
            &src,
            &BlitParams {
                blend_mode: 7,
                alpha_channel_mode: 6,
                global_alpha: 0,
                color_or_center: 0,
                ..Default::default()
            },
            (0, 0),
            None,
        );
        assert_eq!(d7, src);
    }

    // Independent Porter-Duff Over blit preserves destination alpha
    {
        let mut d = Surface::filled(2, 2, 0x8020_4060).unwrap();
        let s = Surface::filled(2, 2, 0x8080_4020).unwrap();
        assert!(blit_porter_duff_over(&mut d, &s, (0, 0), (0, 0), None));
        assert_ne!(d.get_pixel(0, 0)[3], 0x80);
    }

    // In-place filters
    {
        let mut f = Surface::from_bgra(1, 1, vec![30, 60, 90, 0x77]).unwrap();
        filter_grayscale(&mut f);
        assert_eq!(f.get_pixel(0, 0), [60, 60, 60, 0x77]);

        filter_darken_pct(&mut f, 50);
        assert_eq!(f.get_pixel(0, 0)[3], 0x77);
        assert!(f.get_pixel(0, 0)[0] < 60);

        filter_invert_rgb(&mut f);
        assert_eq!(f.get_pixel(0, 0)[3], 0x77);

        filter_embos_tint(&mut f, 0x0080_4020);
        assert_eq!(f.get_pixel(0, 0)[3], 0x77);

        filter_fill_rgb_keep_alpha(&mut f, 0x0011_2233);
        assert_eq!(f.get_pixel(0, 0), [0x33, 0x22, 0x11, 0x77]);

        filter_fill_argb(&mut f, 0xAA44_5566);
        assert_eq!(f.get_pixel(0, 0), [0x66, 0x55, 0x44, 0xAA]);
    }
}

#[test]
fn transitions_all_0_to_300_and_real_msk_assets() {
    // 1. Focal grid mapping sanity
    assert_eq!(focal_grid_point(7, 800, 600), (0, 0));
    assert_eq!(focal_grid_point(5, 800, 600), (400, 300));
    assert_eq!(focal_grid_point(0, 800, 600), (400, 300));
    assert_eq!(focal_grid_point(3, 800, 600), (800, 600));
    assert_eq!(zoom_crop_rect(0, 400, 300, 800, 600), (0, 0, 800, 600));
    assert_eq!(zoom_crop_rect(100, 400, 300, 800, 600), (200, 150, 400, 300));

    let real_vfs = if require_real_assets_or_skip("transitions_all_0_to_300_and_real_msk_assets(msk)") {
        Some(AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap())
    } else {
        None
    };

    // Pre-build a test canvas factory
    let make_canvas = || {
        let mut c = Canvas800x600::new();
        c.front.fill_u32(0x0010_2030);
        c.new_scene.fill_u32(0x00A0_B0C0);
        c
    };

    // 2. Verify every single transition ID 0..=299 + 300
    for id in 0u16..=300 {
        let kind = TransitionKind::from_id(id);
        let mut canvas = make_canvas();

        // Use real .msk for the 12 existing mask IDs (14..=19, 21..=26) when real assets are present.
        let mut runner = if let (true, Some(vfs)) = (matches!(id, 14..=19 | 21..=26), &real_vfs) {
            TransitionRunner::with_vfs(id, 4, 10, vfs).unwrap()
        } else {
            TransitionRunner::new(id, 4, 10, None)
        };

        assert_eq!(runner.kind(), kind);
        let frames = runner.run_to_end(&mut canvas);

        if id >= 300 {
            assert_eq!(frames, 0);
            assert_eq!(canvas.front.get_pixel(0, 0), [0x30, 0x20, 0x10, 0x00]);
        } else {
            assert!(frames >= 1, "transition {id} must render at least 1 frame");
            assert!(runner.is_done());
            assert_eq!(
                canvas.front.as_bytes(),
                canvas.new_scene.as_bytes(),
                "transition {id} must finish with front == new_scene"
            );
        }
    }

    // 3. Detailed frame-by-frame start/mid/end verification per transition family
    // Crossfade (id = 1)
    {
        let mut c = make_canvas();
        c.front.fill_u32(0x0000_0000);
        c.new_scene.fill_u32(0x00FF_FFFF);
        let mut r = TransitionRunner::new(1, 4, 15, None);
        assert_eq!(r.step(&mut c), Some(15));
        assert_eq!(c.front.get_pixel(0, 0), [0, 0, 0, 0]); // global_alpha=255 -> old screen
        assert_eq!(r.step(&mut c), Some(15)); // i=1
        assert_eq!(r.step(&mut c), Some(15)); // i=2 -> ~50% gray
        let mid = c.front.get_pixel(0, 0);
        assert!(mid[0] > 100 && mid[0] < 160);
        r.run_to_end(&mut c);
        assert_eq!(c.front.as_bytes(), c.new_scene.as_bytes());
    }

    // Ordered Dither (id = 2)
    {
        let mut c = make_canvas();
        let mut r = TransitionRunner::new(2, 4, 10, None);
        assert_eq!(r.step(&mut c), Some(10));
        let count = 1 + r.run_to_end(&mut c);
        assert_eq!(count, 4); // 3 dither frames + 1 final commit
        assert_eq!(c.front.as_bytes(), c.new_scene.as_bytes());
    }

    // Fade through white (id = 3) & black (id = 4) hit pure white / pure black at midpoint
    {
        let mut cw = make_canvas();
        let mut rw = TransitionRunner::new(3, 4, 10, None);
        for _ in 0..5 {
            rw.step(&mut cw);
        }
        assert_eq!(cw.front.get_pixel(0, 0), [0xFF, 0xFF, 0xFF, 0x00]);
        rw.run_to_end(&mut cw);
        assert_eq!(cw.front.as_bytes(), cw.new_scene.as_bytes());

        let mut cb = make_canvas();
        let mut rb = TransitionRunner::new(4, 4, 10, None);
        for _ in 0..5 {
            rb.step(&mut cb);
        }
        assert_eq!(cb.front.get_pixel(0, 0), [0x00, 0x00, 0x00, 0x00]);
        rb.run_to_end(&mut cb);
        assert_eq!(cb.front.as_bytes(), cb.new_scene.as_bytes());
    }

    // Real rule mask transition (ef14.msk) shows partial transition at mid-frame
    if let Some(vfs) = &real_vfs {
        let mut c = make_canvas();
        let msk = decode_msk(&vfs.read_transition_mask(14).unwrap()).unwrap();
        let mut r = TransitionRunner::new(14, 10, 10, Some(msk));
        for _ in 0..10 {
            r.step(&mut c);
        }
        let has_old = c
            .front
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0..3] == [0x30, 0x20, 0x10]);
        let has_new = c
            .front
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0..3] == [0xC0, 0xB0, 0xA0]);
        assert!(has_old && has_new, "mid-frame rule mask must mix old and new pixels");
        r.run_to_end(&mut c);
        assert_eq!(c.front.as_bytes(), c.new_scene.as_bytes());
    }

    // Zoom-in 50% blend (id = 155) & Zoom-out to black (id = 255)
    {
        let mut c1 = make_canvas();
        let mut r1 = TransitionRunner::new(155, 0, 0, None);
        assert_eq!(r1.step(&mut c1), Some(5));
        let count1 = 1 + r1.run_to_end(&mut c1);
        assert_eq!(count1, 31 + 31 + 1);
        assert_eq!(c1.front.as_bytes(), c1.new_scene.as_bytes());

        let mut c2 = make_canvas();
        let mut r2 = TransitionRunner::new(255, 0, 0, None);
        for _ in 0..41 {
            r2.step(&mut c2);
        }
        // Corner pixel at max shrink (z = 2000) must be black
        assert_eq!(c2.front.get_pixel(0, 0), [0, 0, 0, 0]);
        // Center pixel must still hold old scene color
        assert_eq!(
            c2.front.get_pixel(GAME_WIDTH / 2, GAME_HEIGHT / 2),
            [0x30, 0x20, 0x10, 0x00]
        );
        r2.run_to_end(&mut c2);
        assert_eq!(c2.front.as_bytes(), c2.new_scene.as_bytes());
    }
}
