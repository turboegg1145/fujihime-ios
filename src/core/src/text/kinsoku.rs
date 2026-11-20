//! Kinsoku (line-wrapping punctuation禁则) rules (`KhimeDL.exe` `0x441bf0`..`0x441e98`).
//!
//! Disassembly of `0x441bf0..0x441e98` confirms:
//! - `LineStartForbidden` (`type == 1`): 10 ASCII + 38 Shift-JIS characters (48 items total)
//! - `LineEndForbidden` (`type == 2`): 12 Shift-JIS characters
//!
//! For `ChineseV1` mode, GBK/Unicode punctuation equivalents are also mapped cleanly.

/// Kinsoku classification stored in `*(u8*)(this+0x180)` per character slot (`0x441bf0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KinsokuClass {
    /// Normal character (`0`).
    #[default]
    None = 0,
    /// Cannot begin a new line (`1`); allows at most 1 hanging overhang char on current line.
    LineStartForbidden = 1,
    /// Cannot end a line (`2`); pushed together with the following character onto the next line.
    LineEndForbidden = 2,
}

/// Exact 10 ASCII line-start-forbidden codes from `0x441bf0..0x441c4e`.
pub const KINSOKU_START_ASCII: [u16; 10] = [
    0x0020, // ' '
    0x0027, // '\''
    0x0022, // '"'
    0x002C, // ','
    0x002E, // '.'
    0x003F, // '?'
    0x0021, // '!'
    0x0029, // ')'
    0x007D, // '}'
    0x005D, // ']'
];

/// Exact 38 Shift-JIS (`(b0 << 8) | b1`) line-start-forbidden codes from `0x441c54..0x441df0`.
pub const KINSOKU_START_SJIS: [u16; 38] = [
    0x8140, // 　
    0x8141, // 、
    0x8142, // 。
    0x8143, // ，
    0x8144, // ．
    0x8148, // ？
    0x8149, // ！
    0x815B, // ー
    0x8160, // ～
    0x8163, // …
    0x816A, // ）
    0x816C, // 〕
    0x816E, // ］
    0x8170, // ｝
    0x8172, // 〉
    0x8174, // 》
    0x8176, // 」
    0x8178, // 』
    0x817A, // 】
    0x8166, // ’
    0x8168, // ”
    0x82C1, // っ
    0x829F, // ぁ
    0x82A1, // ぃ
    0x82A3, // ぅ
    0x82A5, // ぇ
    0x82A7, // ぉ
    0x82E1, // ゃ
    0x82E3, // ゅ
    0x82E5, // ょ
    0x8340, // ァ
    0x8342, // ィ
    0x8344, // ゥ
    0x8346, // ェ
    0x8348, // ォ
    0x8383, // ャ
    0x8385, // ュ
    0x8387, // ョ
];

/// Exact 12 Shift-JIS (`(b0 << 8) | b1`) line-end-forbidden codes from `0x441df6..0x441e48`.
pub const KINSOKU_END_SJIS: [u16; 12] = [
    0x8165, // ‘
    0x8167, // “
    0x8169, // （
    0x816B, // 〔
    0x816D, // ［
    0x816F, // ｛
    0x8171, // 〈
    0x8173, // 《
    0x8175, // 「
    0x8177, // 『
    0x8179, // 【
    0x814A, // ゛
];

/// Classify a raw CP932 character code (`0x00xx` for 1-byte ASCII, `0xAABB` for 2-byte SJIS)
/// identically to `0x441bf0..0x441e98`.
#[inline]
pub fn classify_cp932_kinsoku(raw_code: u16) -> KinsokuClass {
    if KINSOKU_START_ASCII.contains(&raw_code) || KINSOKU_START_SJIS.contains(&raw_code) {
        KinsokuClass::LineStartForbidden
    } else if KINSOKU_END_SJIS.contains(&raw_code) {
        KinsokuClass::LineEndForbidden
    } else {
        KinsokuClass::None
    }
}

/// Classify a Unicode `char` for `ChineseV1` GBK / UTF-8 text (preserving all CP932 punctuation
/// semantics plus Chinese punctuation like `：`, `；`, `—`, `、`, `。`, `，`, `？`, `！`).
#[inline]
pub fn classify_unicode_kinsoku(ch: char) -> KinsokuClass {
    match ch {
        // Line-start forbidden (matches 48 JP items + common CJK equivalents)
        ' ' | '\'' | '"' | ',' | '.' | '?' | '!' | ')' | '}' | ']' | '\u{3000}' | '、' | '。'
        | '，' | '．' | '？' | '！' | 'ー' | '～' | '…' | '）' | '〕' | '］' | '｝' | '〉' | '》'
        | '」' | '』' | '】' | '’' | '”' | 'っ' | 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ'
        | 'ょ' | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ャ' | 'ュ' | 'ョ' | '：' | '；' | '—' => {
            KinsokuClass::LineStartForbidden
        }
        // Line-end forbidden (matches 12 JP items)
        '‘' | '“' | '（' | '〔' | '［' | '｛' | '〈' | '《' | '「' | '『' | '【' | '゛' => {
            KinsokuClass::LineEndForbidden
        }
        _ => KinsokuClass::None,
    }
}
