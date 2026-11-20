//! Complete `.gsc` bytecode instruction decoder (`KhimeDL.exe` `0x41f560..0x41f9f0`).
//!
//! Covers:
//! - All 15 high-family expression & assignment opcodes (`0x1000..=0xF000`)
//! - All 120 command opcodes (`0x03..=0xD5`)

use crate::error::{FujihimeError, Result};

/// 4-byte indirect expression operand (`FUN_00421350`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExprOperand {
    pub raw_val: i16,
    pub depth: i16,
}

impl ExprOperand {
    #[inline]
    pub fn from_raw_u32(raw: u32) -> Self {
        Self {
            raw_val: (raw & 0xFFFF) as i16,
            depth: ((raw >> 16) & 0xFFFF) as i16,
        }
    }
}

/// Addressing mode inside `0x1000..=0xF000` (`0x41f450`, `0x41f4b0`, `0x41f500`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandMode {
    Immediate,
    Reg,
    Var,
    Unknown(u8),
}

impl OperandMode {
    #[inline]
    pub const fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Immediate,
            1 => Self::Reg,
            2 => Self::Var,
            other => Self::Unknown(other),
        }
    }
}

/// All 15 high-family expression and assignment opcodes (`0x1000..=0xF000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiOpKind {
    /// `0x1000` (`0x41f590` -> `0x41f500`): `lhs = rhs; regs[reg_dst] = val`
    Assign,
    /// `0x2000` (`0x41f5c0`): `regs[reg_dst] = (lhs != 0 || rhs != 0) ? 1 : 0`
    LogicalOr,
    /// `0x3000` (`0x41f620`): `regs[reg_dst] = (lhs != 0 && rhs != 0) ? 1 : 0`
    LogicalAnd,
    /// `0x4000` (`0x41f680`): `regs[reg_dst] = (lhs == rhs) ? 1 : 0`
    Eq,
    /// `0x5000` (`0x41f6c0`): `regs[reg_dst] = (lhs >= rhs) ? 1 : 0`
    Ge,
    /// `0x6000` (`0x41f700`): `regs[reg_dst] = (lhs > rhs) ? 1 : 0`
    Gt,
    /// `0x7000` (`0x41f740`): `regs[reg_dst] = (lhs <= rhs) ? 1 : 0`
    Le,
    /// `0x8000` (`0x41f780`): `regs[reg_dst] = (lhs < rhs) ? 1 : 0`
    Lt,
    /// `0x9000` (`0x41f7c0`): `regs[reg_dst] = (lhs != rhs) ? 1 : 0`
    Ne,
    /// `0xA000` (`0x41f800`): `regs[reg_dst] = lhs.wrapping_add(rhs)`
    Add,
    /// `0xB000` (`0x41f840`): `regs[reg_dst] = lhs.wrapping_sub(rhs)`
    Sub,
    /// `0xC000` (`0x41f880`): `regs[reg_dst] = lhs.wrapping_mul(rhs)`
    Mul,
    /// `0xD000` (`0x41f8c0`): `regs[reg_dst] = lhs / rhs`
    Div,
    /// `0xE000` (`0x41f900`): `regs[reg_dst] = lhs % rhs`
    Mod,
    /// `0xF000` (`0x41f560`): `regs[reg_dst] = rhs`
    LoadReg,
}

impl HiOpKind {
    pub const fn from_u16(op_hi: u16) -> Option<Self> {
        match op_hi {
            0x1000 => Some(Self::Assign),
            0x2000 => Some(Self::LogicalOr),
            0x3000 => Some(Self::LogicalAnd),
            0x4000 => Some(Self::Eq),
            0x5000 => Some(Self::Ge),
            0x6000 => Some(Self::Gt),
            0x7000 => Some(Self::Le),
            0x8000 => Some(Self::Lt),
            0x9000 => Some(Self::Ne),
            0xA000 => Some(Self::Add),
            0xB000 => Some(Self::Sub),
            0xC000 => Some(Self::Mul),
            0xD000 => Some(Self::Div),
            0xE000 => Some(Self::Mod),
            0xF000 => Some(Self::LoadReg),
            _ => None,
        }
    }
}

/// Decoded high-family expression instruction (`0x1000..=0xF000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiOp {
    pub raw_opcode: u16,
    pub kind: HiOpKind,
    pub lhs_mode: OperandMode,
    pub rhs_mode: OperandMode,
    pub lhs_dep: u8,
    pub rhs_dep: u8,
    pub reg_dst: u16,
    pub lhs_raw: i16,
    pub rhs_raw: i16,
}

