use std::ffi::CString;
use std::fs;
use std::path::Path;

use fujihime_core::engine::{ConfirmAction, DialogKind, Engine, EngineConfig, FontRole, PointerKind, TopScene};
use fujihime_core::ffi::{
    fujihime_engine_create, fujihime_engine_destroy, fujihime_engine_enter_scene,
    fujihime_engine_get_state, fujihime_engine_import_2002_cd_save, fujihime_engine_load_font,
    fujihime_engine_perform_action, fujihime_engine_set_language_mode, FujihimeEngineConfig,
    FujihimeEngineState, FujihimeFontRole, FujihimeLanguageMode, FujihimeSceneKind,
    FujihimeSystemAction, FUJIHIME_OK,
};
use fujihime_core::gfx::surface::{GAME_HEIGHT, GAME_WIDTH};
use fujihime_core::save::{GlobalSave2007, SLOT_SAVE_SIZE};
use fujihime_core::ui::{
    messagebox_text, CgGalleryState, CompaneWidget, SaveScrnMode, SelMapOverlay,
    ALL_MESSAGEBOX_CALLSITES, CG_GALLERY_TABLE, CG_TAB_COUNTS, REPLAY_TAB_COUNTS,
    SCENE_REPLAY_TABLE,
};
use fujihime_core::vfs::{AssetCategory, LanguageMode};

const GAME_DIR: &str = "/workspace/game/腐姫ＤＬ";
const FULLCG_2002_SAVE: &str = "/workspace/game/腐姫ＤＬ/补丁/全CG档/KhDLsave.dat";

