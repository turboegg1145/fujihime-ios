//! Local HTTP/WebCanvas player for 《腐姫～euthanasia～》 (`fujihime-core`).
//!
//! Uses only `std` + `fujihime_core` (zero external crates) so it builds completely offline
//! inside `fujihime-re` and runs identically to the iOS Metal/AVAudioEngine frontend:
//! - Framebuffer conversion matches `src/ios/Sources/Shaders.metal`:
//!   `BGRA8 -> RGBA8` with alpha forced to `255` (`1.0`).
//! - Audio renders `44,100 Hz` interleaved stereo `f32` PCM from `Engine::render_audio_f32`.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fujihime_core::engine::{
    Engine, EngineConfig, FontRole, PointerKind, TopScene,
};
use fujihime_core::gfx::surface::{GAME_HEIGHT, GAME_WIDTH};
use fujihime_core::ui::SaveScrnMode;
use fujihime_core::vfs::LanguageMode;

struct PlayerState {
    engine: Engine,
    bgra_scratch: Vec<u8>,
    rgba_frame: Vec<u8>,
    frame_seq: u64,
    paused: bool,
    last_fps_instant: Instant,
    ticks_in_window: u32,
    measured_fps: f32,
    game_dir: PathBuf,
    orig_backup_dir: Option<PathBuf>,
}

impl PlayerState {
    fn refresh_rgba_from_engine(&mut self) {
        if self.engine.copy_present_bgra8(&mut self.bgra_scratch).is_ok() {
            let mut changed = false;
            for (src, dst) in self
                .bgra_scratch
                .as_chunks::<4>()
                .0
                .iter()
                .zip(self.rgba_frame.as_chunks_mut::<4>().0.iter_mut())
            {
                // Matches `Shaders.metal`: swap BGRA -> RGBA and force alpha = 255 (`1.0h`).
                let r = src[2];
                let g = src[1];
                let b = src[0];
                if dst[0] != r || dst[1] != g || dst[2] != b || dst[3] != 255 {
                    dst[0] = r;
                    dst[1] = g;
                    dst[2] = b;
                    dst[3] = 255;
                    changed = true;
                }
            }
            if changed || self.frame_seq == 0 {
                self.frame_seq = self.frame_seq.wrapping_add(1).max(1);
            }
        }
    }

    fn reload_fonts(&mut self, lang: LanguageMode, font_dirs: &[PathBuf]) {
        let gothic_names = match lang {
            LanguageMode::Japanese => &[
                "jp_gothic.ttf",
                "ipag.ttf",
                "NotoSansCJK-Regular.ttc",
                "wqy-zenhei.ttc",
            ][..],
            LanguageMode::ChineseV1 => &[
                "chs_hei.ttc",
                "chs_hei.ttf",
                "wqy-zenhei.ttc",
                "ipag.ttf",
            ][..],
        };
        let mincho_names = &["jp_mincho.ttf", "ipam.ttf", "ipag.ttf"][..];
        if let Some(bytes) = find_first_font(font_dirs, gothic_names) {
            let _ = self.engine.load_font_bytes(FontRole::Gothic, bytes, 0);
        }
        if let Some(bytes) = find_first_font(font_dirs, mincho_names) {
            let _ = self.engine.load_font_bytes(FontRole::Mincho, bytes, 0);
        }
    }
}

fn find_first_font(dirs: &[PathBuf], names: &[&str]) -> Option<Vec<u8>> {
    for dir in dirs {
        for name in names {
            let p = dir.join(name);
            if p.is_file() {
                if let Ok(b) = fs::read(&p) {
                    if !b.is_empty() {
                        return Some(b);
                    }
                }
            }
        }
    }
    None
}

fn resolve_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(custom) = env::var("FUJIHIME_FONT_DIR") {
        dirs.push(PathBuf::from(custom));
    }
    for cand in [
        "re/.cache/fonts",
        "../../re/.cache/fonts",
        "/workspace/re/.cache/fonts",
        "/tmp/test_fonts/usr/share/fonts/opentype/ipafont-gothic",
        "/tmp/test_fonts/usr/share/fonts/opentype/ipafont-mincho",
        "/tmp/test_fonts/usr/share/fonts/truetype/wqy",
        "/usr/share/fonts/opentype/ipafont-gothic",
        "/usr/share/fonts/opentype/ipafont-mincho",
        "/usr/share/fonts/truetype/wqy",
    ] {
        let p = PathBuf::from(cand);
        if p.is_dir() {
            dirs.push(p);
        }
    }
    dirs
}

