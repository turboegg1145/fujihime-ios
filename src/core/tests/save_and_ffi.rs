use std::ffi::CString;
use std::fs;
use std::path::Path;

use fujihime_core::engine::ConfirmAction;
use fujihime_core::ffi::{
    fujihime_engine_confirm_dialog_response, fujihime_engine_copy_present_bgra8,
    fujihime_engine_create, fujihime_engine_destroy, fujihime_engine_get_state,
    fujihime_engine_key_event, fujihime_engine_load_font, fujihime_engine_load_slot,
    fujihime_engine_perform_action, fujihime_engine_pointer_event,
    fujihime_engine_poll_pending_dialog, fujihime_engine_render_audio_f32,
    fujihime_engine_rollback, fujihime_engine_save_global, fujihime_engine_save_slot,
    fujihime_engine_start_game, fujihime_engine_tick, FujihimeDialogKind, FujihimeEngineConfig,
    FujihimeEngineState, FujihimeFontRole, FujihimeLanguageMode, FujihimePendingDialog,
    FujihimePointerKind, FujihimeSceneKind, FujihimeSystemAction, FUJIHIME_OK,
    FUJIHIME_PRESENT_BGRA8_LEN,
};
use fujihime_core::save::{
    dbcs_truncate_title_26b, GlobalSave2002Cd, GlobalSave2007, SceneSnapshot, SlotSave2007,
    SlotSaveHeader2007, CD_2002_SAVE_SIZE, DL_2007_GLOBAL_MIN_SIZE, LOCAL_VARS_COUNT,
    SLOT_SAVE_SIZE,
};
use fujihime_core::text::caret::TextEncoding;
use sha2::{Digest, Sha256};

const GAME_DIR: &str = "/workspace/game/腐姫ＤＬ";
const REAL_FULLCG_SAVE: &str = "/workspace/game/腐姫ＤＬ/补丁/全CG档/KhDLsave.dat";
const EXPECTED_FULLCG_SHA256: &str =
    "78ceb006b10ac36837da0c432f74dd0b3b4e2de04dab310fb8b0d7d630b3cc69";

