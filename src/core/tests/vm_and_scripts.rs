use std::fs;
use std::path::Path;

use fujihime_core::engine::{Engine, EngineConfig, FontRole, PointerKind};
use fujihime_core::gfx::surface::{GAME_HEIGHT, GAME_WIDTH};
use fujihime_core::save::{SlotSave2007, SLOT_SAVE_SIZE};
use fujihime_core::text::caret::TextEncoding;
use fujihime_core::vfs::{AssetCategory, AssetVfs, LanguageMode, VfsConfig};
use fujihime_core::vm::{
    expand_script_string, GscQuirk, GscScript, HaltReason, VmHost, VmRuntime, VmState, VmYield,
};

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
fn real_all_341_orig_and_341_chs_gsc_parse_and_disassemble() {
    if !require_real_assets_or_skip("real_all_341_orig_and_341_chs_gsc_parse_and_disassemble") {
        return;
    }
    let jp_vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();
    let chs_vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::ChineseV1)).unwrap();

    let mut jp_scripts = std::collections::HashMap::new();

    for (label, vfs, allow_quirks, enc) in [
        ("orig", &jp_vfs, false, TextEncoding::Cp932),
        ("chs", &chs_vfs, true, TextEncoding::Gbk),
    ] {
        let scr = vfs.archive(AssetCategory::Scr).unwrap();
        assert_eq!(scr.len(), 341, "{label} scr.xfl count");

        let mut total_code_bytes = 0usize;
        let mut total_insns = 0usize;
        let mut total_jumps = 0usize;
        let mut total_eof_jumps = 0usize;
        let mut total_strings = 0usize;
        let mut total_str_refs = 0usize;
        let mut quirk_names = Vec::new();
        let mut zeroed_lbl_names = Vec::new();

        for entry in scr.entries() {
            let sid: u16 = entry
                .name
                .trim_end_matches(".gsc")
                .trim_end_matches(".GSC")
                .parse()
                .unwrap_or(0);
            let raw = scr.read_entry(entry).unwrap();
            let mut gsc = GscScript::parse(sid, &raw, allow_quirks)
                .unwrap_or_else(|e| panic!("{label}/{} parse failed: {e}", entry.name));

            for i in 0..gsc.str_count() {
                gsc.decode_string(i as u32, enc)
                    .unwrap_or_else(|e| panic!("{label}/{} str[{i}] decode failed: {e}", entry.name));
            }

            let dis = gsc
                .disassemble_all()
                .unwrap_or_else(|e| panic!("{label}/{} disasm failed: {e}", entry.name));

            total_code_bytes += gsc.code.len();
            total_insns += dis.instructions.len();
            total_jumps += dis.jump_count;
            total_eof_jumps += dis.eof_jump_count;
            total_strings += gsc.str_count();
            total_str_refs += dis.str_ref_count;

            if gsc.quirk.is_some() {
                quirk_names.push((entry.name.clone(), gsc.quirk));
            }
            if gsc.chs_zeroed_labels {
                zeroed_lbl_names.push(entry.name.clone());
                if let Some(orig) = jp_scripts.get(&sid) {
                    gsc.repair_labels_from(orig);
                    assert!(!gsc.chs_zeroed_labels);
                    assert!(gsc.label_count() > 0);
                }
            }
            if !allow_quirks {
                jp_scripts.insert(sid, gsc);
            }
        }

        // Exact Phase 1B counts from `re/verify_gsc.py`
        assert_eq!(total_code_bytes, 1_125_986, "{label} code_bytes");
        assert_eq!(total_insns, 68_819, "{label} insn_count");
        assert_eq!(total_jumps, 1_607, "{label} jump_count");
        assert_eq!(total_eof_jumps, 20, "{label} eof_jumps");
        assert_eq!(total_strings, 19_161, "{label} str_count");
        assert_eq!(total_str_refs, 37_878, "{label} str_ref_count");

        if !allow_quirks {
            assert!(quirk_names.is_empty());
            assert!(zeroed_lbl_names.is_empty());
        } else {
            assert_eq!(quirk_names.len(), 4);
            assert!(quirk_names.iter().any(|(n, q)| n == "1122.gsc" && *q == Some(GscQuirk::TrailingNulPadding(2))));
            assert!(quirk_names.iter().any(|(n, q)| n == "2233.gsc" && *q == Some(GscQuirk::TrailingNulPadding(6))));
            assert!(quirk_names.iter().any(|(n, q)| n == "3202.gsc" && *q == Some(GscQuirk::TrailingNulPadding(1))));
            assert!(quirk_names.iter().any(|(n, q)| n == "1160.gsc" && *q == Some(GscQuirk::ShortZeroDummyTail)));
            assert_eq!(zeroed_lbl_names.len(), 9);
        }
    }
}