fn resolve_game_dir() -> PathBuf {
    if let Ok(custom) = env::var("FUJIHIME_GAME_DIR") {
        return PathBuf::from(custom);
    }
    for cand in [
        "game/腐姫ＤＬ",
        "../../game/腐姫ＤＬ",
        "/workspace/game/腐姫ＤＬ",
    ] {
        let p = PathBuf::from(cand);
        if p.is_dir() {
            return p;
        }
    }
    PathBuf::from("/workspace/game/腐姫ＤＬ")
}

fn resolve_save_dir() -> PathBuf {
    if let Ok(custom) = env::var("FUJIHIME_SAVE_DIR") {
        return PathBuf::from(custom);
    }
    for base in ["re/.cache", "../../re/.cache", "/workspace/re/.cache"] {
        let p = PathBuf::from(base);
        if p.is_dir() || Path::new(base).parent().is_some_and(|par| par.is_dir()) {
            return p.join("web_save");
        }
    }
    PathBuf::from("/tmp/fujihime_web_save")
}

/// Pure-Rust lossless RGBA8 -> PNG encoder (uncompressed DEFLATE stored blocks + CRC32/Adler32).
fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut crc_table = [0u32; 256];
    for (i, slot) in crc_table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if (c & 1) != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let crc32 = |data: &[u8]| -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c = crc_table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
        }
        !c
    };
    let write_chunk = |out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]| {
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(tag);
        out.extend_from_slice(payload);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    };

    let row_bytes = (width as usize) * 4;
    let mut raw = Vec::with_capacity((row_bytes + 1) * (height as usize));
    for y in 0..(height as usize) {
        raw.push(0); // Filter type 0 (None)
        let off = y * row_bytes;
        raw.extend_from_slice(&rgba[off..off + row_bytes]);
    }

    // Adler-32 checksum of raw scanline stream
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in &raw {
        s1 = (s1 + u32::from(b)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    let adler = (s2 << 16) | s1;

    // Zlib stream using uncompressed stored DEFLATE blocks (max 65535 bytes per block)
    let mut zlib = Vec::with_capacity(raw.len() + 64);
    zlib.push(0x78);
    zlib.push(0x01);
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (idx, ch) in chunks.iter().enumerate() {
        let is_last = idx + 1 == chunks.len();
        zlib.push(if is_last { 0x01 } else { 0x00 });
        let len = ch.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(ch);
    }
    zlib.extend_from_slice(&adler.to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // RGBA
    ihdr[10] = 0;
    ihdr[11] = 0;
    ihdr[12] = 0;
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib);
    write_chunk(&mut png, b"IEND", &[]);
    png
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(k.to_string(), v.to_string());
    }
    map
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    if !body.is_empty() {
        let _ = stream.write_all(body);
    }
}