fn require_real_assets_or_skip(test_name: &str) -> bool {
    if Path::new(GAME_DIR).is_dir() && Path::new(REAL_FULLCG_SAVE).is_file() {
        return true;
    }
    if std::env::var("FUJIHIME_REQUIRE_REAL_ASSETS").ok().as_deref() == Some("1") {
        panic!("FUJIHIME_REQUIRE_REAL_ASSETS=1 but real assets missing at {GAME_DIR}");
    }
    eprintln!("[skip] {test_name}: real game assets not found at {GAME_DIR}");
    false
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

fn find_test_gothic_font() -> Vec<u8> {
    let candidates = [
        "/tmp/test_fonts/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/tmp/test_fonts/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    ];
    candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|p| fs::read(p).unwrap())
        .expect("test Gothic font must exist in container")
}

#[test]
fn real_2002_cd_fullcg_save_sha256_roundtrip_and_2007_dl_migration() {
    if !require_real_assets_or_skip("real_2002_cd_fullcg_save_sha256_roundtrip_and_2007_dl_migration") {
        return;
    }
    let orig_bytes = fs::read(REAL_FULLCG_SAVE).unwrap();
    assert_eq!(orig_bytes.len(), CD_2002_SAVE_SIZE);
    assert_eq!(sha256_hex(&orig_bytes), EXPECTED_FULLCG_SHA256);

    let parsed = GlobalSave2002Cd::from_bytes(&orig_bytes).unwrap();
    let rebuilt = parsed.to_bytes();
    assert_eq!(rebuilt.len(), CD_2002_SAVE_SIZE);
    assert_eq!(sha256_hex(&rebuilt), EXPECTED_FULLCG_SHA256);

    // Explicit migration to 2007 DL `KhDLsave.dat` preserves vars[7000..9999] & CG flags
    let migrated = parsed.to_2007_dl();
    assert_eq!(migrated.vars_7000_9999, parsed.vars_7000_9999);
    let nz_count = migrated
        .vars_7000_9999
        .iter()
        .filter(|&&v| v != 0)
        .count();
    assert_eq!(nz_count, 309);
    let migrated_bytes = migrated.to_bytes();
    let migrated_reparsed = GlobalSave2007::from_bytes(&migrated_bytes).unwrap();
    assert_eq!(migrated_reparsed, migrated);
}

#[test]
fn save_2007_dl_global_and_slot_byte_exact_roundtrips() {
    // 1. Empty 2007 DL global save baseline == 8,116 B
    let empty = GlobalSave2007::default();
    let empty_bytes = empty.to_bytes();
    assert_eq!(empty_bytes.len(), DL_2007_GLOBAL_MIN_SIZE);
    assert_eq!(GlobalSave2007::from_bytes(&empty_bytes).unwrap(), empty);

    // 2. Populated 2007 DL global save with config, global strings, kidoku & global_flags
    let mut pop = GlobalSave2007::default();
    pop.config.set_text_speed(4);
    pop.config.set_bgm_volume(180);
    pop.config.set_se_volume(200);
    pop.config.set_voice_volume(220);
    pop.config.set_voice_cut_on_click(true);
    pop.config.set_skip_unread(true);
    pop.config.set_auto_wait_ms(2000);
    pop.config.global_str_slot_mut(0)[..4].copy_from_slice(b"ABC\0");
    pop.vars_7000_9999[0] = 1; // vars[7000] Extra omake unlock
    pop.vars_7000_9999[2999] = 1; // vars[9999] CG 999
    pop.kidoku.set_bit(1102, 1);
    pop.kidoku.set_bit(1102, 17);
    pop.global_flags.set_bit(0, 999);

    let pop_bytes = pop.to_bytes();
    let pop_sha = sha256_hex(&pop_bytes);
    let pop_roundtrip = GlobalSave2007::from_bytes(&pop_bytes).unwrap();
    assert_eq!(sha256_hex(&pop_roundtrip.to_bytes()), pop_sha);
    assert!(pop_roundtrip.kidoku.get_bit(1102, 17));
    assert!(pop_roundtrip.global_flags.get_bit(0, 999));

    // 3. DBCS-safe 26B title truncation (never split 2-byte CP932 char across byte 23/24)
    let mut long_sjis = Vec::new();
    for _ in 0..20 {
        long_sjis.extend_from_slice(&[0x82, 0xA0]); // 'あ'
    }
    let trunc = dbcs_truncate_title_26b(&long_sjis, TextEncoding::Cp932);
    let nul_pos = trunc.iter().position(|&b| b == 0).unwrap();
    assert_eq!(nul_pos % 2, 0);
    assert!(nul_pos <= 24);

    // 4. 2007 DL SlotSave2007 fixed 75,026 B round-trip
    let mut snap = SceneSnapshot::default();
    snap.set_bgm_id(5);
    snap.set_script_id(0, 1102);
    snap.set_return_pc(0, 120);
    let slot = SlotSave2007 {
        header: SlotSaveHeader2007 {
            year: 2026,
            month: 11,
            day: 20,
            hour: 18,
            minute: 30,
            var1: 1,
            var2: 2,
            var3: 3,
            title_raw: trunc,
        },
        primary_snapshot: snap.clone(),
        primary_vars: [42i16; LOCAL_VARS_COUNT],
        rollback_snapshot: snap,
        rollback_vars: [7i16; LOCAL_VARS_COUNT],
    };
    let slot_bytes = slot.to_bytes();
    assert_eq!(slot_bytes.len(), SLOT_SAVE_SIZE);
    let slot_sha = sha256_hex(&slot_bytes);
    let slot_rt = SlotSave2007::from_bytes(&slot_bytes).unwrap();
    assert_eq!(sha256_hex(&slot_rt.to_bytes()), slot_sha);
}

#[test]
fn c_abi_ffi_smoke_lifecycle_font_tick_bgra8_audio_save_and_dialogs() {
    if !require_real_assets_or_skip("c_abi_ffi_smoke_lifecycle_font_tick_bgra8_audio_save_and_dialogs")
    {
        return;
    }
    let font_bytes = find_test_gothic_font();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let save_dir = std::env::temp_dir().join(format!("fujihime_ffi_test_{nonce}"));
    fs::create_dir_all(&save_dir).unwrap();

    let c_game = CString::new(GAME_DIR).unwrap();
    let c_save = CString::new(save_dir.to_str().unwrap()).unwrap();
    let cfg = FujihimeEngineConfig {
        game_root: c_game.as_ptr(),
        save_dir: c_save.as_ptr(),
        override_dir: std::ptr::null(),
        orig_backup_dir: std::ptr::null(),
        language_mode: FujihimeLanguageMode::Japanese,
        repair_chs_recall_labels: false,
    };

    unsafe {
        let eng = fujihime_engine_create(&cfg);
        assert!(!eng.is_null());

        assert_eq!(
            fujihime_engine_load_font(
                eng,
                FujihimeFontRole::Gothic,
                font_bytes.as_ptr(),
                font_bytes.len(),
                0,
            ),
            FUJIHIME_OK
        );

        assert_eq!(fujihime_engine_start_game(eng, 1102), FUJIHIME_OK);
        for _ in 0..350 {
            assert_eq!(fujihime_engine_tick(eng, 50), FUJIHIME_OK);
        }

        let mut st = FujihimeEngineState::default();
        assert_eq!(fujihime_engine_get_state(eng, &mut st), FUJIHIME_OK);
        assert_eq!(st.scene, FujihimeSceneKind::MainGame);
        assert_eq!(st.language_mode, FujihimeLanguageMode::Japanese);
        assert_eq!(st.current_script_id, 1102);

        // Copy 800x600x4 BGRA8 presentation framebuffer
        let mut bgra = vec![0u8; FUJIHIME_PRESENT_BGRA8_LEN];
        assert_eq!(
            fujihime_engine_copy_present_bgra8(eng, bgra.as_mut_ptr(), bgra.len()),
            FUJIHIME_OK
        );
        assert_eq!(bgra[3], 0xFF);

        // Pull interleaved stereo f32 audio
        let mut pcm = vec![0.0f32; 512 * 2];
        assert_eq!(fujihime_engine_render_audio_f32(eng, pcm.as_mut_ptr(), 512), 512);
        assert!(pcm.iter().all(|s| s.is_finite()));

        // Pointer & key events + save/load/rollback
        assert_eq!(
            fujihime_engine_pointer_event(eng, FujihimePointerKind::Move, 100, 100),
            FUJIHIME_OK
        );
        assert_eq!(fujihime_engine_key_event(eng, 0x70, true), FUJIHIME_OK); // VK_F1 no-op
        assert_eq!(fujihime_engine_save_global(eng), FUJIHIME_OK);
        assert_eq!(fujihime_engine_save_slot(eng, 2, 2026, 11, 20, 19, 0), FUJIHIME_OK);
        assert_eq!(fujihime_engine_load_slot(eng, 2), FUJIHIME_OK);
        assert_eq!(fujihime_engine_rollback(eng), FUJIHIME_OK);

        // Verify no dialog pending initially; check null-safety on poll
        let mut dlg = FujihimePendingDialog::default();
        assert!(!fujihime_engine_poll_pending_dialog(eng, &mut dlg));
        assert_eq!(
            fujihime_engine_confirm_dialog_response(eng, 999, false),
            FUJIHIME_OK
        );

        fujihime_engine_destroy(eng);
    }

    // Also test native confirmation dialog wording directly via Rust `Engine::request_confirm`
    let mut rust_eng = fujihime_core::engine::Engine::create(
        fujihime_core::engine::EngineConfig::new(
            GAME_DIR,
            &save_dir,
            fujihime_core::vfs::LanguageMode::Japanese,
        ),
    )
    .unwrap();
    let did = rust_eng.request_confirm(ConfirmAction::ExitGame);
    let dlg = rust_eng.poll_pending_dialog().unwrap();
    assert_eq!(dlg.id, did);
    assert_eq!(dlg.kind, fujihime_core::engine::DialogKind::YesNoConfirm);
    assert_eq!(dlg.title, "終了確認");
    rust_eng.confirm_dialog_response(did, true).unwrap();
    assert!(rust_eng.status().exit_requested);
    assert!(save_dir.join("KhDLsave.dat").is_file());

    // Confirm enum variant export check
    let _ = FujihimeDialogKind::YesNoConfirm;

    let _ = fs::remove_dir_all(&save_dir);
}

#[test]
fn c_abi_ffi_synthetic_lifecycle_and_dialogs() {
    let font_bytes = find_test_gothic_font();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("fujihime_ffi_synth_{nonce}"));
    let game_dir = root.join("game");
    let save_dir = root.join("save");
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&save_dir).unwrap();

    let c_game = CString::new(game_dir.to_str().unwrap()).unwrap();
    let c_save = CString::new(save_dir.to_str().unwrap()).unwrap();
    let cfg = FujihimeEngineConfig {
        game_root: c_game.as_ptr(),
        save_dir: c_save.as_ptr(),
        override_dir: std::ptr::null(),
        orig_backup_dir: std::ptr::null(),
        language_mode: FujihimeLanguageMode::Japanese,
        repair_chs_recall_labels: false,
    };

    unsafe {
        assert!(fujihime_engine_create(std::ptr::null()).is_null());
        let eng = fujihime_engine_create(&cfg);
        assert!(!eng.is_null());

        assert_eq!(
            fujihime_engine_load_font(
                eng,
                FujihimeFontRole::Gothic,
                font_bytes.as_ptr(),
                font_bytes.len(),
                0,
            ),
            FUJIHIME_OK
        );
        assert_eq!(fujihime_engine_tick(eng, 16), FUJIHIME_OK);

        let mut bgra = vec![0u8; FUJIHIME_PRESENT_BGRA8_LEN];
        assert_eq!(
            fujihime_engine_copy_present_bgra8(eng, bgra.as_mut_ptr(), bgra.len()),
            FUJIHIME_OK
        );
        assert_eq!(bgra[3], 0xFF);

        let mut pcm = vec![0.0f32; 256 * 2];
        assert_eq!(fujihime_engine_render_audio_f32(eng, pcm.as_mut_ptr(), 256), 256);

        assert_eq!(
            fujihime_engine_pointer_event(eng, FujihimePointerKind::Move, 400, 300),
            FUJIHIME_OK
        );
        assert_eq!(fujihime_engine_key_event(eng, 0x70, true), FUJIHIME_OK);
        assert_eq!(fujihime_engine_save_global(eng), FUJIHIME_OK);
        assert!(save_dir.join("KhDLsave.dat").is_file());

        assert_eq!(
            fujihime_engine_perform_action(eng, FujihimeSystemAction::Exit),
            FUJIHIME_OK
        );
        let mut dlg = FujihimePendingDialog::default();
        assert!(fujihime_engine_poll_pending_dialog(eng, &mut dlg));
        assert_eq!(dlg.kind, FujihimeDialogKind::YesNoConfirm);
        assert_eq!(
            fujihime_engine_confirm_dialog_response(eng, dlg.dialog_id, true),
            FUJIHIME_OK
        );

        let mut st = FujihimeEngineState::default();
        assert_eq!(fujihime_engine_get_state(eng, &mut st), FUJIHIME_OK);
        assert!(st.exit_requested);

        fujihime_engine_destroy(eng);
        fujihime_engine_destroy(std::ptr::null_mut());
    }

    let _ = fs::remove_dir_all(&root);
}