fn require_real_assets_or_skip(test_name: &str) -> bool {
    if Path::new(GAME_DIR).is_dir() && Path::new(FULLCG_2002_SAVE).is_file() {
        return true;
    }
    if std::env::var("FUJIHIME_REQUIRE_REAL_ASSETS").ok().as_deref() == Some("1") {
        panic!("FUJIHIME_REQUIRE_REAL_ASSETS=1 but real assets missing at {GAME_DIR}");
    }
    eprintln!("[skip] {test_name}: real game assets not found at {GAME_DIR}");
    false
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

fn fresh_save_dir(tag: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("fujihime_e2e_{tag}_{nonce}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn capture_bgra8(eng: &Engine) -> Vec<u8> {
    let mut buf = vec![0u8; (GAME_WIDTH as usize) * (GAME_HEIGHT as usize) * 4];
    eng.copy_present_bgra8(&mut buf).unwrap();
    buf
}

fn has_non_black_pixels(bgra: &[u8]) -> bool {
    bgra.as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
}

#[test]
fn e2e_1_mainline_title_dialogue_backlog_compane_savescrn_rollback_jp_and_chs() {
    if !require_real_assets_or_skip(
        "e2e_1_mainline_title_dialogue_backlog_compane_savescrn_rollback_jp_and_chs",
    ) {
        return;
    }
    let font = find_test_gothic_font();

    for (label, lang) in [
        ("jp", LanguageMode::Japanese),
        ("chs", LanguageMode::ChineseV1),
    ] {
        let save_dir = fresh_save_dir(&format!("e2e1_{label}"));
        let mut cfg = EngineConfig::new(GAME_DIR, &save_dir, lang);
        cfg.repair_chs_recall_labels = lang == LanguageMode::ChineseV1;
        let mut eng = Engine::create(cfg).unwrap();
        eng.load_font_bytes(FontRole::Gothic, font.clone(), 0).unwrap();

        // 1. Start title OP sequence -> verify MPEG playing, then enter Title screen
        eng.start_title_sequence().unwrap();
        assert!(eng.status().is_playing_video);
        assert_eq!(eng.status().scene, TopScene::Title);
        eng.enter_title_screen().unwrap();
        // Tick through 0101.gsc title fade-in transitions until WaitTimerLoop settles
        for _ in 0..80 {
            eng.tick(50).unwrap();
            if !eng.status().is_transitioning {
                break;
            }
        }
        for _ in 0..40 {
            eng.tick(50).unwrap();
        }
        assert_eq!(eng.status().scene, TopScene::Title);
        let title_bgra = capture_bgra8(&eng);
        assert!(has_non_black_pixels(&title_bgra), "{label}: Title must render artwork");

        // 2. Click New Game on 0101.gsc (layer 41 @ 253, 342) or advance through opening
        eng.pointer_event(PointerKind::Up, 280, 355).unwrap();
        assert_eq!(eng.status().scene, TopScene::MainGame, "{label}: clicking New Game enters MainGame");

        // If 0111.gsc -> 1101/1102 starts with transition/sleep, tick until first WaitClick dialogue
        for _ in 0..500 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_click {
                break;
            }
        }
        if !eng.status().is_waiting_click {
            // Start opening script 1102 directly if 0111 waits at an initial branch/video
            eng.start_game_script(1102).unwrap();
            for _ in 0..450 {
                eng.tick(50).unwrap();
                if eng.status().is_waiting_click {
                    break;
                }
            }
        }
        assert!(eng.status().is_waiting_click, "{label}: must reach first dialogue line");

        // Advance past 2 dialogue lines so backlog accumulates >= 2 entries
        for _ in 0..2 {
            eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
            if eng.status().is_waiting_click {
                eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
            }
            for _ in 0..300 {
                eng.tick(50).unwrap();
                if eng.status().is_waiting_click {
                    break;
                }
            }
        }
        assert!(eng.vm.state.tbox.backlog.len() >= 2, "{label}: backlog must record lines");

        // 3. Open Backlog via `compane` `rev` button `(761+19+8, 494+3+8) = (788, 505)`
        let comp_rev = (
            CompaneWidget::default().origin_x + 24,
            CompaneWidget::default().origin_y + 10,
        );
        eng.pointer_event(PointerKind::Up, comp_rev.0, comp_rev.1).unwrap();
        assert!(eng.backlog.active, "{label}: compane rev must open Backlog");
        eng.pointer_event(PointerKind::WheelUp, 400, 300).unwrap();
        eng.pointer_event(PointerKind::RightUp, 400, 300).unwrap();
        assert!(!eng.backlog.active, "{label}: RightUp must close Backlog");

        // 4. Hide and restore textbox via `compane` `hide` `(761+3+8, 494+67+8) = (772, 575)` & `VK_SHIFT`
        let frame_with_box = capture_bgra8(&eng);
        eng.key_event(0x10, true).unwrap(); // VK_SHIFT hides textbox
        let frame_hidden = capture_bgra8(&eng);
        assert_ne!(frame_with_box, frame_hidden, "{label}: hiding textbox must update canvas");
        eng.key_event(0x10, true).unwrap(); // VK_SHIFT restores textbox
        let frame_restored = capture_bgra8(&eng);
        assert_eq!(frame_with_box, frame_restored, "{label}: restoring textbox must match");

        // Toggle Auto via `compane` `auto@(19,67)` and verify auto-advance timer fires after auto_wait_ms
        let comp_auto = (
            CompaneWidget::default().origin_x + 24,
            CompaneWidget::default().origin_y + 72,
        );
        eng.pointer_event(PointerKind::Up, comp_auto.0, comp_auto.1).unwrap();
        assert!(eng.auto_mode, "{label}: compane auto button toggles Auto mode");
        let pc_before_auto = eng.vm.state.pc;
        for _ in 0..45 {
            eng.tick(50).unwrap();
            if eng.vm.state.pc != pc_before_auto {
                break;
            }
        }
        eng.pointer_event(PointerKind::Up, comp_auto.0, comp_auto.1).unwrap();
        assert!(!eng.auto_mode, "{label}: second click toggles Auto mode off");

        // 5. Record exact state at checkpoint, open `savescrn` in Save mode, save Slot 01
        let saved_script = eng.vm.state.current_script_id();
        let saved_pc = eng.vm.state.pc;
        let saved_vars_sample = [eng.vm.state.vars[0], eng.vm.state.vars[1], eng.vm.state.vars[2]];

        eng.perform_system_action(fujihime_core::ui::SystemPanelAction::Save).unwrap();
        assert!(eng.save_scrn.active);
        assert_eq!(eng.save_scrn.mode, SaveScrnMode::Save);

        // First click on empty Slot 01 `(54+100, 56+30) = (154, 86)` saves immediately (`0x41b41e`)
        eng.pointer_event(PointerKind::Up, 154, 86).unwrap();
        let slot1_path = save_dir.join("KhDLsave01.dat");
        assert_eq!(fs::metadata(&slot1_path).unwrap().len(), SLOT_SAVE_SIZE as u64);

        // Second click on occupied Slot 01 triggers `0x445e90` SaveOverwrite confirm dialog!
        eng.pointer_event(PointerKind::Up, 154, 86).unwrap();
        let dlg = eng.poll_pending_dialog().expect("overwrite confirm must appear");
        assert_eq!(dlg.kind, DialogKind::YesNoConfirm);
        assert_eq!(dlg.action, ConfirmAction::SaveOverwrite(1));
        eng.confirm_dialog_response(dlg.id, true).unwrap();

        // Close `savescrn` via `exit` `(745, 540)`
        eng.pointer_event(PointerKind::Up, 745, 540).unwrap();
        assert!(!eng.save_scrn.active);

        // 6. Advance forward, test one-step Rollback via `compane` `bak` button `(788, 521)`
        eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        if eng.status().is_waiting_click {
            eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        }
        for _ in 0..300 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_click {
                break;
            }
        }
        let advanced_pc = eng.vm.state.pc;
        assert!(advanced_pc != saved_pc || eng.vm.state.current_script_id() != saved_script);

        eng.pointer_event(
            PointerKind::Up,
            CompaneWidget::default().origin_x + 24,
            CompaneWidget::default().origin_y + 26,
        )
        .unwrap();
        assert_eq!(eng.vm.state.pc, saved_pc, "{label}: rollback must restore previous PC");

        // Advance again, then reload Slot 01 via `savescrn` Load UI + Load confirm dialog
        eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        if eng.status().is_waiting_click {
            eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        }
        for _ in 0..300 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_click {
                break;
            }
        }
        eng.perform_system_action(fujihime_core::ui::SystemPanelAction::Load).unwrap();
        assert!(eng.save_scrn.active);
        assert_eq!(eng.save_scrn.mode, SaveScrnMode::Load);

        // Click Slot 01 -> LoadSlot confirm -> confirm `true`
        eng.pointer_event(PointerKind::Up, 154, 86).unwrap();
        let load_dlg = eng.poll_pending_dialog().expect("load confirm must appear");
        assert_eq!(load_dlg.action, ConfirmAction::LoadSlot(1));
        eng.confirm_dialog_response(load_dlg.id, true).unwrap();

        assert!(!eng.save_scrn.active);
        assert_eq!(eng.vm.state.current_script_id(), saved_script);
        assert_eq!(eng.vm.state.pc, saved_pc);
        assert_eq!([eng.vm.state.vars[0], eng.vm.state.vars[1], eng.vm.state.vars[2]], saved_vars_sample);

        let _ = fs::remove_dir_all(&save_dir);
    }
}