struct DummyHost {
    scripts: std::collections::HashMap<u16, GscScript>,
}

impl VmHost for DummyHost {
    fn load_script(&mut self, script_id: u16) -> fujihime_core::Result<GscScript> {
        Ok(self.scripts[&script_id].clone())
    }
    fn refresh_scene(&mut self, _state: &VmState) -> fujihime_core::Result<()> {
        Ok(())
    }
    fn start_transition(&mut self, _state: &VmState, _eid: u16, _steps: u16, _d: u16) -> fujihime_core::Result<()> {
        Ok(())
    }
    fn display_text(&mut self, _state: &VmState, _slot: u16, _raw: &[u8]) -> fujihime_core::Result<()> {
        Ok(())
    }
    fn play_bgm(&mut self, _id: u16, _lf: bool, _fm: u16) {}
    fn stop_bgm(&mut self, _fm: u16) {}
    fn load_se(&mut self, _id: u16) {}
    fn play_se(&mut self, _lc: i16, _ff: u16, _f50: i16) {}
    fn stop_se(&mut self, _fm: u16) {}
    fn play_voice(&mut self, _id: u16, _lc: i16, _ff: u16, _d50: i16) {}
    fn stop_voice(&mut self, _fm: u16) {}
    fn start_video(&mut self, _id: u16) {}
    fn is_audio_playing(&self, _is_voice: bool) -> bool {
        false
    }
    fn voice_cut_on_click(&self) -> bool {
        true
    }
    fn skip_unread(&self) -> bool {
        false
    }
}

