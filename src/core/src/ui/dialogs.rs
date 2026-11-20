//! Exact bilingual (JP original & CHS V1.0) dialog strings for all
//! 10 `MessageBoxA` callsites and 7 `0x445e90` confirmation callsites.

use crate::engine::{ConfirmAction, DialogKind};
use crate::vfs::LanguageMode;

/// All 10 direct `MessageBoxA` callsites in `KhimeDL.exe` / `KhimeDL_CHS.exe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageBoxCallsite {
    /// `0x41d3e9`: `MB_OK` color depth warning (`!= 32bpp`).
    ColorMode32BitHint0x41d3e9,
    /// `0x41d730`: `MB_YESNO | MB_ICONQUESTION` generic confirm fallback.
    GenericYesNo0x41d730,
    /// `0x4214b2`: `MB_OK` `.gsc` header/compile error.
    CompileError0x4214b2,
    /// `0x421566`: `MB_OK` `call_script` stack overflow.
    ScriptNestOverflowCall0x421566,
    /// `0x42163f`: `MB_OK` `gosub_local` stack overflow.
    ScriptNestOverflowGosub0x42163f,
    /// `0x4216cf`: `MB_OK` `ret` stack underflow.
    ScriptNestUnderflow0x4216cf,
    /// `0x4218c3`: `MB_OK` layer index violation.
    LayerViolation0x4218c3,
    /// `0x422a1f`: `MB_OK` dialogue text format error.
    TextFormatErrorSay0x422a1f,
    /// `0x422baa`: `MB_OK` `select_6` choice text format error.
    TextFormatErrorSelect0x422baa,
    /// `0x445ebd`: `MB_YESNO | MB_ICONQUESTION` fallback inside `0x445e90` (`enddlg` absent).
    CustomConfirmFallback0x445ebd,
}

/// All 7 `0x445e90` confirmation dialog callsites in `KhimeDL.exe` / `KhimeDL_CHS.exe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmCallsite {
    /// `0x418e7a`: Config screen -> Return to Title.
    ConfigReturnToTitle0x418e7a,
    /// `0x418f98`: Config/Title Load/Save -> Overwrite existing save slot.
    ConfigSaveOverwrite0x418f98,
    /// `0x419051`: Config/Title Load -> Load existing save slot.
    ConfigLoadSlot0x419051,
    /// `0x41b448`: `compane`/MainGame `savescrn` -> Overwrite existing save slot.
    CompaneSaveOverwrite0x41b448,
    /// `0x41b501`: `compane`/MainGame `savescrn` -> Load existing save slot.
    CompaneLoadSlot0x41b501,
    /// `0x41b5ea`: `compane` -> Return to Title.
    CompaneReturnToTitle0x41b5ea,
    /// `0x44753a`: `WM_SYSCOMMAND SC_CLOSE` / Title Exit -> Quit Game.
    ExitGame0x44753a,
}

/// All 10 `MessageBoxA` callsites in order.
pub const ALL_MESSAGEBOX_CALLSITES: [MessageBoxCallsite; 10] = [
    MessageBoxCallsite::ColorMode32BitHint0x41d3e9,
    MessageBoxCallsite::GenericYesNo0x41d730,
    MessageBoxCallsite::CompileError0x4214b2,
    MessageBoxCallsite::ScriptNestOverflowCall0x421566,
    MessageBoxCallsite::ScriptNestOverflowGosub0x42163f,
    MessageBoxCallsite::ScriptNestUnderflow0x4216cf,
    MessageBoxCallsite::LayerViolation0x4218c3,
    MessageBoxCallsite::TextFormatErrorSay0x422a1f,
    MessageBoxCallsite::TextFormatErrorSelect0x422baa,
    MessageBoxCallsite::CustomConfirmFallback0x445ebd,
];

/// All 7 `0x445e90` callsites in order.
pub const ALL_CONFIRM_CALLSITES: [ConfirmCallsite; 7] = [
    ConfirmCallsite::ConfigReturnToTitle0x418e7a,
    ConfirmCallsite::ConfigSaveOverwrite0x418f98,
    ConfirmCallsite::ConfigLoadSlot0x419051,
    ConfirmCallsite::CompaneSaveOverwrite0x41b448,
    ConfirmCallsite::CompaneLoadSlot0x41b501,
    ConfirmCallsite::CompaneReturnToTitle0x41b5ea,
    ConfirmCallsite::ExitGame0x44753a,
];