/// All 120 command opcodes (`0x03..=0xD5`) from `FUN_0041f9f0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOp {
    // 4.1 Control flow, script calls, subroutines (13)
    Jz { target: u32 },
    Jnz { target: u32 },
    Jmp { target: u32 },
    Exit,
    RandReg { reg_dst: u16 },
    WaitClick,
    WaitClickClear,
    GotoScript { script_id: ExprOperand, label_str: u32 },
    Sleep100ms { ticks: ExprOperand },
    CallScript { script_id: ExprOperand, label_str: u32, args: [ExprOperand; 10] },
    Ret { ret_val: ExprOperand },
    SubVmCall { script_id: ExprOperand },
    GosubLocal { target_pc: u32, args: [ExprOperand; 10] },

    // 4.2 Choices, menus, rollback & snapshots (10)
    Select6 {
        count: u16,
        prompt_str: u32,
        targets: [u32; 5],
        choice_strs: [u32; 5],
        opt0: ExprOperand,
        opt1: ExprOperand,
        opt2: ExprOperand,
    },
    MenuPause,
    CallSubsceneDll { str_idx: u32, arg0: ExprOperand },
    RollbackSave,
    RollbackRestore,
    SkipStop,
    RollbackFlag { enable: ExprOperand },
    SaveSnapshot,
    AbortIfFlag,
    MapSelect { mode: ExprOperand, param2: ExprOperand, unused: ExprOperand },

    // 4.3 Text display, textbox styling & backlog (27)
    TboxShow { slot: ExprOperand, show: ExprOperand },
    PortraitFace { face_id: ExprOperand, mode: ExprOperand },
    TboxStyle { p1: ExprOperand, p2: ExprOperand, p3: ExprOperand, p4: ExprOperand, p5: ExprOperand },
    TboxReset,
    TextClear { slot: ExprOperand },
    TextVoice {
        slot: ExprOperand,
        voice_id: ExprOperand,
        loop_cnt: ExprOperand,
        delay_50ms: ExprOperand,
        unused_str: u32,
        text_str: u32,
        clear: ExprOperand,
    },
    TextSay {
        slot: ExprOperand,
        p2: ExprOperand,
        p3: ExprOperand,
        p4: ExprOperand,
        text_str: u32,
        clear: ExprOperand,
    },
    TextClearWait { slot: ExprOperand },
    TboxTextPos { slot: ExprOperand, x: ExprOperand, y: ExprOperand },
    TboxRect { slot: ExprOperand, x1: ExprOperand, y1: ExprOperand, x2: ExprOperand, y2: ExprOperand },
    TboxFontSize { slot: ExprOperand, size: ExprOperand },
    TboxWrapCfg { slot: ExprOperand, val: ExprOperand },
    TboxMode { mode: ExprOperand },
    TboxColorPreset { slot: ExprOperand, preset: ExprOperand },
    TboxCharSpacing { slot: ExprOperand, px: ExprOperand },
    TboxLineSpacing { slot: ExprOperand, px: ExprOperand },
    TboxShadow { slot: ExprOperand, mode: ExprOperand },
    TboxSpeed { slot: ExprOperand, speed: ExprOperand, p3: ExprOperand },
    TboxFacePos { slot: ExprOperand, x: ExprOperand, y: ExprOperand },
    TboxIconPos { x: ExprOperand, y: ExprOperand },
    TextVoiceWaitMode { mode: ExprOperand },
    TboxFontType { slot: ExprOperand, font_id: ExprOperand },
    TboxBgColor { slot: ExprOperand, r: ExprOperand, g: ExprOperand, b: ExprOperand },
    TboxBgAlpha { alpha: ExprOperand },
    BacklogSetTitle { str_idx: u32, reset: ExprOperand },
    BacklogAddEntry { str_idx: u32, p2: ExprOperand, p3: ExprOperand, p4: ExprOperand },
    BacklogSetExtra { val: u32 },

    // 4.4 Graphics, CG, sprite layers, animations & effects (37)
    BgLoad { cg_id: ExprOperand, mode: ExprOperand },
    BgClear { white_flag: ExprOperand },
    SprLoadLayer0 { cg_id: ExprOperand, x: ExprOperand, y: ExprOperand, alpha: ExprOperand },
    Shake { count: ExprOperand, dir: ExprOperand, u3: ExprOperand, u4: ExprOperand },
    Flash { count: ExprOperand, mode: ExprOperand },
    BatchBegin,
    BatchEnd,
    Transition { effect_id: ExprOperand, steps: ExprOperand, delay_ms: ExprOperand },
    ZoomTransition { out_flag: ExprOperand, focus_digit: ExprOperand },
    SprLoad { layer: ExprOperand, img_id: ExprOperand, x: ExprOperand, y: ExprOperand, z_mode: ExprOperand, alpha: ExprOperand },
    SprLoadStr { layer: ExprOperand, x: ExprOperand, y: ExprOperand, z_mode: ExprOperand, alpha: ExprOperand, str_idx: u32 },
    SprLoadGrp { layer: ExprOperand, x: ExprOperand, y: ExprOperand, img_id: ExprOperand, alpha: ExprOperand },
    SprLoadRel { layer: ExprOperand, dx: ExprOperand, dy: ExprOperand, img_id: ExprOperand, alpha: ExprOperand },
    SprShowMode { layer: ExprOperand, mode: ExprOperand },
    SprClear { layer: ExprOperand, mode: ExprOperand },
    SprVisible { layer: ExprOperand, vis: ExprOperand },
    SprPos { layer: ExprOperand, x: ExprOperand, y: ExprOperand },
    SprBlend { layer: ExprOperand, mode: ExprOperand, param: ExprOperand },
    SprZ { layer: ExprOperand, z: ExprOperand },
    SprGroup { layer: ExprOperand, group_id: ExprOperand },
    EyesOverlay { es_id: ExprOperand, blend_flag: ExprOperand },
    EyesZ { z: ExprOperand },
    FilterOverlay { alpha_pct: ExprOperand, mode_flag: ExprOperand },
    FilterZ { z: ExprOperand },
    CoordScale { sx: ExprOperand, sy: ExprOperand },
    SprType { layer: ExprOperand, type_id: ExprOperand },
    SprLoadCentered { layer: ExprOperand, img_id: ExprOperand, mode: ExprOperand, alpha: ExprOperand },
    SprAnimX { layer: ExprOperand, p2: ExprOperand, p3: ExprOperand, p4: ExprOperand },
    SprAnimY { layer: ExprOperand, p2: ExprOperand, p3: ExprOperand, p4: ExprOperand },
    SprAnimStop { layer: ExprOperand },
    TimerWaitLoop { var_idx: ExprOperand, mode: ExprOperand, p3: ExprOperand },
    AutoClearAnim { enable: ExprOperand },
    SprAnimFrame { layer: ExprOperand, img_id: ExprOperand, x: ExprOperand, y: ExprOperand, slot: ExprOperand },
    SprAnimAlpha { layer: ExprOperand, p2: ExprOperand, p3: ExprOperand, p4: ExprOperand },
    SprAnchor { layer: ExprOperand, val: ExprOperand },
    SprDir { layer: ExprOperand, str_idx: u32 },
    SprClipRect { layer: ExprOperand, l: ExprOperand, t: ExprOperand, r: ExprOperand, b: ExprOperand },

    // 4.5 Audio & video control (10)
    BgmPlay { bgm_id: ExprOperand, loop_flag: ExprOperand, fade_ms: ExprOperand },
    BgmStop { unused: ExprOperand, fade_ms: ExprOperand },
    SeLoad { se_id: ExprOperand },
    SePlay { loop_cnt: ExprOperand, fade_flag: ExprOperand, fade_50ms: ExprOperand },
    SeStop { fade_ms: ExprOperand },
    MpgPlay { mpg_id: ExprOperand },
    VoicePlay { voice_id: ExprOperand, loop_cnt: ExprOperand, fade_flag: ExprOperand, delay_50ms: ExprOperand },
    VoiceStop { fade_ms: ExprOperand },
    SeWait,
    VoiceWait,

    // 4.6 Math, arrays, global flags, UI gauges & string vars (23)
    LoadArray { var_idx: ExprOperand, arr_idx: u32 },
    GetSprPos { layer: ExprOperand, out_x_var: ExprOperand, out_y_var: ExprOperand },
    Muldiv { a: ExprOperand, b: ExprOperand, c: ExprOperand },
    FtolNoop { val: ExprOperand },
    PowI16 { base: ExprOperand, exp: ExprOperand },
    GlobalFlag { flag_id: ExprOperand, is_set: ExprOperand },
    GaugeSetParts { idx: ExprOperand, num_id: ExprOperand, bar_id: ExprOperand, fix_id: ExprOperand },
    GaugeSetRange { idx: ExprOperand, min: ExprOperand, max: ExprOperand, p4: ExprOperand, p5: ExprOperand },
    GaugeVisible { idx: ExprOperand, vis: ExprOperand },
    GaugePos { idx: ExprOperand, x: ExprOperand, y: ExprOperand },
    GaugePartPos { idx: ExprOperand, nx: ExprOperand, ny: ExprOperand, bx: ExprOperand, by: ExprOperand },
    GaugeValue { idx: ExprOperand, val: ExprOperand, anim: ExprOperand },
    StrAssignLit { str_var: ExprOperand, str_idx: u32 },
    StrConcatLit { str_var: ExprOperand, str_idx: u32 },
    StrFromIntFw { str_var: ExprOperand, int_var: ExprOperand },
    StrToInt { out_var: ExprOperand, str_var: ExprOperand },
    StrCopy { dst: ExprOperand, src: ExprOperand },
    StrConcat { dst: ExprOperand, src: ExprOperand },
    RicheditOpen { mode: ExprOperand, u2: ExprOperand, u3: ExprOperand },
    RicheditCfg { idx: ExprOperand, val: ExprOperand, u3: ExprOperand, u4: ExprOperand, u5: ExprOperand },
    StrSaveGlobal { str_var: ExprOperand, slot: ExprOperand },
    StrLoadGlobal { str_var: ExprOperand, slot: ExprOperand },
    MemsetVars { start: ExprOperand, end: ExprOperand, val: ExprOperand },
}

