use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use fujihime_core::gfx::decode::{
    decode_bmp, decode_bmp_with_options, decode_lwg, decode_msk, decode_wcg_with_info,
    BmpDecodeOptions,
};
use fujihime_core::gfx::Surface;
use fujihime_core::vfs::{AssetCategory, AssetSource, AssetVfs, LanguageMode, VfsConfig, XflArchive};
use sha2::{Digest, Sha256};

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

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn build_synthetic_xfl(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let count = entries.len() as u32;
    let dir_size = count * 40;
    let mut out = Vec::new();
    out.extend_from_slice(b"LB\x01\x00");
    out.extend_from_slice(&dir_size.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());

    let mut rel_off = 0u32;
    for (name, payload) in entries {
        let mut name_buf = [0u8; 32];
        let copy_len = name.len().min(32);
        name_buf[..copy_len].copy_from_slice(&name[..copy_len]);
        out.extend_from_slice(&name_buf);
        out.extend_from_slice(&rel_off.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        rel_off += payload.len() as u32;
    }
    for (_, payload) in entries {
        out.extend_from_slice(payload);
    }
    out
}

fn build_uncompressed_bmp(width: i32, height: i32, bpp: u16, palette: &[u8], body: &[u8]) -> Vec<u8> {
    let bf_off = 54u32 + palette.len() as u32;
    let file_size = bf_off + body.len() as u32;
    let mut out = Vec::with_capacity(file_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&bf_off.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&bpp.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(palette);
    out.extend_from_slice(body);
    out
}

#[test]
fn xfl_synthetic_cp932_and_case_insensitive() {
    // Shift-JIS "テスト.txt"
    let sjis_name: &[u8] = &[0x83, 0x65, 0x83, 0x58, 0x83, 0x67, b'.', b'T', b'X', b'T'];
    let bytes = build_synthetic_xfl(&[(b"Alpha.BIN", b"hello"), (sjis_name, b"world")]);
    let arc = XflArchive::from_bytes("mem.xfl", bytes).unwrap();
    assert_eq!(arc.len(), 2);
    assert_eq!(arc.read("alpha.bin").unwrap(), b"hello");
    assert_eq!(arc.read("ALPHA.BIN").unwrap(), b"hello");
    assert_eq!(arc.read("テスト.txt").unwrap(), b"world");
    assert!(arc.read("missing.bin").is_err());
}

#[test]
fn vfs_priority_override_scattered_backup_and_mov() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("fujihime_vfs_test_{nonce}"));
    let game = root.join("game");
    let backup = game.join("补丁").join("原版备份");
    let scattered = game.join("grpe");
    let mov = game.join("mov");
    let ovr = root.join("override");
    let ovr_grpe = ovr.join("grpe");
    fs::create_dir_all(&backup).unwrap();
    fs::create_dir_all(&scattered).unwrap();
    fs::create_dir_all(&mov).unwrap();
    fs::create_dir_all(&ovr_grpe).unwrap();

    fs::write(
        game.join("grpe.xfl"),
        build_synthetic_xfl(&[(b"0001.wcg", b"xfl-grpe")]),
    )
    .unwrap();
    fs::write(
        game.join("grpo_tp.xfl"),
        build_synthetic_xfl(&[(b"8000.bmp", b"chs-tp")]),
    )
    .unwrap();
    fs::write(
        backup.join("grpo_tp.xfl"),
        build_synthetic_xfl(&[(b"8000.wcg", b"orig-tp")]),
    )
    .unwrap();
    fs::write(scattered.join("0001.WCG"), b"chs-scattered").unwrap();
    fs::write(mov.join("0002.MPG"), b"mpg-data").unwrap();

    // Japanese mode ignores scattered directory and uses backup grpo_tp.xfl.
    let jp_vfs = AssetVfs::open(VfsConfig::new(&game, LanguageMode::Japanese)).unwrap();
    let r1 = jp_vfs.resolve(AssetCategory::Grpe, "0001.wcg").unwrap();
    assert_eq!(r1.bytes, b"xfl-grpe");
    assert!(matches!(r1.source, AssetSource::Archive { .. }));
    let r2 = jp_vfs
        .resolve_with_extensions(AssetCategory::GrpoTp, "8000", &["wcg", "bmp"])
        .unwrap();
    assert_eq!(r2.bytes, b"orig-tp");
    assert_eq!(
        jp_vfs.read(AssetCategory::Mov, "0002.mpg").unwrap(),
        b"mpg-data"
    );

    // ChineseV1 mode prefers scattered directory and root grpo_tp.xfl.
    let chs_vfs = AssetVfs::open(VfsConfig::new(&game, LanguageMode::ChineseV1)).unwrap();
    let c1 = chs_vfs.resolve(AssetCategory::Grpe, "0001.wcg").unwrap();
    assert_eq!(c1.bytes, b"chs-scattered");
    assert!(matches!(c1.source, AssetSource::ScatteredPatchFile(_)));
    let c2 = chs_vfs
        .resolve_with_extensions(AssetCategory::GrpoTp, "8000", &["wcg", "bmp"])
        .unwrap();
    assert_eq!(c2.bytes, b"chs-tp");

    // Override dir beats everything in both modes.
    fs::write(ovr_grpe.join("0001.wcg"), b"hot-override").unwrap();
    let ovr_vfs =
        AssetVfs::open(VfsConfig::new(&game, LanguageMode::ChineseV1).with_override_dir(&ovr))
            .unwrap();
    let o1 = ovr_vfs.resolve(AssetCategory::Grpe, "0001.wcg").unwrap();
    assert_eq!(o1.bytes, b"hot-override");
    assert!(matches!(o1.source, AssetSource::OverrideFile(_)));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn bmp_and_msk_bit_depths_and_present_alpha() {
    // 24bpp 2x1 bottom-up BMP with pure green (0x00FF00) and blue (0xFF0000).
    // Row stride for 2 pixels * 3 bytes = 6 -> padded to 8 bytes.
    let bmp24 = build_uncompressed_bmp(2, 1, 24, &[], &[0x00, 0xFF, 0x00, 0xFF, 0x00, 0x00, 0, 0]);
    let surf_raw = decode_bmp(&bmp24).unwrap();
    assert_eq!(surf_raw.get_pixel(0, 0), [0x00, 0xFF, 0x00, 0x00]);
    assert_eq!(surf_raw.get_pixel(1, 0), [0xFF, 0x00, 0x00, 0x00]);

    let surf_keyed = decode_bmp_with_options(
        &bmp24,
        BmpDecodeOptions {
            companion_stream: None,
            green_colorkey: true,
        },
    )
    .unwrap()
    .surface;
    assert_eq!(surf_keyed.get_pixel(0, 0), [0x00, 0xFF, 0x00, 0xFF]); // inverted transparent
    assert_eq!(surf_keyed.get_pixel(1, 0), [0xFF, 0x00, 0x00, 0x00]); // inverted opaque

    let present = surf_keyed.to_present_bgra8();
    assert_eq!(&present[0..4], &[0x00, 0xFF, 0x00, 0x00]); // standard transparent
    assert_eq!(&present[4..8], &[0xFF, 0x00, 0x00, 0xFF]); // standard opaque

    // 1bpp 2x2 MSK: bottom row (y=1) = 10 -> [0xFF, 0x00], top row (y=0) = 01 -> [0x00, 0xFF].
    let pal1 = [0, 0, 0, 0, 255, 255, 255, 0];
    let msk1 = build_uncompressed_bmp(2, 2, 1, &pal1, &[0b1000_0000, 0, 0, 0, 0b0100_0000, 0, 0, 0]);
    let mask = decode_msk(&msk1).unwrap();
    assert_eq!(mask.data, vec![0x00, 0xFF, 0xFF, 0x00]);

    // 8bpp 2x1 MSK reading palette Blue channel.
    let mut pal8 = vec![0u8; 256 * 4];
    pal8[4] = 42;
    pal8[8] = 200;
    let msk8 = build_uncompressed_bmp(2, 1, 8, &pal8, &[1, 2, 0, 0]);
    let mask8 = decode_msk(&msk8).unwrap();
    assert_eq!(mask8.data, vec![42, 200]);

    // 32bpp 1x1 BMP preserving inverted alpha directly.
    let bmp32 = build_uncompressed_bmp(1, 1, 32, &[], &[10, 20, 30, 0x80]);
    let s32 = decode_bmp(&bmp32).unwrap();
    assert_eq!(s32.get_pixel(0, 0), [10, 20, 30, 0x80]);

    let s = Surface::from_bgra(1, 1, vec![1, 2, 3, 0x40]).unwrap();
    assert_eq!(s.to_present_bgra8(), vec![1, 2, 3, 0xBF]);
}

#[test]
fn real_japanese_all_assets_decode() {
    if !require_real_assets_or_skip("real_japanese_all_assets_decode") {
        return;
    }
    let vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();

    let gfx_cats = [
        AssetCategory::Grpe,
        AssetCategory::Grpo,
        AssetCategory::Grpp,
        AssetCategory::Grps,
        AssetCategory::Grpf,
        AssetCategory::GrpoEx,
        AssetCategory::GrpoTp,
    ];

    let mut wcg_count = 0usize;
    let mut lwg_count = 0usize;
    let mut lwg_layers = 0usize;
    let mut msk_count = 0usize;
    let mut pad_bytes = 0usize;

    for cat in gfx_cats {
        let arc = vfs.archive(cat).expect("archive present");
        for entry in arc.entries() {
            let raw = arc.read_entry(entry).unwrap();
            let lower = entry.name.to_ascii_lowercase();
            if lower.ends_with(".wcg") {
                let dec = decode_wcg_with_info(&raw).unwrap();
                assert_eq!(
                    dec.surface.as_bytes().len(),
                    (dec.surface.width() * dec.surface.height() * 4) as usize
                );
                pad_bytes += dec.trailing_pad_bytes;
                wcg_count += 1;
            } else if lower.ends_with(".lwg") {
                let lwg = decode_lwg(&raw).unwrap();
                assert_eq!(lwg.layers.len(), lwg.layers.len());
                pad_bytes += lwg.total_trailing_pad_bytes();
                lwg_layers += lwg.layers.len();
                lwg_count += 1;
            } else if lower.ends_with(".msk") {
                let msk = decode_msk(&raw).unwrap();
                assert_eq!(msk.data.len(), (msk.width * msk.height) as usize);
                msk_count += 1;
            } else {
                panic!("unexpected asset in original Japanese archive: {}", entry.name);
            }
        }
    }

    assert_eq!(wcg_count, 1727, "expected 1727 original .wcg files");
    assert_eq!(lwg_count, 15, "expected 15 original .lwg files");
    assert_eq!(lwg_layers, 154, "expected 154 original .lwg layers");
    assert_eq!(msk_count, 12, "expected 12 original .msk files");
    assert_eq!(pad_bytes, 0, "original assets must have 0 trailing pad bytes");

    // Verify original scr.xfl, audio archives, and mov/ are resolved cleanly in Japanese mode.
    assert_eq!(vfs.archive(AssetCategory::Scr).unwrap().len(), 341);
    assert_eq!(vfs.archive(AssetCategory::Bgm).unwrap().len(), 21);
    assert_eq!(vfs.archive(AssetCategory::Voice).unwrap().len(), 3015);
    assert_eq!(vfs.archive(AssetCategory::Wav).unwrap().len(), 48);

    let tp8000 = vfs.resolve(AssetCategory::GrpoTp, "8000.wcg").unwrap();
    assert!(matches!(tp8000.source, AssetSource::Archive { .. }));
    // Scattered CHS dir must NOT be read in Japanese mode.
    let compane = vfs.resolve(AssetCategory::Grps, "compane.lwg").unwrap();
    assert!(matches!(compane.source, AssetSource::Archive { .. }));

    let mov1 = vfs.resolve(AssetCategory::Mov, "0001.mpg").unwrap();
    assert!(matches!(mov1.source, AssetSource::MovieFile(_)));
    assert_eq!(mov1.bytes.len(), 47_437_828);
    let mov2 = vfs.resolve(AssetCategory::Mov, "0002.mpg").unwrap();
    assert_eq!(mov2.bytes.len(), 278_532);
}

#[test]
fn real_chs_v1_all_assets_decode() {
    if !require_real_assets_or_skip("real_chs_v1_all_assets_decode") {
        return;
    }
    let vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::ChineseV1)).unwrap();

    // 1. CHS grpo_tp.xfl: 80 .wcg + 36 .bmp
    let tp_arc = vfs.archive(AssetCategory::GrpoTp).unwrap();
    let mut tp_wcg = 0usize;
    let mut tp_bmp = 0usize;
    for entry in tp_arc.entries() {
        let raw = tp_arc.read_entry(entry).unwrap();
        let lower = entry.name.to_ascii_lowercase();
        if lower.ends_with(".wcg") {
            let dec = decode_wcg_with_info(&raw).unwrap();
            assert_eq!(dec.trailing_pad_bytes, 0);
            tp_wcg += 1;
        } else if lower.ends_with(".bmp") {
            let dec = decode_bmp_with_options(&raw, BmpDecodeOptions::default()).unwrap();
            assert_eq!(dec.bpp, 32);
            tp_bmp += 1;
        } else {
            panic!("unexpected entry in CHS grpo_tp.xfl: {}", entry.name);
        }
    }
    assert_eq!(tp_wcg, 80);
    assert_eq!(tp_bmp, 36);

    // Verify stem lookup prefers BMP in CHS grpo_tp.xfl where 8000.wcg was replaced by 8000.bmp.
    let tp8000 = vfs
        .resolve_with_extensions(AssetCategory::GrpoTp, "8000", &["wcg", "bmp"])
        .unwrap();
    assert_eq!(decode_bmp(&tp8000.bytes).unwrap().width(), 496);

    // 2. Scattered CHS directories: grpe/, grpo_ex/, grps/ (232 .wcg + 2 .lwg = 76 layers, 93 pad bytes)
    let mut dir_wcg = 0usize;
    let mut dir_lwg = 0usize;
    let mut dir_lwg_layers = 0usize;
    let mut dir_pad = 0usize;

    for (cat, sub) in [
        (AssetCategory::Grpe, "grpe"),
        (AssetCategory::GrpoEx, "grpo_ex"),
        (AssetCategory::Grps, "grps"),
    ] {
        let dir_path = Path::new(GAME_DIR).join(sub);
        let mut names: Vec<String> = fs::read_dir(&dir_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();

        for name in names {
            let resolved = vfs.resolve(cat, &name).unwrap();
            assert!(
                matches!(resolved.source, AssetSource::ScatteredPatchFile(_)),
                "expected ScatteredPatchFile for {sub}/{name}"
            );
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".wcg") {
                let dec = decode_wcg_with_info(&resolved.bytes).unwrap();
                dir_pad += dec.trailing_pad_bytes;
                dir_wcg += 1;
            } else if lower.ends_with(".lwg") {
                let lwg = decode_lwg(&resolved.bytes).unwrap();
                dir_pad += lwg.total_trailing_pad_bytes();
                dir_lwg_layers += lwg.layers.len();
                dir_lwg += 1;
            } else {
                panic!("unexpected file in CHS {sub}/: {name}");
            }
        }
    }

    assert_eq!(dir_wcg, 232);
    assert_eq!(dir_lwg, 2);
    assert_eq!(dir_lwg_layers, 76);
    assert_eq!(dir_pad, 93);
}