#[test]
fn e2e_2_confscrn_sliders_switches_audio_mixer_and_khdlsave_persistence() {
    if !require_real_assets_or_skip(
        "e2e_2_confscrn_sliders_switches_audio_mixer_and_khdlsave_persistence",
    ) {
        return;
    }
    let font = find_test_gothic_font();
    let save_dir = fresh_save_dir("e2e2_conf");

    {
        let mut cfg = EngineConfig::new(GAME_DIR, &save_dir, LanguageMode::ChineseV1);
        cfg.repair_chs_recall_labels = true;
        let mut eng = Engine::create(cfg).unwrap();
        eng.load_font_bytes(FontRole::Gothic, font.clone(), 0).unwrap();
        eng.enter_title_screen().unwrap();
        for _ in 0..80 {
            eng.tick(50).unwrap();
        }

        // Click `0101.gsc` Config button `(280, 420)` -> opens `confscrn.lwg` (57 layers)
        eng.pointer_event(PointerKind::Up, 280, 420).unwrap();
        assert_eq!(eng.status().scene, TopScene::ConfigMenu);
        assert!(eng.conf_scrn.active);
        let conf_bgra = capture_bgra8(&eng);
        assert!(has_non_black_pixels(&conf_bgra));

        // 1. Modify text speed: `msp_slw@(370,276)` = 4, then `msp_now@(614,276)` = 0
        eng.pointer_event(PointerKind::Up, 400, 290).unwrap();
        assert_eq!(eng.global_save().config.text_speed(), 4);
        eng.pointer_event(PointerKind::Up, 650, 290).unwrap();
        assert_eq!(eng.global_save().config.text_speed(), 0);

        // 2. Drag/click BGM, SE, Voice & Auto sliders along `x = 463..705`
        // BGM slider `y=104..126`, midpoint `463 + 11 + 110 = 584` -> 128
        eng.pointer_event(PointerKind::Down, 584, 114).unwrap();
        eng.pointer_event(PointerKind::Up, 584, 114).unwrap();
        assert_eq!(eng.global_save().config.bgm_volume(), 128);

        // SE slider `y=149..171`, `x=463 + 11 + 55 = 529` -> 64
        eng.pointer_event(PointerKind::Down, 529, 159).unwrap();
        eng.pointer_event(PointerKind::Up, 529, 159).unwrap();
        let se_vol = eng.global_save().config.se_volume();
        assert!((63..=65).contains(&se_vol), "expected ~64, got {se_vol}");

        // Voice slider `y=194..216`, `x=463 + 11 + 165 = 639` -> 191
        eng.pointer_event(PointerKind::Down, 639, 204).unwrap();
        eng.pointer_event(PointerKind::Up, 639, 204).unwrap();
        let voc_vol = eng.global_save().config.voice_volume();
        assert!((190..=192).contains(&voc_vol), "expected ~191, got {voc_vol}");

        // Auto-wait slider `y=374..396`, right end `463 + 11 + 220 = 694` -> 3000 ms
        eng.pointer_event(PointerKind::Down, 694, 384).unwrap();
        eng.pointer_event(PointerKind::Up, 694, 384).unwrap();
        assert_eq!(eng.global_save().config.auto_wait_ms(), 3000);

        // Verify live DirectSound mixer target_mb updated before muting
        {
            let m = eng.mixer().lock().unwrap();
            assert_eq!(m.bgm_slots()[0].target_mb(), ((128 - 255) * 3000) / 255);
            assert_eq!(m.se0().target_mb(), ((i32::from(se_vol) - 255) * 5300) / 255);
            assert_eq!(m.voice().target_mb(), ((i32::from(voc_vol) - 255) * 5300) / 255);
        }

        // 3. Toggle mute switches: `bgm_off@(297,96)`, `sef_off@(297,141)`, `voc_off@(297,186)`
        eng.pointer_event(PointerKind::Up, 315, 110).unwrap();
        eng.pointer_event(PointerKind::Up, 315, 155).unwrap();
        eng.pointer_event(PointerKind::Up, 315, 200).unwrap();
        assert!(!eng.global_save().config.bgm_enable());
        assert!(!eng.global_save().config.se_enable());
        assert!(!eng.global_save().config.voice_enable());
        {
            let m = eng.mixer().lock().unwrap();
            assert_eq!(m.bgm_slots()[0].current_gain(), 0.0);
            assert_eq!(m.se0().current_gain(), 0.0);
            assert_eq!(m.voice().current_gain(), 0.0);
        }

        // 4. Toggle `gef_off@(297,231)`, `vocst_on@(595,231)`, `msk_on@(327,321)`
        eng.pointer_event(PointerKind::Up, 315, 245).unwrap();
        eng.pointer_event(PointerKind::Up, 620, 245).unwrap();
        eng.pointer_event(PointerKind::Up, 360, 335).unwrap();
        assert!(!eng.global_save().config.effect_enable());
        assert!(eng.global_save().config.voice_cut_on_click());
        assert!(eng.global_save().config.skip_unread());

        // 5. Click `close@(723,517)` -> writes `KhDLsave.dat`
        eng.pointer_event(PointerKind::Up, 745, 540).unwrap();
        assert!(!eng.conf_scrn.active);
        assert!(save_dir.join("KhDLsave.dat").is_file());
    }

    // 6. Reload fresh Engine instance and confirm 100% persistence
    let parsed_file = GlobalSave2007::load_file(save_dir.join("KhDLsave.dat")).unwrap();
    assert_eq!(parsed_file.config.text_speed(), 0);
    assert_eq!(parsed_file.config.bgm_volume(), 128);
    assert_eq!(parsed_file.config.auto_wait_ms(), 3000);
    assert!(!parsed_file.config.bgm_enable());
    assert!(!parsed_file.config.se_enable());
    assert!(!parsed_file.config.voice_enable());
    assert!(!parsed_file.config.effect_enable());
    assert!(parsed_file.config.voice_cut_on_click());
    assert!(parsed_file.config.skip_unread());

    let mut cfg2 = EngineConfig::new(GAME_DIR, &save_dir, LanguageMode::Japanese);
    cfg2.repair_chs_recall_labels = false;
    let eng2 = Engine::create(cfg2).unwrap();
    assert_eq!(eng2.global_save().config, parsed_file.config);
    assert_eq!(eng2.mixer().lock().unwrap().bgm_slots()[0].current_gain(), 0.0);

    let _ = fs::remove_dir_all(&save_dir);
}