impl CommandOp {
    /// Numeric 16-bit opcode ID (`0x03..=0xD5`).
    pub fn opcode(&self) -> u16 {
        match self {
            Self::Jz { .. } => 0x03,
            Self::Jnz { .. } => 0x04,
            Self::Jmp { .. } => 0x05,
            Self::Exit => 0x08,
            Self::RandReg { .. } => 0x09,
            Self::WaitClick => 0x0A,
            Self::WaitClickClear => 0x0B,
            Self::GotoScript { .. } => 0x0C,
            Self::Sleep100ms { .. } => 0x0D,
            Self::Select6 { .. } => 0x0E,
            Self::CallScript { .. } => 0x0F,
            Self::Ret { .. } => 0x10,
            Self::MenuPause => 0x11,
            Self::LoadArray { .. } => 0x12,
            Self::SubVmCall { .. } => 0x13,
            Self::BgLoad { .. } => 0x14,
            Self::BgClear { .. } => 0x15,
            Self::SprLoadLayer0 { .. } => 0x16,
            Self::Shake { .. } => 0x17,
            Self::Flash { .. } => 0x18,
            Self::CallSubsceneDll { .. } => 0x19,
            Self::BatchBegin => 0x1A,
            Self::BatchEnd => 0x1B,
            Self::Transition { .. } => 0x1C,
            Self::ZoomTransition { .. } => 0x1D,
            Self::SprLoad { .. } => 0x1E,
            Self::SprLoadStr { .. } => 0x20,
            Self::SprLoadGrp { .. } => 0x21,
            Self::SprLoadRel { .. } => 0x22,
            Self::SprShowMode { .. } => 0x23,
            Self::SprClear { .. } => 0x24,
            Self::SprVisible { .. } => 0x25,
            Self::SprPos { .. } => 0x26,
            Self::SprBlend { .. } => 0x27,
            Self::SprZ { .. } => 0x28,
            Self::SprGroup { .. } => 0x29,
            Self::TboxShow { .. } => 0x2A,
            Self::EyesOverlay { .. } => 0x2B,
            Self::EyesZ { .. } => 0x2C,
            Self::FilterOverlay { .. } => 0x2D,
            Self::FilterZ { .. } => 0x2E,
            Self::CoordScale { .. } => 0x2F,
            Self::PortraitFace { .. } => 0x30,
            Self::SprType { .. } => 0x31,
            Self::RollbackSave => 0x32,
            Self::RollbackRestore => 0x33,
            Self::SkipStop => 0x34,
            Self::RollbackFlag { .. } => 0x35,
            Self::SaveSnapshot => 0x37,
            Self::TboxStyle { .. } => 0x38,
            Self::TboxReset => 0x39,
            Self::AbortIfFlag => 0x3A,
            Self::SprLoadCentered { .. } => 0x3B,
            Self::BgmPlay { .. } => 0x3C,
            Self::BgmStop { .. } => 0x3D,
            Self::SeLoad { .. } => 0x3E,
            Self::SePlay { .. } => 0x3F,
            Self::SeStop { .. } => 0x40,
            Self::MpgPlay { .. } => 0x41,
            Self::VoicePlay { .. } => 0x42,
            Self::VoiceStop { .. } => 0x43,
            Self::SeWait => 0x44,
            Self::VoiceWait => 0x45,
            Self::SprAnimX { .. } => 0x46,
            Self::SprAnimY { .. } => 0x47,
            Self::SprAnimStop { .. } => 0x48,
            Self::TimerWaitLoop { .. } => 0x49,
            Self::AutoClearAnim { .. } => 0x4A,
            Self::SprAnimFrame { .. } => 0x4B,
            Self::SprAnimAlpha { .. } => 0x4D,
            Self::TextClear { .. } => 0x50,
            Self::TextVoice { .. } => 0x51,
            Self::TextSay { .. } => 0x52,
            Self::TextClearWait { .. } => 0x53,
            Self::TboxTextPos { .. } => 0x5A,
            Self::TboxRect { .. } => 0x5B,
            Self::TboxFontSize { .. } => 0x5C,
            Self::TboxWrapCfg { .. } => 0x5D,
            Self::TboxMode { .. } => 0x5E,
            Self::TboxColorPreset { .. } => 0x5F,
            Self::TboxCharSpacing { .. } => 0x60,
            Self::TboxLineSpacing { .. } => 0x61,
            Self::TboxShadow { .. } => 0x62,
            Self::TboxSpeed { .. } => 0x63,
            Self::TboxFacePos { .. } => 0x64,
            Self::TboxIconPos { .. } => 0x65,
            Self::TextVoiceWaitMode { .. } => 0x66,
            Self::TboxFontType { .. } => 0x67,
            Self::TboxBgColor { .. } => 0x68,
            Self::TboxBgAlpha { .. } => 0x69,
            Self::GetSprPos { .. } => 0x6E,
            Self::Muldiv { .. } => 0x6F,
            Self::FtolNoop { .. } => 0x70,
            Self::PowI16 { .. } => 0x71,
            Self::GlobalFlag { .. } => 0x72,
            Self::SprAnchor { .. } => 0x78,
            Self::SprDir { .. } => 0x79,
            Self::GaugeSetParts { .. } => 0x82,
            Self::GaugeSetRange { .. } => 0x83,
            Self::GaugeVisible { .. } => 0x84,
            Self::GaugePos { .. } => 0x86,
            Self::GaugePartPos { .. } => 0x87,
            Self::GaugeValue { .. } => 0x88,
            Self::StrAssignLit { .. } => 0x96,
            Self::StrConcatLit { .. } => 0x97,
            Self::StrFromIntFw { .. } => 0x98,
            Self::StrToInt { .. } => 0x99,
            Self::StrCopy { .. } => 0x9A,
            Self::StrConcat { .. } => 0x9B,
            Self::RicheditOpen { .. } => 0x9C,
            Self::RicheditCfg { .. } => 0x9D,
            Self::StrSaveGlobal { .. } => 0x9E,
            Self::StrLoadGlobal { .. } => 0x9F,
            Self::GosubLocal { .. } => 0xC8,
            Self::SprClipRect { .. } => 0xC9,
            Self::MemsetVars { .. } => 0xCA,
            Self::BacklogSetTitle { .. } => 0xD2,
            Self::BacklogAddEntry { .. } => 0xD3,
            Self::BacklogSetExtra { .. } => 0xD4,
            Self::MapSelect { .. } => 0xD5,
        }
    }