fn handle_client(
    mut stream: TcpStream,
    state: Arc<Mutex<PlayerState>>,
    font_dirs: Arc<Vec<PathBuf>>,
) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let full_target = parts[1];
    let (path, query_str) = full_target.split_once('?').unwrap_or((full_target, ""));

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("Content-Length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body_buf = vec![0u8; content_length.min(1024 * 64)];
    if !body_buf.is_empty() {
        let _ = reader.read_exact(&mut body_buf);
    }
    let mut params = parse_query(query_str);
    if method == "POST" && !body_buf.is_empty() {
        if let Ok(bstr) = std::str::from_utf8(&body_buf) {
            for (k, v) in parse_query(bstr) {
                params.insert(k, v);
            }
        }
    }

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            write_response(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                &[],
                INDEX_HTML.as_bytes(),
            );
        }
        ("GET", "/frame") => {
            let since: u64 = params
                .get("since")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let st = state.lock().unwrap();
            if since != 0 && since == st.frame_seq {
                let seq = st.frame_seq.to_string();
                write_response(
                    &mut stream,
                    "204 No Content",
                    "application/octet-stream",
                    &[("X-Frame-Seq", &seq)],
                    &[],
                );
            } else {
                let seq = st.frame_seq.to_string();
                let bytes = st.rgba_frame.clone();
                drop(st);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/octet-stream",
                    &[("X-Frame-Seq", &seq)],
                    &bytes,
                );
            }
        }
        ("GET", "/frame.png") => {
            let rgba = {
                let st = state.lock().unwrap();
                st.rgba_frame.clone()
            };
            let png = encode_png_rgba(GAME_WIDTH, GAME_HEIGHT, &rgba);
            write_response(
                &mut stream,
                "200 OK",
                "image/png",
                &[("Content-Disposition", "inline; filename=\"fujihime.png\"")],
                &png,
            );
        }
        ("GET", "/audio") => {
            let frames: usize = params
                .get("frames")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2048)
                .clamp(256, 8192);
            let mut pcm = vec![0.0f32; frames * 2];
            {
                let st = state.lock().unwrap();
                st.engine.render_audio_f32(&mut pcm);
            }
            let mut raw = Vec::with_capacity(pcm.len() * 4);
            for s in pcm {
                raw.extend_from_slice(&s.to_le_bytes());
            }
            write_response(&mut stream, "200 OK", "application/octet-stream", &[], &raw);
        }
        ("GET", "/state") => {
            let mut st = state.lock().unwrap();
            let status = st.engine.status();
            let dlg = st.engine.poll_pending_dialog();
            let non_black = st
                .rgba_frame
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
                .count();
            let total_px = (GAME_WIDTH * GAME_HEIGHT) as usize;
            let dlg_json = if let Some(d) = dlg {
                format!(
                    "{{\"id\":{},\"kind\":{},\"title\":\"{}\",\"message\":\"{}\"}}",
                    d.id,
                    d.kind as u32,
                    json_escape(&d.title),
                    json_escape(&d.message),
                )
            } else {
                "null".to_string()
            };
            let json = format!(
                "{{\"scene\":\"{:?}\",\"lang\":\"{:?}\",\"script_id\":{},\"pc\":{},\"waiting_click\":{},\"waiting_select\":{},\"transitioning\":{},\"playing_video\":{},\"paused\":{},\"frame_seq\":{},\"fps\":{:.1},\"non_black_px\":{},\"total_px\":{},\"textbox_visible\":{},\"backlog_len\":{},\"dialog\":{}}}",
                status.scene,
                status.language_mode,
                status.current_script_id,
                status.current_pc,
                status.is_waiting_click,
                status.is_waiting_select,
                status.is_transitioning,
                status.is_playing_video,
                st.paused,
                st.frame_seq,
                st.measured_fps,
                non_black,
                total_px,
                st.engine.vm.state.tbox.visible,
                st.engine.vm.state.tbox.backlog.len(),
                dlg_json,
            );
            write_response(&mut stream, "200 OK", "application/json", &[], json.as_bytes());
        }
        ("POST", "/input") => {
            let kind_type = params.get("type").map(String::as_str).unwrap_or("");
            let mut st = state.lock().unwrap();
            let res = match kind_type {
                "pointer" => {
                    let k = params.get("kind").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                    let x = params.get("x").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
                    let y = params.get("y").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
                    let pk = match k {
                        0 => PointerKind::Move,
                        1 => PointerKind::Down,
                        2 => PointerKind::Up,
                        3 => PointerKind::RightUp,
                        4 => PointerKind::MiddleUp,
                        5 => PointerKind::WheelUp,
                        _ => PointerKind::WheelDown,
                    };
                    st.engine.pointer_event(pk, x, y)
                }
                "key" => {
                    let vk = params.get("vk").and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
                    let down = params.get("down").map(|v| v == "1" || v == "true").unwrap_or(true);
                    st.engine.key_event(vk, down)
                }
                _ => Ok(()),
            };
            st.refresh_rgba_from_engine();
            let msg = match res {
                Ok(()) => "{\"ok\":true}".to_string(),
                Err(e) => format!("{{\"ok\":false,\"error\":\"{}\"}}", json_escape(&e.to_string())),
            };
            write_response(&mut stream, "200 OK", "application/json", &[], msg.as_bytes());
        }
        ("POST", "/action") => {
            let cmd = params.get("cmd").map(String::as_str).unwrap_or("");
            let val = params.get("val").map(String::as_str).unwrap_or("");
            let mut st = state.lock().unwrap();
            let res: fujihime_core::Result<()> = match cmd {
                "lang" => {
                    let lang = if val.eq_ignore_ascii_case("chs") {
                        LanguageMode::ChineseV1
                    } else {
                        LanguageMode::Japanese
                    };
                    let repair = lang == LanguageMode::ChineseV1;
                    let bak = st.orig_backup_dir.clone();
                    let r = st.engine.set_language_mode(lang, bak, repair);
                    st.reload_fonts(lang, &font_dirs);
                    let _ = st.engine.refresh_display();
                    r
                }
                "scene" => match val {
                    "op" => st.engine.start_title_sequence(),
                    "title" => st.engine.enter_title_screen(),
                    "load" => st.engine.open_save_overlay(SaveScrnMode::Load),
                    "save" => st.engine.open_save_overlay(SaveScrnMode::Save),
                    "config" => st.engine.open_config_overlay(),
                    "extra" => st.engine.enter_scene(TopScene::ExtraMenu),
                    "cg" => st.engine.enter_scene(TopScene::CgGallery),
                    "replay" => st.engine.enter_scene(TopScene::SceneReplay),
                    "music" => st.engine.enter_scene(TopScene::MusicMode),
                    _ => Ok(()),
                },
                "script" => {
                    let sid: u16 = val.parse().unwrap_or(111);
                    st.engine.start_game_script(sid)
                }
                "pause" => {
                    st.paused = !st.paused;
                    Ok(())
                }
                "step" => {
                    let ms: u32 = val.parse().unwrap_or(50);
                    st.engine.tick(ms)
                }
                "click_advance" => st.engine.advance_dialogue_click(),
                "skip" => {
                    st.engine.vm.state.fast_skip_active = !st.engine.vm.state.fast_skip_active;
                    Ok(())
                }
                "save_slot1" => st.engine.save_slot(1, 2026, 11, 20, 12, 0),
                "load_slot1" => st.engine.load_slot(1),
                "fullcg" => {
                    let path = st.game_dir.join("补丁").join("全CG档").join("KhDLsave.dat");
                    if let Ok(bytes) = fs::read(&path) {
                        st.engine.import_2002_cd_save(&bytes)
                    } else {
                        Ok(())
                    }
                }
                "dialog" => {
                    let id = params.get("id").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                    let accept = params.get("accept").map(|v| v == "1" || v == "true").unwrap_or(false);
                    st.engine.confirm_dialog_response(id, accept)
                }
                _ => Ok(()),
            };
            st.refresh_rgba_from_engine();
            let msg = match res {
                Ok(()) => "{\"ok\":true}".to_string(),
                Err(e) => format!("{{\"ok\":false,\"error\":\"{}\"}}", json_escape(&e.to_string())),
            };
            write_response(&mut stream, "200 OK", "application/json", &[], msg.as_bytes());
        }
        _ => {
            write_response(&mut stream, "404 Not Found", "text/plain", &[], b"Not Found");
        }
    }
}