fn build_synthetic_gsc(code: &[u8], strs: &[&[u8]], arrs: &[&[u16]]) -> Vec<u8> {
    let mut str_offs = vec![0u32];
    let mut str_data = vec![0u8];
    for s in strs {
        str_offs.push(str_data.len() as u32);
        str_data.extend_from_slice(s);
        str_data.push(0);
    }

    let mut arr_offs = vec![0u32];
    let mut arr_data = vec![0u16];
    for a in arrs {
        arr_offs.push(arr_data.len() as u32);
        arr_data.push(a.len() as u16);
        arr_data.extend_from_slice(a);
    }

    let soff_sz = (str_offs.len() * 4) as u32;
    let sdat_sz = str_data.len() as u32;
    let aoff_sz = (arr_offs.len() * 4) as u32;
    let adat_cnt = arr_data.len() as u32;
    let loff_sz = 4u32;
    let ldat_sz = 1u32;
    let fsize = 36 + code.len() as u32 + soff_sz + sdat_sz + aoff_sz + adat_cnt * 2 + loff_sz * 2 + ldat_sz;

    let mut out = Vec::new();
    for v in [fsize, 36, code.len() as u32, soff_sz, sdat_sz, aoff_sz, adat_cnt, loff_sz, ldat_sz] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(code);
    for o in str_offs {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&str_data);
    for o in arr_offs {
        out.extend_from_slice(&o.to_le_bytes());
    }
    for u in arr_data {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // lbl name off
    out.extend_from_slice(&0u32.to_le_bytes()); // lbl target
    out.push(0); // lbl data
    out
}

#[test]
fn vm_all_15_hi_ops_indirection_interpolation_arrays_and_callstack() {
    // Test string interpolation (@var and $0..$9) with DBCS 0x5E/0x40 safety
    let mut vars = [0i16; 10_000];
    vars[10] = 42;
    vars[42] = -7;
    let mut str_vars = [[0u8; 201]; 10];
    str_vars[3][..4].copy_from_slice(b"HIME");
    // CP932 0x81 0x40 (ideographic space) + "@10 / @@10 / $3"
    let raw = b"\x81\x40@10 / @@10 / $3";
    let expanded = expand_script_string(raw, TextEncoding::Cp932, &vars, &str_vars);
    assert_eq!(expanded, b"\x81\x4042 / -7 / HIME");

    // Bytecode exercising all 15 HiOps (`0x1000..=0xF000`), indirect var access, `0x12 load_array`,
    // `0xCA memset_vars`, `0x6F muldiv`, `0x71 pow_i16`, `0x96..0x9F`, `0xC8 gosub_local`, `0x10 ret`, `0x9C` no-op.
    let mut code = Vec::new();
    let push_hi = |c: &mut Vec<u8>, op: u16, reg: u16, lhs: i16, rhs: i16| {
        c.extend_from_slice(&op.to_le_bytes());
        c.extend_from_slice(&reg.to_le_bytes());
        if (op & 0xF000) != 0xF000 {
            c.extend_from_slice(&lhs.to_le_bytes());
        }
        c.extend_from_slice(&rhs.to_le_bytes());
    };
    let push_expr = |c: &mut Vec<u8>, val: i16, dep: i16| {
        let raw = ((val as u16 as u32) | ((dep as u16 as u32) << 16)).to_le_bytes();
        c.extend_from_slice(&raw);
    };

    // 0x1000: vars[1] = 10 (lhs_mode=Var(2) -> 0x1800)
    push_hi(&mut code, 0x1800, 0, 1, 10);
    // 0x1000: vars[2] = 3
    push_hi(&mut code, 0x1800, 0, 2, 3);
    // 0xA000: regs[1] = vars[1] + vars[2] (13) -> lhs_mode=Var(2), rhs_mode=Var(2) => 0xAA00
    push_hi(&mut code, 0xAA00, 1, 1, 2);
    // 0xB000: regs[2] = vars[1] - vars[2] (7)
    push_hi(&mut code, 0xBA00, 2, 1, 2);
    // 0xC000: regs[3] = vars[1] * vars[2] (30)
    push_hi(&mut code, 0xCA00, 3, 1, 2);
    // 0xD000: regs[4] = vars[1] / vars[2] (3)
    push_hi(&mut code, 0xDA00, 4, 1, 2);
    // 0xE000: regs[5] = vars[1] % vars[2] (1)
    push_hi(&mut code, 0xEA00, 5, 1, 2);
    // 0x4000..=0x9000 comparisons: regs[6..11]
    push_hi(&mut code, 0x4A00, 6, 1, 1); // 10 == 10 -> 1
    push_hi(&mut code, 0x5A00, 7, 1, 2); // 10 >= 3 -> 1
    push_hi(&mut code, 0x6A00, 8, 2, 1); // 3 > 10 -> 0
    push_hi(&mut code, 0x7A00, 9, 2, 1); // 3 <= 10 -> 1
    push_hi(&mut code, 0x8A00, 10, 2, 1); // 3 < 10 -> 1
    push_hi(&mut code, 0x9A00, 11, 1, 2); // 10 != 3 -> 1
    // 0x2000 LogicalOr, 0x3000 LogicalAnd
    push_hi(&mut code, 0x2400, 12, 0, 5); // 0 || 5 -> 1 (lhs imm, rhs imm -> 0x2000; wait 0x2400 is lhs_mode=1)
    code.truncate(code.len() - 8);
    push_hi(&mut code, 0x2000, 12, 0, 5);
    push_hi(&mut code, 0x3000, 13, 1, 0);
    // 0xF000 LoadReg: regs[14] = vars[vars[2]] (where vars[3] = 99 via indirect assign)
    push_hi(&mut code, 0x1800, 0, 3, 99);
    // lhs_mode=Var(2), lhs_dep=0, rhs_mode=Var(2), rhs_dep=1 => 0xF000 | (2<<8) | 1 = 0xF201
    push_hi(&mut code, 0xF201, 14, 0, 2);

    // 0x12 load_array(var_idx=100, arr_idx=1)
    code.extend_from_slice(&0x0012u16.to_le_bytes());
    push_expr(&mut code, 100, 0);
    code.extend_from_slice(&1u32.to_le_bytes());

    // 0xCA memset_vars(200, 204, 77)
    code.extend_from_slice(&0x00CAu16.to_le_bytes());
    push_expr(&mut code, 200, 0);
    push_expr(&mut code, 204, 0);
    push_expr(&mut code, 77, 0);

    // 0x96 str_assign_lit(0, 1) -> "123"
    code.extend_from_slice(&0x0096u16.to_le_bytes());
    push_expr(&mut code, 0, 0);
    code.extend_from_slice(&1u32.to_le_bytes());
    // 0x99 str_to_int(out_var=210, str_var=0)
    code.extend_from_slice(&0x0099u16.to_le_bytes());
    push_expr(&mut code, 210, 0);
    push_expr(&mut code, 0, 0);
    // 0x9C richedit_open (safe no-op)
    code.extend_from_slice(&0x009Cu16.to_le_bytes());
    push_expr(&mut code, 0, 0);
    push_expr(&mut code, 0, 0);
    push_expr(&mut code, 0, 0);

    // 0xC8 gosub_local to sub_pc, then 0x08 exit
    let gosub_patch = code.len() + 2;
    code.extend_from_slice(&0x00C8u16.to_le_bytes());
    code.extend_from_slice(&0u32.to_le_bytes()); // target_pc patched below
    for a in 0..10i16 {
        push_expr(&mut code, a + 100, 0);
    }
    code.extend_from_slice(&0x0008u16.to_le_bytes()); // Exit

    let sub_pc = code.len() as u32;
    code[gosub_patch..gosub_patch + 4].copy_from_slice(&sub_pc.to_le_bytes());
    // Sub: 0x10 ret(vars[10] + 5 -> return vars[10] directly: 100)
    code.extend_from_slice(&0x0010u16.to_le_bytes());
    push_expr(&mut code, 10, 1); // vars[10] = 100

    let blob = build_synthetic_gsc(&code, &[b"123"], &[&[11, 22, 33]]);
    let gsc = GscScript::parse(1, &blob, false).unwrap();
    gsc.disassemble_all().unwrap();

    let mut host = DummyHost {
        scripts: [(1, gsc)].into_iter().collect(),
    };
    let mut vm = VmRuntime::new(TextEncoding::Cp932);
    vm.start_script(&mut host, 1, 0).unwrap();
    let y = vm.run_until_yield(&mut host, 1000).unwrap();
    assert_eq!(
        y,
        Some(VmYield::Halted {
            reason: HaltReason::ExitOpcode
        })
    );

    assert_eq!(vm.state.regs[1..=14], [13, 7, 30, 3, 1, 1, 1, 0, 1, 1, 1, 1, 0, 99]);
    assert_eq!(&vm.state.vars[100..103], &[11, 22, 33]);
    assert_eq!(&vm.state.vars[200..=204], &[77, 77, 77, 77, 77]);
    assert_eq!(vm.state.vars[210], 123);
    assert_eq!(vm.state.vars[0], 100); // ret_val from gosub_local
}

#[test]
fn real_opening_script_stepping_graphics_audio_rollback_and_slot_save_jp_and_chs() {
    if !require_real_assets_or_skip(
        "real_opening_script_stepping_graphics_audio_rollback_and_slot_save_jp_and_chs",
    ) {
        return;
    }
    let font_bytes = find_test_gothic_font();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    for (label, lang) in [
        ("jp", LanguageMode::Japanese),
        ("chs", LanguageMode::ChineseV1),
    ] {
        let save_dir = std::env::temp_dir().join(format!("fujihime_vm_step_{label}_{nonce}"));
        let mut cfg = EngineConfig::new(GAME_DIR, &save_dir, lang);
        cfg.repair_chs_recall_labels = lang == LanguageMode::ChineseV1;
        let mut eng = Engine::create(cfg).unwrap();
        eng.load_font_bytes(FontRole::Gothic, font_bytes.clone(), 0)
            .unwrap();

        // Start from 1102.gsc (opening prologue: bg_load 999, transition, se_load/play, text_voice)
        eng.start_game_script(1102).unwrap();
        // Step through transition (20 steps * 20 ms) + sleep (1500 ms + 2000 ms + 4000 ms)
        for _ in 0..450 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_click {
                break;
            }
        }
        assert!(
            eng.status().is_waiting_click,
            "{label}: 1102.gsc should reach first WaitClick dialogue"
        );
        // CG 999 unlocked in `bg_load(999)` -> vars[9999] == 1 & global_flags[0] bit 999
        assert_eq!(eng.vm.state.vars[9999], 1);
        assert!(eng.vm.state.global_flags.get_bit(0, 999));
        assert!(!eng.vm.state.tbox.backlog.is_empty());
        assert!(eng.vm.state.kidoku.get_bit(1102, 1));

        // Capture framebuffer and verify non-blank rendered pixels (BG + textbox + text)
        let mut bgra = vec![0u8; (GAME_WIDTH as usize) * (GAME_HEIGHT as usize) * 4];
        eng.copy_present_bgra8(&mut bgra).unwrap();
        assert!(bgra.iter().any(|&b| b != 0 && b != 0xFF));
        // Standard presentation BGRA8 has alpha == 0xFF
        assert_eq!(bgra[3], 0xFF);

        // Save slot 1 (75,026 B) right here at first dialogue line
        let pc_at_line1 = eng.vm.state.pc;
        eng.save_slot(1, 2026, 11, 20, 12, 34).unwrap();
        let slot_path = save_dir.join("KhDLsave01.dat");
        assert_eq!(fs::metadata(&slot_path).unwrap().len(), SLOT_SAVE_SIZE as u64);
        let parsed_slot = SlotSave2007::load_file(&slot_path).unwrap();
        assert_eq!(parsed_slot.to_bytes().len(), SLOT_SAVE_SIZE);

        // Advance past click & second sleep (5000ms) to second dialogue line
        eng.pointer_event(PointerKind::Up, 400, 300).unwrap(); // finish typewriter if still revealing
        if eng.status().is_waiting_click {
            eng.pointer_event(PointerKind::Up, 400, 300).unwrap();
        }
        for _ in 0..200 {
            eng.tick(50).unwrap();
            if eng.status().is_waiting_click {
                break;
            }
        }
        assert!(eng.status().is_waiting_click);
        assert!(eng.vm.state.pc > pc_at_line1);
        assert!(eng.vm.state.kidoku.get_bit(1102, 2));

        // Audio mixer pull produces finite PCM without errors
        let mut pcm = vec![0.0f32; 1024 * 2];
        assert_eq!(eng.render_audio_f32(&mut pcm), 1024);
        assert!(pcm.iter().all(|s| s.is_finite()));

        // Rollback one step back to line 1
        eng.trigger_rollback().unwrap();
        assert!(eng.vm.state.pc <= pc_at_line1);

        // Load slot 1 and verify exact PC restoration
        eng.load_slot(1).unwrap();
        assert_eq!(eng.vm.state.current_script_id(), 1102);
        assert_eq!(eng.vm.state.pc, pc_at_line1);

        let _ = fs::remove_dir_all(&save_dir);
    }
}