    /// Collect all bytecode jump targets referenced by this instruction (`0x03`, `0x04`, `0x05`, `0x0E`, `0xC8`).
    pub fn jump_targets(&self) -> Vec<u32> {
        match self {
            Self::Jz { target } | Self::Jnz { target } | Self::Jmp { target } => vec![*target],
            Self::Select6 { targets, .. } => targets.iter().copied().filter(|&t| t != 0).collect(),
            Self::GosubLocal { target_pc, .. } => vec![*target_pc],
            _ => Vec::new(),
        }
    }

    /// Collect all string table indices referenced by this instruction.
    pub fn string_refs(&self) -> Vec<u32> {
        match self {
            Self::GotoScript { label_str, .. } | Self::CallScript { label_str, .. } => {
                vec![*label_str]
            }
            Self::Select6 {
                prompt_str,
                choice_strs,
                ..
            } => {
                let mut v = Vec::with_capacity(6);
                v.push(*prompt_str);
                v.extend_from_slice(choice_strs);
                v
            }
            Self::CallSubsceneDll { str_idx, .. }
            | Self::BacklogSetTitle { str_idx, .. }
            | Self::BacklogAddEntry { str_idx, .. }
            | Self::BacklogSetExtra { val: str_idx }
            | Self::SprLoadStr { str_idx, .. }
            | Self::TextSay {
                text_str: str_idx, ..
            }
            | Self::SprDir { str_idx, .. }
            | Self::StrAssignLit { str_idx, .. }
            | Self::StrConcatLit { str_idx, .. } => vec![*str_idx],
            Self::TextVoice {
                unused_str,
                text_str,
                ..
            } => vec![*unused_str, *text_str],
            _ => Vec::new(),
        }
    }