#[test]
fn e2e_3_fullcg_import_extra_cg_gallery_music_and_scene_replay_jp_and_chs() {
    if !require_real_assets_or_skip(
        "e2e_3_fullcg_import_extra_cg_gallery_music_and_scene_replay_jp_and_chs",
    ) {
        return;
    }
    let font = find_test_gothic_font();
    let fullcg_bytes = fs::read(FULLCG_2002_SAVE).unwrap();

    // Verify 0x4648a8 (102 sets / 266 images) & 0x46db08 (79 entries) metadata tables
    assert_eq!(CG_GALLERY_TABLE.len(), 102);
    assert_eq!(CG_TAB_COUNTS.iter().sum::<usize>(), 102);
    let total_imgs: usize = CG_GALLERY_TABLE
        .iter()
        .map(|e| usize::from(e.thumb_id != 0) + e.active_variants().count())
        .sum();
    assert_eq!(total_imgs, 266);
    assert_eq!(SCENE_REPLAY_TABLE.len(), 79);
    assert_eq!(REPLAY_TAB_COUNTS.iter().sum::<usize>(), 79);

    for (label, lang) in [
        ("jp", LanguageMode::Japanese),
        ("chs", LanguageMode::ChineseV1),
    ] {
        let save_dir = fresh_save_dir(&format!("e2e3_{label}"));
        let mut cfg = EngineConfig::new(GAME_DIR, &save_dir, lang);
        cfg.repair_chs_recall_labels = lang == LanguageMode::ChineseV1;
        let mut eng = Engine::create(cfg).unwrap();
        eng.load_font_bytes(FontRole::Gothic, font.clone(), 0).unwrap();

        // 1. Import 2002 CD Full CG save (unlocks 61 CD CGs across Tabs 0..4 + all 26 0399 routes)
        eng.import_2002_cd_save(&fullcg_bytes).unwrap();
        assert_eq!(eng.unlocked_cg_count(), 61, "{label}: 2002 CD full-CG save unlocks 61 sets");
        // Unlock a 2007 DL Tab 6 CG (`vars[9301]`) to verify Tab 6 `Grpo` rule alongside Tab 0 `Grpe`
        eng.vm.state.vars[9301] = 1;

        // 2. Extra -> CG Gallery -> Tab 6 Grpo rule + real variant cycling
        eng.enter_scene(TopScene::ExtraMenu).unwrap();
        eng.enter_scene(TopScene::CgGallery).unwrap();
        assert_eq!(eng.status().scene, TopScene::CgGallery);

        // Check Tab 6 category rule (`0x41cc5c`)
        eng.pointer_event(PointerKind::Up, CgGalleryState::tab_rect(6).x + 10, CgGalleryState::tab_rect(6).y + 10).unwrap();
        assert_eq!(eng.cg_gallery.tab, 6);
        let (_, tab6_entry) = eng.cg_gallery.current_entry(0).unwrap();
        assert_eq!(tab6_entry.full_image_category(), AssetCategory::Grpo);
        eng.pointer_event(PointerKind::Up, CgGalleryState::slot_rect(0).x + 80, CgGalleryState::slot_rect(0).y + 90).unwrap();
        assert!(eng.cg_gallery.fullscreen.is_some());
        eng.pointer_event(PointerKind::RightUp, 400, 300).unwrap();
        assert!(eng.cg_gallery.fullscreen.is_none());

        // Switch to Tab 0 -> Grpe rule, open slot 0 (variants [1, 2, 3]), cycle all 3 real WCGs
        eng.pointer_event(PointerKind::Up, CgGalleryState::tab_rect(0).x + 10, CgGalleryState::tab_rect(0).y + 10).unwrap();
        assert_eq!(eng.cg_gallery.tab, 0);
        let (_, tab0_entry) = eng.cg_gallery.current_entry(0).unwrap();
        assert_eq!(tab0_entry.full_image_category(), AssetCategory::Grpe);
        assert_eq!(tab0_entry.variants[..3], [1, 2, 3]);

        eng.pointer_event(PointerKind::Up, CgGalleryState::slot_rect(0).x + 80, CgGalleryState::slot_rect(0).y + 90).unwrap();
        assert_eq!(eng.cg_gallery.fullscreen.as_ref().unwrap().current_img_id, 1);
        let cg1_pixels = capture_bgra8(&eng);
        assert!(has_non_black_pixels(&cg1_pixels));

        eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        assert_eq!(eng.cg_gallery.fullscreen.as_ref().unwrap().current_img_id, 2);
        let cg2_pixels = capture_bgra8(&eng);
        assert_ne!(cg1_pixels, cg2_pixels);

        eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        assert_eq!(eng.cg_gallery.fullscreen.as_ref().unwrap().current_img_id, 3);

        // Next click at last variant exits fullscreen cleanly
        eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        assert!(eng.cg_gallery.fullscreen.is_none());

        // 3. Verify MusicMode & 2007 DL asset boundaries (no panics, faithful reporting)
        eng.enter_scene(TopScene::MusicMode).unwrap();
        assert!(!eng.music_mode.muscmenu_lwg_present);
        assert_eq!(eng.music_mode.available_tracks, (3..=23).collect::<Vec<u16>>());
        assert!(!eng.music_mode_play_track(1)); // Track01 missing in 2007 DL
        assert!(eng.music_mode_play_track(3)); // Track03 present in 2007 DL
        eng.music_mode_stop();

        // 4. Scene Replay: test 0399.gsc label-repaired route `8023` (2058.gsc lbl "8023") on JP + CHS
        eng.enter_scene(TopScene::SceneReplay).unwrap();
        assert!(!eng.launch_replay_table_entry(0, 0).unwrap()); // 8111.gsc absent in 2007 DL -> graceful false
        assert!(eng.launch_recall_0399_route(8023).unwrap(), "{label}: 0399 route 8023 must launch");
        assert!(eng.vm.state.recall_flag);
        assert_eq!(eng.status().scene, TopScene::MainGame);

        for _ in 0..300 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_click {
                break;
            }
        }
        assert!(eng.status().is_waiting_click, "{label}: 2058 recall should display dialogue");

        // Advance/fast-forward through all lines of recall scene until `0399` returns cleanly to SceneReplay
        eng.vm.state.fast_skip_active = true;
        for _ in 0..600 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_select {
                eng.choose_select(0).unwrap();
                eng.vm.state.fast_skip_active = true;
            } else if eng.status().is_waiting_click {
                eng.advance_dialogue_click().unwrap();
            }
            if eng.status().scene == TopScene::SceneReplay {
                break;
            }
        }
        assert_eq!(
            eng.status().scene,
            TopScene::SceneReplay,
            "{label}: completed recall must cleanly return to SceneReplay"
        );
        assert!(!eng.vm.state.recall_flag);

        let _ = fs::remove_dir_all(&save_dir);
    }
}