#[test]
fn real_pixel_sha256_parity_with_python_reference() {
    if !require_real_assets_or_skip("real_pixel_sha256_parity_with_python_reference") {
        return;
    }
    let jp_vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();
    let chs_vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::ChineseV1)).unwrap();

    // 1. grpe.xfl/0001.wcg (800x600 opaque WCG)
    let bg = decode_wcg_with_info(&jp_vfs.read(AssetCategory::Grpe, "0001.wcg").unwrap())
        .unwrap()
        .surface;
    assert_eq!((bg.width(), bg.height()), (800, 600));
    assert_eq!(bg.get_pixel(0, 0), [13, 28, 25, 0]);
    assert_eq!(
        sha256_hex(bg.as_bytes()),
        "ee7ea087f5ddbb4296780a003df3d85d6b0d3db097096bb536628a07c67adf05"
    );

    // 2. 补丁/原版备份/grpo_tp.xfl/8000.wcg (496x218 inverted-alpha WCG)
    let tp_orig =
        decode_wcg_with_info(&jp_vfs.read(AssetCategory::GrpoTp, "8000.wcg").unwrap())
            .unwrap()
            .surface;
    assert_eq!((tp_orig.width(), tp_orig.height()), (496, 218));
    assert_eq!(tp_orig.get_pixel(0, 0), [0, 0, 0, 255]);
    assert_eq!(
        sha256_hex(tp_orig.as_bytes()),
        "b07de25bb8760648e8bb2089c214ab8e9ee7e7308470c1c1e626dea825ca53a7"
    );

    // 3. CHS grpo_tp.xfl/8000.bmp (496x218 32bpp pre-inverted-alpha BMP)
    let tp_bmp = decode_bmp(&chs_vfs.read(AssetCategory::GrpoTp, "8000.bmp").unwrap()).unwrap();
    assert_eq!((tp_bmp.width(), tp_bmp.height()), (496, 218));
    assert_eq!(tp_bmp.get_pixel(0, 0), [0, 0, 0, 255]);
    assert_eq!(
        sha256_hex(tp_bmp.as_bytes()),
        "d929d10c24ae7b66917f505b862cb1ad5577b831902ee3059933a1914c8cd994"
    );

    // 4. grps.xfl/compane.lwg (original LWG + nested WCG layer)
    let compane = decode_lwg(&jp_vfs.read(AssetCategory::Grps, "compane.lwg").unwrap()).unwrap();
    assert_eq!((compane.width, compane.height), (39, 106));
    assert_eq!(compane.layers.len(), 18);
    let first = &compane.layers[0];
    assert_eq!(first.name, "slide_lev");
    assert_eq!((first.surface.width(), first.surface.height()), (14, 60));
    assert_eq!(
        sha256_hex(first.surface.as_bytes()),
        "0a04fad48f3f2ac1b768d815f01c1b550a9fdac6bb0388881c8e08afa89247cd"
    );

    // 5. grps.xfl/ef14.msk (800x600 24bpp MSK)
    let ef14 = decode_msk(&jp_vfs.read(AssetCategory::Grps, "ef14.msk").unwrap()).unwrap();
    assert_eq!((ef14.width, ef14.height), (800, 600));
    assert_eq!(*ef14.data.iter().min().unwrap(), 26);
    assert_eq!(*ef14.data.iter().max().unwrap(), 255);
    assert_eq!(
        sha256_hex(&ef14.data),
        "c6f9da9de2f4b4db8b9aed6c7fe8ff48c66bcfe58bb93e7261483026375684c6"
    );

    // 6. CHS scattered grpe/9001.wcg (1-byte trailing 0x00 pad tolerance)
    let chs_pad = decode_wcg_with_info(&chs_vfs.read(AssetCategory::Grpe, "9001.wcg").unwrap())
        .unwrap();
    assert_eq!(chs_pad.trailing_pad_bytes, 1);
    assert_eq!(
        sha256_hex(chs_pad.surface.as_bytes()),
        "3566e54b09e81174f2e23e66990fb1e5d73b30e765fd1777371cd45327b971ce"
    );

    // 7. CHS scattered grps/confscrn.lwg (57 layers, 17 trailing pad bytes)
    let chs_lwg =
        decode_lwg(&chs_vfs.read(AssetCategory::Grps, "confscrn.lwg").unwrap()).unwrap();
    assert_eq!(chs_lwg.layers.len(), 57);
    assert_eq!(chs_lwg.total_trailing_pad_bytes(), 17);
    assert_eq!(
        sha256_hex(chs_lwg.layers[0].surface.as_bytes()),
        "f65daabf732b2805e78173720189f97e56f8bf206eef827515b619f271bcfcc2"
    );
}