    /// Collect all array table indices referenced by this instruction (`0x12`).
    pub fn array_refs(&self) -> Vec<u32> {
        match self {
            Self::LoadArray { arr_idx, .. } => vec![*arr_idx],
            _ => Vec::new(),
        }
    }
}

/// Decoded bytecode instruction payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionKind {
    Hi(HiOp),
    Cmd(CommandOp),
}

/// Complete decoded instruction with bytecode offset and byte length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub pc: u32,
    pub byte_len: u32,
    pub kind: InstructionKind,
}

struct Cursor<'a> {
    code: &'a [u8],
    pc: usize,
    insn_pc: usize,
}

impl<'a> Cursor<'a> {
    fn read_u16(&mut self) -> Result<u16> {
        if self.pc + 2 > self.code.len() {
            return Err(FujihimeError::InvalidScript {
                reason: format!(
                    "truncated u16 operand at pc={} (insn @ {})",
                    self.pc, self.insn_pc
                ),
            });
        }
        let v = u16::from_le_bytes([self.code[self.pc], self.code[self.pc + 1]]);
        self.pc += 2;
        Ok(v)
    }

    fn read_i16(&mut self) -> Result<i16> {
        Ok(self.read_u16()? as i16)
    }

    fn read_u32(&mut self) -> Result<u32> {
        if self.pc + 4 > self.code.len() {
            return Err(FujihimeError::InvalidScript {
                reason: format!(
                    "truncated u32 operand at pc={} (insn @ {})",
                    self.pc, self.insn_pc
                ),
            });
        }
        let v = u32::from_le_bytes(self.code[self.pc..self.pc + 4].try_into().unwrap());
        self.pc += 4;
        Ok(v)
    }

    fn read_expr(&mut self) -> Result<ExprOperand> {
        Ok(ExprOperand::from_raw_u32(self.read_u32()?))
    }

    fn read_expr10(&mut self) -> Result<[ExprOperand; 10]> {
        let mut arr = [ExprOperand {
            raw_val: 0,
            depth: 0,
        }; 10];
        for dst in &mut arr {
            *dst = self.read_expr()?;
        }
        Ok(arr)
    }
}