fn main() {
    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);
    let game_dir = resolve_game_dir();
    let save_dir = resolve_save_dir();
    let _ = fs::create_dir_all(&save_dir);
    let font_dirs = Arc::new(resolve_font_dirs());
    let lang = if env::var("FUJIHIME_LANG")
        .ok()
        .is_some_and(|v| v.eq_ignore_ascii_case("chs"))
    {
        LanguageMode::ChineseV1
    } else {
        LanguageMode::Japanese
    };
    let orig_backup = game_dir.join("补丁").join("原版备份");
    let orig_backup_dir = if orig_backup.is_dir() {
        Some(orig_backup)
    } else {
        None
    };

    let mut cfg = EngineConfig::new(&game_dir, &save_dir, lang);
    cfg.orig_backup_dir = orig_backup_dir.clone();
    cfg.repair_chs_recall_labels = lang == LanguageMode::ChineseV1;

    let engine = Engine::create(cfg).expect("Failed to initialize fujihime-core Engine");
    let frame_len = (GAME_WIDTH as usize) * (GAME_HEIGHT as usize) * 4;
    let mut player = PlayerState {
        engine,
        bgra_scratch: vec![0u8; frame_len],
        rgba_frame: vec![0u8; frame_len],
        frame_seq: 0,
        paused: false,
        last_fps_instant: Instant::now(),
        ticks_in_window: 0,
        measured_fps: 0.0,
        game_dir: game_dir.clone(),
        orig_backup_dir,
    };
    player.reload_fonts(lang, &font_dirs);
    if env::var("FUJIHIME_SKIP_OP").ok().as_deref() == Some("1") {
        player.engine.enter_title_screen().expect("enter_title_screen failed");
    } else {
        player.engine.start_title_sequence().expect("start_title_sequence failed");
    }
    player.refresh_rgba_from_engine();

    let state = Arc::new(Mutex::new(player));
    let tick_state = Arc::clone(&state);
    thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            thread::sleep(Duration::from_millis(16));
            let now = Instant::now();
            let dt = now.duration_since(last).as_millis().clamp(1, 50) as u32;
            last = now;
            if let Ok(mut st) = tick_state.lock() {
                if !st.paused {
                    let _ = st.engine.tick(dt);
                    st.refresh_rgba_from_engine();
                    st.ticks_in_window += 1;
                    let elapsed = now.duration_since(st.last_fps_instant).as_secs_f32();
                    if elapsed >= 0.5 {
                        st.measured_fps = (st.ticks_in_window as f32) / elapsed;
                        st.ticks_in_window = 0;
                        st.last_fps_instant = now;
                    }
                }
            }
        }
    });

    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).expect("Failed to bind web_player port");
    eprintln!("[fujihime-web-player] listening on http://127.0.0.1:{port} (game_dir={})", game_dir.display());
    for stream in listener.incoming().flatten() {
        let st = Arc::clone(&state);
        let fdirs = Arc::clone(&font_dirs);
        thread::spawn(move || handle_client(stream, st, fdirs));
    }
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<title>腐姫～euthanasia～ Local Web Player</title>
<style>
  :root { color-scheme: dark; }
  body {
    margin: 0; padding: 12px; background: #0e1014; color: #e6e8ec;
    font-family: ui-sans-serif, system-ui, -apple-system, sans-serif;
    display: flex; flex-direction: column; align-items: center; gap: 10px;
  }
  .toolbar, .status-bar {
    width: 800px; max-width: 100%; display: flex; flex-wrap: wrap; gap: 6px;
    align-items: center; background: #181c23; border: 1px solid #2b3240;
    border-radius: 6px; padding: 8px 10px; box-sizing: border-box; font-size: 12px;
  }
  button {
    background: #252c38; color: #e6e8ec; border: 1px solid #394457;
    border-radius: 4px; padding: 4px 8px; cursor: pointer; font-size: 12px;
  }
  button:hover { background: #313b4b; }
  button.active { background: #46608a; border-color: #6b8fc9; }
  #stage {
    position: relative; width: 800px; height: 600px; background: #000;
    border: 1px solid #2b3240; user-select: none;
  }
  canvas { display: block; width: 800px; height: 600px; cursor: default; }
  #dialogOverlay {
    display: none; position: fixed; inset: 0; background: rgba(0,0,0,0.65);
    align-items: center; justify-content: center; z-index: 99;
  }
  #dialogBox {
    background: #1b2029; border: 1px solid #4c5a73; border-radius: 8px;
    padding: 16px; min-width: 300px; max-width: 440px;
  }
  #dialogBox h3 { margin: 0 0 8px 0; font-size: 15px; }
  #dialogBox p { margin: 0 0 14px 0; white-space: pre-wrap; font-size: 13px; }
  .dlg-btns { display: flex; justify-content: flex-end; gap: 8px; }
  .mono { font-family: ui-monospace, monospace; color: #9db4d8; }
</style>
</head>
<body>
  <div class="toolbar">
    <strong>腐姫 Web Player</strong>
    <button onclick="act('lang','jp')" id="btnJp">JP 原版</button>
    <button onclick="act('lang','chs')" id="btnChs">CHS V1.0</button>
    <span>|</span>
    <button onclick="act('scene','op')">Start OP</button>
    <button onclick="act('scene','title')">Title</button>
    <button onclick="act('script','1102')">1102 Prologue</button>
    <button onclick="act('scene','load')">Load</button>
    <button onclick="act('scene','config')">Config</button>
    <button onclick="act('scene','cg')">CG Gallery</button>
    <button onclick="act('scene','replay')">Replay</button>
    <span>|</span>
    <button onclick="act('fullcg','1')">Import Full-CG</button>
    <button onclick="act('save_slot1','1')">Save #1</button>
    <button onclick="act('load_slot1','1')">Load #1</button>
    <span>|</span>
    <button onclick="act('pause','1')" id="btnPause">Pause</button>
    <button onclick="act('step','50')">Step 50ms</button>
    <button onclick="act('click_advance','1')">Advance</button>
    <button onclick="toggleAudio()" id="btnAudio">Audio: Off</button>
    <button onclick="capturePng()">Capture PNG</button>
  </div>

  <div id="stage">
    <canvas id="game" width="800" height="600" tabindex="0"></canvas>
  </div>

  <div class="status-bar">
    <span id="statusText" class="mono">loading...</span>
  </div>

  <div id="dialogOverlay">
    <div id="dialogBox">
      <h3 id="dlgTitle"></h3>
      <p id="dlgMsg"></p>
      <div class="dlg-btns">
        <button id="dlgCancel" onclick="replyDlg(false)">No / Cancel</button>
        <button id="dlgOk" onclick="replyDlg(true)">Yes / OK</button>
      </div>
    </div>
  </div>

<script>
const canvas = document.getElementById('game');
const ctx = canvas.getContext('2d');
const imgData = ctx.createImageData(800, 600);
let frameSeq = 0;
let currentDialog = null;
let audioCtx = null;
let audioEnabled = false;
let audioPending = false;

async function fetchFrame() {
  try {
    const res = await fetch('/frame?since=' + frameSeq);
    if (res.status === 200) {
      const seq = Number(res.headers.get('X-Frame-Seq') || 0);
      const buf = await res.arrayBuffer();
      if (buf.byteLength === 800 * 600 * 4) {
        imgData.data.set(new Uint8ClampedArray(buf));
        ctx.putImageData(imgData, 0, 0);
        frameSeq = seq;
      }
    }
  } catch (_) {}
  requestAnimationFrame(fetchFrame);
}

async function pollState() {
  try {
    const res = await fetch('/state');
    const st = await res.json();
    const pct = ((st.non_black_px * 100) / st.total_px).toFixed(1);
    document.getElementById('statusText').textContent =
      `Scene=${st.scene} | Lang=${st.lang} | Script=${String(st.script_id).padStart(4,'0')}.gsc @ 0x${st.pc.toString(16)} | WaitClick=${st.waiting_click} | TBox=${st.textbox_visible} | Backlog=${st.backlog_len} | FPS=${st.fps} | NonBlack=${pct}%`;
    document.getElementById('btnJp').classList.toggle('active', st.lang === 'Japanese');
    document.getElementById('btnChs').classList.toggle('active', st.lang === 'ChineseV1');
    document.getElementById('btnPause').textContent = st.paused ? 'Resume' : 'Pause';
    if (st.dialog && (!currentDialog || currentDialog.id !== st.dialog.id)) {
      currentDialog = st.dialog;
      document.getElementById('dlgTitle').textContent = st.dialog.title;
      document.getElementById('dlgMsg').textContent = st.dialog.message;
      document.getElementById('dlgCancel').style.display = st.dialog.kind === 1 ? 'inline-block' : 'none';
      document.getElementById('dialogOverlay').style.display = 'flex';
    } else if (!st.dialog && currentDialog) {
      currentDialog = null;
      document.getElementById('dialogOverlay').style.display = 'none';
    }
  } catch (_) {}
}

function gameCoords(ev) {
  const r = canvas.getBoundingClientRect();
  const x = Math.max(0, Math.min(799, Math.floor((ev.clientX - r.left) * 800 / r.width)));
  const y = Math.max(0, Math.min(599, Math.floor((ev.clientY - r.top) * 600 / r.height)));
  return [x, y];
}

function sendPointer(kind, x, y) {
  fetch(`/input?type=pointer&kind=${kind}&x=${x}&y=${y}`, { method: 'POST' });
}

canvas.addEventListener('mousemove', e => { const [x,y] = gameCoords(e); sendPointer(0, x, y); });
canvas.addEventListener('mousedown', e => {
  if (e.button === 0) { const [x,y] = gameCoords(e); sendPointer(1, x, y); }
});
canvas.addEventListener('mouseup', e => {
  const [x,y] = gameCoords(e);
  if (e.button === 0) sendPointer(2, x, y);
  else if (e.button === 1) sendPointer(4, x, y);
  else if (e.button === 2) sendPointer(3, x, y);
});
canvas.addEventListener('contextmenu', e => e.preventDefault());
canvas.addEventListener('wheel', e => {
  e.preventDefault();
  const [x,y] = gameCoords(e);
  sendPointer(e.deltaY < 0 ? 5 : 6, x, y);
}, { passive: false });

function mapKeyToVk(e) {
  if (e.key === 'Control') return 0x11;
  if (e.key === 'Tab') return 0x09;
  if (e.key === 'Shift') return 0x10;
  if (e.key === 'Escape') return 0x1B;
  if (e.key === 'Enter') return 0x0D;
  if (e.key === ' ') return 0x20;
  if (e.key === 'ArrowUp') return 0x26;
  if (e.key === 'ArrowDown') return 0x28;
  if (e.key === 'F1') return 0x70;
  return 0;
}
window.addEventListener('keydown', e => {
  const vk = mapKeyToVk(e);
  if (vk) { e.preventDefault(); fetch(`/input?type=key&vk=${vk}&down=1`, { method: 'POST' }); }
});
window.addEventListener('keyup', e => {
  const vk = mapKeyToVk(e);
  if (vk) { e.preventDefault(); fetch(`/input?type=key&vk=${vk}&down=0`, { method: 'POST' }); }
});

function act(cmd, val) {
  fetch(`/action?cmd=${encodeURIComponent(cmd)}&val=${encodeURIComponent(val)}`, { method: 'POST' });
}
function replyDlg(accept) {
  if (!currentDialog) return;
  fetch(`/action?cmd=dialog&id=${currentDialog.id}&accept=${accept?1:0}`, { method: 'POST' });
}
function capturePng() {
  const a = document.createElement('a');
  a.href = '/frame.png?ts=' + Date.now();
  a.download = 'fujihime_frame.png';
  a.click();
}
function toggleAudio() {
  audioEnabled = !audioEnabled;
  document.getElementById('btnAudio').textContent = audioEnabled ? 'Audio: On' : 'Audio: Off';
  if (audioEnabled && !audioCtx) {
    audioCtx = new (window.AudioContext || window.webkitAudioContext)({ sampleRate: 44100 });
    const node = audioCtx.createScriptProcessor(2048, 0, 2);
    let qL = [], qR = [];
    node.onaudioprocess = ev => {
      const outL = ev.outputBuffer.getChannelData(0);
      const outR = ev.outputBuffer.getChannelData(1);
      for (let i = 0; i < outL.length; i++) {
        outL[i] = qL.length ? qL.shift() : 0;
        outR[i] = qR.length ? qR.shift() : 0;
      }
      if (audioEnabled && !audioPending && qL.length < 4096) {
        audioPending = true;
        fetch('/audio?frames=2048').then(r => r.arrayBuffer()).then(buf => {
          const f = new Float32Array(buf);
          for (let i = 0; i + 1 < f.length; i += 2) { qL.push(f[i]); qR.push(f[i+1]); }
          audioPending = false;
        }).catch(() => { audioPending = false; });
      }
    };
    node.connect(audioCtx.destination);
  }
  if (audioCtx && audioCtx.state === 'suspended') audioCtx.resume();
}

requestAnimationFrame(fetchFrame);
setInterval(pollState, 250);
pollState();
</script>
</body>
</html>
"#;