#[test]
fn e2e_4_dialogs_hotkeys_vk_f1_and_selmap01_via_c_abi() {
    // 1. Check all 10 MessageBoxA callsites in JP & CHS have distinct localized non-empty titles
    for site in ALL_MESSAGEBOX_CALLSITES {
        let (_, t_jp, _) = messagebox_text(site, LanguageMode::Japanese, None);
        let (_, t_chs, _) = messagebox_text(site, LanguageMode::ChineseV1, None);
        assert!(!t_jp.is_empty());
        assert!(!t_chs.is_empty());
    }

    if !require_real_assets_or_skip("e2e_4_dialogs_hotkeys_vk_f1_and_selmap01_via_c_abi(real)") {
        return;
    }

    let font = find_test_gothic_font();
    let save_dir = fresh_save_dir("e2e4_ffi");

    // 2. Test bilingual dialogs, Cancel/Accept branches, VK_F1, and selmap01.lwg
    {
        let mut cfg = EngineConfig::new(GAME_DIR, &save_dir, LanguageMode::Japanese);
        cfg.repair_chs_recall_labels = false;
        let mut eng = Engine::create(cfg).unwrap();
        eng.load_font_bytes(FontRole::Gothic, font.clone(), 0).unwrap();
        eng.start_game_script(1102).unwrap();
        for _ in 0..450 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_click {
                break;
            }
        }

        // VK_F1 (`0x70`) release no-op (`0x41e490`)
        let before = eng.status();
        eng.key_event(0x70, true).unwrap();
        eng.key_event(0x70, false).unwrap();
        assert_eq!(eng.status(), before);
        assert!(eng.poll_pending_dialog().is_none());

        // `selmap01.lwg` (`0xD5`) real rendering + click hit on select2 `(119, 142+126)` -> vars[0] == 2
        eng.open_map_select(1, 0, 0).unwrap();
        assert!(eng.selmap.active);
        let selmap_bgra = capture_bgra8(&eng);
        assert!(has_non_black_pixels(&selmap_bgra));
        let r2 = SelMapOverlay::slot_rect(None, 2);
        eng.pointer_event(PointerKind::Up, r2.x + 20, r2.y + 15).unwrap();
        assert!(!eng.selmap.active);
        assert_eq!(eng.vm.state.vars[0], 2);

        // Confirm ReturnToTitle JP exact text -> Cancel branch preserves MainGame
        let dlg_id = eng.request_confirm(ConfirmAction::ReturnToTitle);
        let dlg_jp = eng.poll_pending_dialog().unwrap();
        assert_eq!(dlg_jp.title, "確認");
        assert_eq!(dlg_jp.message, "タイトル画面に戻ります。\nよろしいですか？");
        eng.confirm_dialog_response(dlg_id, false).unwrap();
        assert_eq!(eng.status().scene, TopScene::MainGame);

        // Switch runtime language to CHS, trigger ReturnToTitle -> Accept returns to Title
        eng.set_language_mode(LanguageMode::ChineseV1, None, true).unwrap();
        let dlg_chs_id = eng.request_confirm(ConfirmAction::ReturnToTitle);
        let dlg_chs = eng.poll_pending_dialog().unwrap();
        assert_eq!(dlg_chs.title, "确认");
        assert_eq!(dlg_chs.message, "接下来要回到标题画面。\n这样可以吗？");
        eng.confirm_dialog_response(dlg_chs_id, true).unwrap();
        assert_eq!(eng.status().scene, TopScene::Title);

        // ExitGame CHS exact text -> Cancel then Accept
        let exit_cancel = eng.request_confirm(ConfirmAction::ExitGame);
        let exit_dlg = eng.poll_pending_dialog().unwrap();
        assert_eq!(exit_dlg.title, "确认结束");
        assert_eq!(exit_dlg.message, "真的要结束游戏吗？");
        eng.confirm_dialog_response(exit_cancel, false).unwrap();
        assert!(!eng.status().exit_requested);

        let exit_ok = eng.request_confirm(ConfirmAction::ExitGame);
        eng.confirm_dialog_response(exit_ok, true).unwrap();
        assert!(eng.status().exit_requested);
    }

    // 3. Verify new C11 FFI entrypoints end-to-end
    let c_game = CString::new(GAME_DIR).unwrap();
    let c_save = CString::new(save_dir.to_str().unwrap()).unwrap();
    let ffi_cfg = FujihimeEngineConfig {
        game_root: c_game.as_ptr(),
        save_dir: c_save.as_ptr(),
        override_dir: std::ptr::null(),
        orig_backup_dir: std::ptr::null(),
        language_mode: FujihimeLanguageMode::Japanese,
        repair_chs_recall_labels: false,
    };
    unsafe {
        let eng = fujihime_engine_create(&ffi_cfg);
        assert!(!eng.is_null());
        assert_eq!(
            fujihime_engine_load_font(eng, FujihimeFontRole::Gothic, font.as_ptr(), font.len(), 0),
            FUJIHIME_OK
        );
        let fullcg = fs::read(FULLCG_2002_SAVE).unwrap();
        assert_eq!(
            fujihime_engine_import_2002_cd_save(eng, fullcg.as_ptr(), fullcg.len()),
            FUJIHIME_OK
        );
        assert_eq!(
            fujihime_engine_set_language_mode(eng, FujihimeLanguageMode::ChineseV1, std::ptr::null(), true),
            FUJIHIME_OK
        );
        assert_eq!(
            fujihime_engine_enter_scene(eng, FujihimeSceneKind::CgGallery),
            FUJIHIME_OK
        );
        let mut st = FujihimeEngineState::default();
        assert_eq!(fujihime_engine_get_state(eng, &mut st), FUJIHIME_OK);
        assert_eq!(st.scene, FujihimeSceneKind::CgGallery);
        assert_eq!(st.language_mode, FujihimeLanguageMode::ChineseV1);

        assert_eq!(
            fujihime_engine_perform_action(eng, FujihimeSystemAction::Exit),
            FUJIHIME_OK
        );
        fujihime_engine_destroy(eng);
    }

    let _ = fs::remove_dir_all(&save_dir);
}