/// Resolve exact `(DialogKind, title, message)` for one of the 10 `MessageBoxA` sites.
pub fn messagebox_text(
    site: MessageBoxCallsite,
    lang: LanguageMode,
    detail: Option<&str>,
) -> (DialogKind, String, String) {
    let is_chs = lang == LanguageMode::ChineseV1;
    let (kind, title, base) = match (site, is_chs) {
        (MessageBoxCallsite::ColorMode32BitHint0x41d3e9, false) => (
            DialogKind::OkAlert,
            "お知らせ",
            "このゲームは32Bitカラーモードで最適化されています。\nカラーモードを変更していただくとより一層お楽しみになれます。",
        ),
        (MessageBoxCallsite::ColorMode32BitHint0x41d3e9, true) => (
            DialogKind::OkAlert,
            "通知",
            "本游戏最适合32bit的显示模式。\n改变显示模式的话能获得更棒的显示效果。",
        ),
        (MessageBoxCallsite::GenericYesNo0x41d730, false) => {
            (DialogKind::YesNoConfirm, "確認", "")
        }
        (MessageBoxCallsite::GenericYesNo0x41d730, true) => {
            (DialogKind::YesNoConfirm, "确认", "")
        }
        (MessageBoxCallsite::CompileError0x4214b2, false) => {
            (DialogKind::OkAlert, "エラー", "コンパイルエラー")
        }
        (MessageBoxCallsite::CompileError0x4214b2, true) => {
            (DialogKind::OkAlert, "错误", "汇编错误")
        }
        (
            MessageBoxCallsite::ScriptNestOverflowCall0x421566
            | MessageBoxCallsite::ScriptNestOverflowGosub0x42163f,
            false,
        ) => (
            DialogKind::OkAlert,
            "エラー",
            "スクリプトネストオーバーフロー",
        ),
        (
            MessageBoxCallsite::ScriptNestOverflowCall0x421566
            | MessageBoxCallsite::ScriptNestOverflowGosub0x42163f,
            true,
        ) => (DialogKind::OkAlert, "错误", "脚本文字数过多"),
        (MessageBoxCallsite::ScriptNestUnderflow0x4216cf, false) => (
            DialogKind::OkAlert,
            "エラー",
            "スクリプトネストアンダーフロー",
        ),
        (MessageBoxCallsite::ScriptNestUnderflow0x4216cf, true) => {
            (DialogKind::OkAlert, "错误", "脚本文字数不足")
        }
        (MessageBoxCallsite::LayerViolation0x4218c3, false) => {
            (DialogKind::OkAlert, "エラー", "レイヤー指定違反")
        }
        (MessageBoxCallsite::LayerViolation0x4218c3, true) => {
            (DialogKind::OkAlert, "错误", "层指定违法")
        }
        (
            MessageBoxCallsite::TextFormatErrorSay0x422a1f
            | MessageBoxCallsite::TextFormatErrorSelect0x422baa,
            false,
        ) => (
            DialogKind::OkAlert,
            "エラー",
            "テキストフォーマットエラー",
        ),
        (
            MessageBoxCallsite::TextFormatErrorSay0x422a1f
            | MessageBoxCallsite::TextFormatErrorSelect0x422baa,
            true,
        ) => (DialogKind::OkAlert, "错误", "文本格式错误"),
        (MessageBoxCallsite::CustomConfirmFallback0x445ebd, false) => {
            (DialogKind::YesNoConfirm, "確認", "")
        }
        (MessageBoxCallsite::CustomConfirmFallback0x445ebd, true) => {
            (DialogKind::YesNoConfirm, "确认", "")
        }
    };
    let msg = match detail {
        Some(d) if !d.is_empty() && !base.is_empty() => format!("{base}\n{d}"),
        Some(d) if !d.is_empty() => d.to_string(),
        _ => base.to_string(),
    };
    (kind, title.to_string(), msg)
}

/// Resolve exact `(title, message)` for a `ConfirmAction` matching the 7 `0x445e90` sites.
pub fn confirm_action_text(action: ConfirmAction, lang: LanguageMode) -> (&'static str, &'static str) {
    let is_chs = lang == LanguageMode::ChineseV1;
    match (action, is_chs) {
        (ConfirmAction::ReturnToTitle, false) => (
            "確認",
            "タイトル画面に戻ります。\nよろしいですか？",
        ),
        (ConfirmAction::ReturnToTitle, true) => (
            "确认",
            "接下来要回到标题画面。\n这样可以吗？",
        ),
        (ConfirmAction::SaveOverwrite(_), false) => (
            "確認",
            "セーブデータを上書きします。\nよろしいですか？",
        ),
        (ConfirmAction::SaveOverwrite(_), true) => (
            "确认",
            "接下来要覆盖原存档。\n这样可以吗？",
        ),
        (ConfirmAction::LoadSlot(_), false) => (
            "確認",
            "セーブデータをロードします。\nよろしいですか？",
        ),
        (ConfirmAction::LoadSlot(_), true) => (
            "确认",
            "接下来要读取存档。\n这样可以吗？",
        ),
        (ConfirmAction::ExitGame, false) => (
            "終了確認",
            "本当にゲームを終了しますか？",
        ),
        (ConfirmAction::ExitGame, true) => (
            "确认结束",
            "真的要结束游戏吗？",
        ),
        (ConfirmAction::AcknowledgeOnly, false) => ("お知らせ", ""),
        (ConfirmAction::AcknowledgeOnly, true) => ("通知", ""),
    }
}

/// Resolve exact `(ConfirmAction, title, message)` for a specific `0x445e90` callsite.
pub fn confirm_callsite_text(
    site: ConfirmCallsite,
    slot_num: u16,
    lang: LanguageMode,
) -> (ConfirmAction, &'static str, &'static str) {
    let action = match site {
        ConfirmCallsite::ConfigReturnToTitle0x418e7a
        | ConfirmCallsite::CompaneReturnToTitle0x41b5ea => ConfirmAction::ReturnToTitle,
        ConfirmCallsite::ConfigSaveOverwrite0x418f98
        | ConfirmCallsite::CompaneSaveOverwrite0x41b448 => ConfirmAction::SaveOverwrite(slot_num),
        ConfirmCallsite::ConfigLoadSlot0x419051 | ConfirmCallsite::CompaneLoadSlot0x41b501 => {
            ConfirmAction::LoadSlot(slot_num)
        }
        ConfirmCallsite::ExitGame0x44753a => ConfirmAction::ExitGame,
    };
    let (title, msg) = confirm_action_text(action, lang);
    (action, title, msg)
}