/// Decode one instruction from `code` at byte offset `pc`.
pub fn decode_instruction(code: &[u8], pc: usize) -> Result<Instruction> {
    if pc + 2 > code.len() {
        return Err(FujihimeError::InvalidScript {
            reason: format!("truncated opcode at pc={pc}, code_size={}", code.len()),
        });
    }
    let mut cur = Cursor {
        code,
        pc,
        insn_pc: pc,
    };
    let op = cur.read_u16()?;
    let hi = op & 0xF000;
    if hi != 0 {
        let kind = HiOpKind::from_u16(hi).ok_or_else(|| FujihimeError::InvalidScript {
            reason: format!("unknown high-family opcode 0x{op:04x} at pc={pc}"),
        })?;
        let reg_dst = cur.read_u16()?;
        let (lhs_raw, rhs_raw) = if hi == 0xF000 {
            (0i16, cur.read_i16()?)
        } else {
            (cur.read_i16()?, cur.read_i16()?)
        };
        let hi_op = HiOp {
            raw_opcode: op,
            kind,
            lhs_mode: OperandMode::from_bits(((op >> 10) & 3) as u8),
            rhs_mode: OperandMode::from_bits(((op >> 8) & 3) as u8),
            lhs_dep: ((op >> 4) & 0xF) as u8,
            rhs_dep: (op & 0xF) as u8,
            reg_dst,
            lhs_raw,
            rhs_raw,
        };
        return Ok(Instruction {
            pc: pc as u32,
            byte_len: (cur.pc - pc) as u32,
            kind: InstructionKind::Hi(hi_op),
        });
    }

    let cmd = match op {
        0x03 => CommandOp::Jz {
            target: cur.read_u32()?,
        },
        0x04 => CommandOp::Jnz {
            target: cur.read_u32()?,
        },
        0x05 => CommandOp::Jmp {
            target: cur.read_u32()?,
        },
        0x08 => CommandOp::Exit,
        0x09 => CommandOp::RandReg {
            reg_dst: cur.read_u16()?,
        },
        0x0A => CommandOp::WaitClick,
        0x0B => CommandOp::WaitClickClear,
        0x0C => CommandOp::GotoScript {
            script_id: cur.read_expr()?,
            label_str: cur.read_u32()?,
        },
        0x0D => CommandOp::Sleep100ms {
            ticks: cur.read_expr()?,
        },
        0x0E => {
            let count = cur.read_u16()?;
            let prompt_str = cur.read_u32()?;
            let targets = [
                cur.read_u32()?,
                cur.read_u32()?,
                cur.read_u32()?,
                cur.read_u32()?,
                cur.read_u32()?,
            ];
            let choice_strs = [
                cur.read_u32()?,
                cur.read_u32()?,
                cur.read_u32()?,
                cur.read_u32()?,
                cur.read_u32()?,
            ];
            let opt0 = cur.read_expr()?;
            let opt1 = cur.read_expr()?;
            let opt2 = cur.read_expr()?;
            CommandOp::Select6 {
                count,
                prompt_str,
                targets,
                choice_strs,
                opt0,
                opt1,
                opt2,
            }
        }
        0x0F => CommandOp::CallScript {
            script_id: cur.read_expr()?,
            label_str: cur.read_u32()?,
            args: cur.read_expr10()?,
        },
        0x10 => CommandOp::Ret {
            ret_val: cur.read_expr()?,
        },
        0x11 => CommandOp::MenuPause,
        0x12 => CommandOp::LoadArray {
            var_idx: cur.read_expr()?,
            arr_idx: cur.read_u32()?,
        },
        0x13 => CommandOp::SubVmCall {
            script_id: cur.read_expr()?,
        },
        0x14 => CommandOp::BgLoad {
            cg_id: cur.read_expr()?,
            mode: cur.read_expr()?,
        },
        0x15 => CommandOp::BgClear {
            white_flag: cur.read_expr()?,
        },
        0x16 => CommandOp::SprLoadLayer0 {
            cg_id: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
            alpha: cur.read_expr()?,
        },
        0x17 => CommandOp::Shake {
            count: cur.read_expr()?,
            dir: cur.read_expr()?,
            u3: cur.read_expr()?,
            u4: cur.read_expr()?,
        },
        0x18 => CommandOp::Flash {
            count: cur.read_expr()?,
            mode: cur.read_expr()?,
        },
        0x19 => CommandOp::CallSubsceneDll {
            str_idx: cur.read_u32()?,
            arg0: cur.read_expr()?,
        },
        0x1A => CommandOp::BatchBegin,
        0x1B => CommandOp::BatchEnd,
        0x1C => CommandOp::Transition {
            effect_id: cur.read_expr()?,
            steps: cur.read_expr()?,
            delay_ms: cur.read_expr()?,
        },
        0x1D => CommandOp::ZoomTransition {
            out_flag: cur.read_expr()?,
            focus_digit: cur.read_expr()?,
        },
        0x1E => CommandOp::SprLoad {
            layer: cur.read_expr()?,
            img_id: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
            z_mode: cur.read_expr()?,
            alpha: cur.read_expr()?,
        },
        0x20 => CommandOp::SprLoadStr {
            layer: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
            z_mode: cur.read_expr()?,
            alpha: cur.read_expr()?,
            str_idx: cur.read_u32()?,
        },
        0x21 => CommandOp::SprLoadGrp {
            layer: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
            img_id: cur.read_expr()?,
            alpha: cur.read_expr()?,
        },
        0x22 => CommandOp::SprLoadRel {
            layer: cur.read_expr()?,
            dx: cur.read_expr()?,
            dy: cur.read_expr()?,
            img_id: cur.read_expr()?,
            alpha: cur.read_expr()?,
        },
        0x23 => CommandOp::SprShowMode {
            layer: cur.read_expr()?,
            mode: cur.read_expr()?,
        },
        0x24 => CommandOp::SprClear {
            layer: cur.read_expr()?,
            mode: cur.read_expr()?,
        },
        0x25 => CommandOp::SprVisible {
            layer: cur.read_expr()?,
            vis: cur.read_expr()?,
        },
        0x26 => CommandOp::SprPos {
            layer: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
        },
        0x27 => CommandOp::SprBlend {
            layer: cur.read_expr()?,
            mode: cur.read_expr()?,
            param: cur.read_expr()?,
        },
        0x28 => CommandOp::SprZ {
            layer: cur.read_expr()?,
            z: cur.read_expr()?,
        },
        0x29 => CommandOp::SprGroup {
            layer: cur.read_expr()?,
            group_id: cur.read_expr()?,
        },
        0x2A => CommandOp::TboxShow {
            slot: cur.read_expr()?,
            show: cur.read_expr()?,
        },
        0x2B => CommandOp::EyesOverlay {
            es_id: cur.read_expr()?,
            blend_flag: cur.read_expr()?,
        },
        0x2C => CommandOp::EyesZ {
            z: cur.read_expr()?,
        },
        0x2D => CommandOp::FilterOverlay {
            alpha_pct: cur.read_expr()?,
            mode_flag: cur.read_expr()?,
        },
        0x2E => CommandOp::FilterZ {
            z: cur.read_expr()?,
        },
        0x2F => CommandOp::CoordScale {
            sx: cur.read_expr()?,
            sy: cur.read_expr()?,
        },
        0x30 => CommandOp::PortraitFace {
            face_id: cur.read_expr()?,
            mode: cur.read_expr()?,
        },
        0x31 => CommandOp::SprType {
            layer: cur.read_expr()?,
            type_id: cur.read_expr()?,
        },
        0x32 => CommandOp::RollbackSave,
        0x33 => CommandOp::RollbackRestore,
        0x34 => CommandOp::SkipStop,
        0x35 => CommandOp::RollbackFlag {
            enable: cur.read_expr()?,
        },
        0x37 => CommandOp::SaveSnapshot,
        0x38 => CommandOp::TboxStyle {
            p1: cur.read_expr()?,
            p2: cur.read_expr()?,
            p3: cur.read_expr()?,
            p4: cur.read_expr()?,
            p5: cur.read_expr()?,
        },
        0x39 => CommandOp::TboxReset,
        0x3A => CommandOp::AbortIfFlag,
        0x3B => CommandOp::SprLoadCentered {
            layer: cur.read_expr()?,
            img_id: cur.read_expr()?,
            mode: cur.read_expr()?,
            alpha: cur.read_expr()?,
        },
        0x3C => CommandOp::BgmPlay {
            bgm_id: cur.read_expr()?,
            loop_flag: cur.read_expr()?,
            fade_ms: cur.read_expr()?,
        },
        0x3D => CommandOp::BgmStop {
            unused: cur.read_expr()?,
            fade_ms: cur.read_expr()?,
        },
        0x3E => CommandOp::SeLoad {
            se_id: cur.read_expr()?,
        },
        0x3F => CommandOp::SePlay {
            loop_cnt: cur.read_expr()?,
            fade_flag: cur.read_expr()?,
            fade_50ms: cur.read_expr()?,
        },
        0x40 => CommandOp::SeStop {
            fade_ms: cur.read_expr()?,
        },
        0x41 => CommandOp::MpgPlay {
            mpg_id: cur.read_expr()?,
        },
        0x42 => CommandOp::VoicePlay {
            voice_id: cur.read_expr()?,
            loop_cnt: cur.read_expr()?,
            fade_flag: cur.read_expr()?,
            delay_50ms: cur.read_expr()?,
        },
        0x43 => CommandOp::VoiceStop {
            fade_ms: cur.read_expr()?,
        },
        0x44 => CommandOp::SeWait,
        0x45 => CommandOp::VoiceWait,
        0x46 => CommandOp::SprAnimX {
            layer: cur.read_expr()?,
            p2: cur.read_expr()?,
            p3: cur.read_expr()?,
            p4: cur.read_expr()?,
        },
        0x47 => CommandOp::SprAnimY {
            layer: cur.read_expr()?,
            p2: cur.read_expr()?,
            p3: cur.read_expr()?,
            p4: cur.read_expr()?,
        },
        0x48 => CommandOp::SprAnimStop {
            layer: cur.read_expr()?,
        },
        0x49 => CommandOp::TimerWaitLoop {
            var_idx: cur.read_expr()?,
            mode: cur.read_expr()?,
            p3: cur.read_expr()?,
        },
        0x4A => CommandOp::AutoClearAnim {
            enable: cur.read_expr()?,
        },
        0x4B => CommandOp::SprAnimFrame {
            layer: cur.read_expr()?,
            img_id: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
            slot: cur.read_expr()?,
        },
        0x4D => CommandOp::SprAnimAlpha {
            layer: cur.read_expr()?,
            p2: cur.read_expr()?,
            p3: cur.read_expr()?,
            p4: cur.read_expr()?,
        },
        0x50 => CommandOp::TextClear {
            slot: cur.read_expr()?,
        },
        0x51 => CommandOp::TextVoice {
            slot: cur.read_expr()?,
            voice_id: cur.read_expr()?,
            loop_cnt: cur.read_expr()?,
            delay_50ms: cur.read_expr()?,
            unused_str: cur.read_u32()?,
            text_str: cur.read_u32()?,
            clear: cur.read_expr()?,
        },
        0x52 => CommandOp::TextSay {
            slot: cur.read_expr()?,
            p2: cur.read_expr()?,
            p3: cur.read_expr()?,
            p4: cur.read_expr()?,
            text_str: cur.read_u32()?,
            clear: cur.read_expr()?,
        },
        0x53 => CommandOp::TextClearWait {
            slot: cur.read_expr()?,
        },
        0x5A => CommandOp::TboxTextPos {
            slot: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
        },
        0x5B => CommandOp::TboxRect {
            slot: cur.read_expr()?,
            x1: cur.read_expr()?,
            y1: cur.read_expr()?,
            x2: cur.read_expr()?,
            y2: cur.read_expr()?,
        },
        0x5C => CommandOp::TboxFontSize {
            slot: cur.read_expr()?,
            size: cur.read_expr()?,
        },
        0x5D => CommandOp::TboxWrapCfg {
            slot: cur.read_expr()?,
            val: cur.read_expr()?,
        },
        0x5E => CommandOp::TboxMode {
            mode: cur.read_expr()?,
        },
        0x5F => CommandOp::TboxColorPreset {
            slot: cur.read_expr()?,
            preset: cur.read_expr()?,
        },
        0x60 => CommandOp::TboxCharSpacing {
            slot: cur.read_expr()?,
            px: cur.read_expr()?,
        },
        0x61 => CommandOp::TboxLineSpacing {
            slot: cur.read_expr()?,
            px: cur.read_expr()?,
        },
        0x62 => CommandOp::TboxShadow {
            slot: cur.read_expr()?,
            mode: cur.read_expr()?,
        },
        0x63 => CommandOp::TboxSpeed {
            slot: cur.read_expr()?,
            speed: cur.read_expr()?,
            p3: cur.read_expr()?,
        },
        0x64 => CommandOp::TboxFacePos {
            slot: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
        },
        0x65 => CommandOp::TboxIconPos {
            x: cur.read_expr()?,
            y: cur.read_expr()?,
        },
        0x66 => CommandOp::TextVoiceWaitMode {
            mode: cur.read_expr()?,
        },
        0x67 => CommandOp::TboxFontType {
            slot: cur.read_expr()?,
            font_id: cur.read_expr()?,
        },
        0x68 => CommandOp::TboxBgColor {
            slot: cur.read_expr()?,
            r: cur.read_expr()?,
            g: cur.read_expr()?,
            b: cur.read_expr()?,
        },
        0x69 => CommandOp::TboxBgAlpha {
            alpha: cur.read_expr()?,
        },
        0x6E => CommandOp::GetSprPos {
            layer: cur.read_expr()?,
            out_x_var: cur.read_expr()?,
            out_y_var: cur.read_expr()?,
        },
        0x6F => CommandOp::Muldiv {
            a: cur.read_expr()?,
            b: cur.read_expr()?,
            c: cur.read_expr()?,
        },
        0x70 => CommandOp::FtolNoop {
            val: cur.read_expr()?,
        },
        0x71 => CommandOp::PowI16 {
            base: cur.read_expr()?,
            exp: cur.read_expr()?,
        },
        0x72 => CommandOp::GlobalFlag {
            flag_id: cur.read_expr()?,
            is_set: cur.read_expr()?,
        },
        0x78 => CommandOp::SprAnchor {
            layer: cur.read_expr()?,
            val: cur.read_expr()?,
        },
        0x79 => CommandOp::SprDir {
            layer: cur.read_expr()?,
            str_idx: cur.read_u32()?,
        },
        0x82 => CommandOp::GaugeSetParts {
            idx: cur.read_expr()?,
            num_id: cur.read_expr()?,
            bar_id: cur.read_expr()?,
            fix_id: cur.read_expr()?,
        },
        0x83 => CommandOp::GaugeSetRange {
            idx: cur.read_expr()?,
            min: cur.read_expr()?,
            max: cur.read_expr()?,
            p4: cur.read_expr()?,
            p5: cur.read_expr()?,
        },
        0x84 => CommandOp::GaugeVisible {
            idx: cur.read_expr()?,
            vis: cur.read_expr()?,
        },
        0x86 => CommandOp::GaugePos {
            idx: cur.read_expr()?,
            x: cur.read_expr()?,
            y: cur.read_expr()?,
        },
        0x87 => CommandOp::GaugePartPos {
            idx: cur.read_expr()?,
            nx: cur.read_expr()?,
            ny: cur.read_expr()?,
            bx: cur.read_expr()?,
            by: cur.read_expr()?,
        },
        0x88 => CommandOp::GaugeValue {
            idx: cur.read_expr()?,
            val: cur.read_expr()?,
            anim: cur.read_expr()?,
        },
        0x96 => CommandOp::StrAssignLit {
            str_var: cur.read_expr()?,
            str_idx: cur.read_u32()?,
        },
        0x97 => CommandOp::StrConcatLit {
            str_var: cur.read_expr()?,
            str_idx: cur.read_u32()?,
        },
        0x98 => CommandOp::StrFromIntFw {
            str_var: cur.read_expr()?,
            int_var: cur.read_expr()?,
        },
        0x99 => CommandOp::StrToInt {
            out_var: cur.read_expr()?,
            str_var: cur.read_expr()?,
        },
        0x9A => CommandOp::StrCopy {
            dst: cur.read_expr()?,
            src: cur.read_expr()?,
        },
        0x9B => CommandOp::StrConcat {
            dst: cur.read_expr()?,
            src: cur.read_expr()?,
        },
        0x9C => CommandOp::RicheditOpen {
            mode: cur.read_expr()?,
            u2: cur.read_expr()?,
            u3: cur.read_expr()?,
        },
        0x9D => CommandOp::RicheditCfg {
            idx: cur.read_expr()?,
            val: cur.read_expr()?,
            u3: cur.read_expr()?,
            u4: cur.read_expr()?,
            u5: cur.read_expr()?,
        },
        0x9E => CommandOp::StrSaveGlobal {
            str_var: cur.read_expr()?,
            slot: cur.read_expr()?,
        },
        0x9F => CommandOp::StrLoadGlobal {
            str_var: cur.read_expr()?,
            slot: cur.read_expr()?,
        },
        0xC8 => CommandOp::GosubLocal {
            target_pc: cur.read_u32()?,
            args: cur.read_expr10()?,
        },
        0xC9 => CommandOp::SprClipRect {
            layer: cur.read_expr()?,
            l: cur.read_expr()?,
            t: cur.read_expr()?,
            r: cur.read_expr()?,
            b: cur.read_expr()?,
        },
        0xCA => CommandOp::MemsetVars {
            start: cur.read_expr()?,
            end: cur.read_expr()?,
            val: cur.read_expr()?,
        },
        0xD2 => CommandOp::BacklogSetTitle {
            str_idx: cur.read_u32()?,
            reset: cur.read_expr()?,
        },
        0xD3 => CommandOp::BacklogAddEntry {
            str_idx: cur.read_u32()?,
            p2: cur.read_expr()?,
            p3: cur.read_expr()?,
            p4: cur.read_expr()?,
        },
        0xD4 => CommandOp::BacklogSetExtra {
            val: cur.read_u32()?,
        },
        0xD5 => CommandOp::MapSelect {
            mode: cur.read_expr()?,
            param2: cur.read_expr()?,
            unused: cur.read_expr()?,
        },
        _ => {
            return Err(FujihimeError::InvalidScript {
                reason: format!("unknown command opcode 0x{op:04x} at pc={pc}"),
            });
        }
    };

    Ok(Instruction {
        pc: pc as u32,
        byte_len: (cur.pc - pc) as u32,
        kind: InstructionKind::Cmd(cmd),
    })
}
