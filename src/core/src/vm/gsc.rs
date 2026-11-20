//! `.gsc` compiled RScript parser & validator (`KhimeDL.exe` `0x417c70`, `0x4179d0`, `0x417ab0`).

use std::collections::{HashMap, HashSet};

use encoding_rs::{GBK, SHIFT_JIS};

use crate::error::{FujihimeError, Result};
use crate::text::caret::TextEncoding;
use crate::vm::opcodes::{decode_instruction, Instruction, InstructionKind};

/// 36-byte `.gsc` header (`9 x u32` little-endian, `0x417c70`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GscHeader {
    pub file_size: u32,
    pub header_size: u32,
    pub code_size: u32,
    pub str_off_size: u32,
    pub str_data_size: u32,
    pub arr_off_size: u32,
    pub arr_data_u16_cnt: u32,
    pub lbl_table_size: u32,
    pub lbl_names_size: u32,
}

impl GscHeader {
    /// Logical expected byte length from header section sizes.
    pub fn calc_size(&self) -> usize {
        36 + self.code_size as usize
            + self.str_off_size as usize
            + self.str_data_size as usize
            + self.arr_off_size as usize
            + (self.arr_data_u16_cnt as usize) * 2
            + (self.lbl_table_size as usize) * 2
            + self.lbl_names_size as usize
    }
}

/// CHS V1.0 repacker compatibility quirk observed during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GscQuirk {
    /// Trailing `0x00` bytes beyond `file_size` (`1122.gsc` +2B, `2233.gsc` +6B, `3202.gsc` +1B).
    TrailingNulPadding(usize),
    /// Truncated all-zero dummy `arr`/`lbl` tail (`1160.gsc`: `188 B` vs `calc_size = 192 B`).
    ShortZeroDummyTail,
}

/// Summary produced by [`GscScript::disassemble_all`].
#[derive(Debug, Clone)]
pub struct GscDisassembly {
    pub instructions: Vec<Instruction>,
    pub hi_count: usize,
    pub cmd_count: usize,
    pub jump_count: usize,
    pub eof_jump_count: usize,
    pub str_ref_count: usize,
    pub arr_ref_count: usize,
}

/// Parsed `.gsc` script module.
#[derive(Debug, Clone)]
pub struct GscScript {
    pub script_id: u16,
    pub header: GscHeader,
    pub code: Vec<u8>,
    pub str_offsets: Vec<u32>,
    pub str_data: Vec<u8>,
    pub arr_offsets: Vec<u32>,
    pub arr_data: Vec<u16>,
    pub label_name_offsets: Vec<u32>,
    pub label_targets: Vec<u32>,
    pub label_names_data: Vec<u8>,
    labels_by_name: HashMap<Vec<u8>, u32>,
    pub quirk: Option<GscQuirk>,
    pub chs_zeroed_labels: bool,
}

impl GscScript {
    /// Parse a `.gsc` binary blob (`0x417c70`), strictly matching original layout while
    /// faithfully tolerating known CHS V1.0 repacker tail quirks when `allow_chs_quirks` is true.
    pub fn parse(script_id: u16, data: &[u8], allow_chs_quirks: bool) -> Result<Self> {
        if data.len() < 36 {
            return Err(FujihimeError::InvalidScript {
                reason: format!("script {script_id:04} too short ({} < 36)", data.len()),
            });
        }
        let rd32 = |off: usize| -> u32 {
            u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
        };
        let header = GscHeader {
            file_size: rd32(0x00),
            header_size: rd32(0x04),
            code_size: rd32(0x08),
            str_off_size: rd32(0x0C),
            str_data_size: rd32(0x10),
            arr_off_size: rd32(0x14),
            arr_data_u16_cnt: rd32(0x18),
            lbl_table_size: rd32(0x1C),
            lbl_names_size: rd32(0x20),
        };

        if header.header_size != 36 {
            return Err(FujihimeError::InvalidScript {
                reason: format!(
                    "script {script_id:04} invalid header_size={} != 36",
                    header.header_size
                ),
            });
        }
        if !header.str_off_size.is_multiple_of(4)
            || !header.arr_off_size.is_multiple_of(4)
            || !header.lbl_table_size.is_multiple_of(4)
        {
            return Err(FujihimeError::InvalidScript {
                reason: format!("script {script_id:04} has unaligned table size"),
            });
        }

        let calc_sz = header.calc_size();
        let fsize = header.file_size as usize;
        let mut quirk = None;

        if fsize == data.len() && calc_sz == data.len() {
            // Exact original layout.
        } else if allow_chs_quirks
            && fsize == calc_sz
            && data.len() > fsize
            && data[fsize..].iter().all(|&b| b == 0)
        {
            quirk = Some(GscQuirk::TrailingNulPadding(data.len() - fsize));
        } else if allow_chs_quirks
            && fsize == data.len()
            && calc_sz == data.len() + 4
            && header.arr_off_size == 4
            && header.arr_data_u16_cnt == 1
            && header.lbl_table_size == 4
            && header.lbl_names_size == 1
        {
            let tail_start = 36
                + header.code_size as usize
                + header.str_off_size as usize
                + header.str_data_size as usize;
            if tail_start <= data.len() && data[tail_start..].iter().all(|&b| b == 0) {
                quirk = Some(GscQuirk::ShortZeroDummyTail);
            } else {
                return Err(FujihimeError::InvalidScript {
                    reason: format!(
                        "script {script_id:04} non-zero short tail: len={}, calc={calc_sz}",
                        data.len()
                    ),
                });
            }
        } else {
            return Err(FujihimeError::InvalidScript {
                reason: format!(
                    "script {script_id:04} size mismatch: len={}, file_size={fsize}, calc={calc_sz}",
                    data.len()
                ),
            });
        }

        let mut off = 36usize;
        let code_end = off + header.code_size as usize;
        let code = data[off..code_end].to_vec();
        off = code_end;

        let str_cnt = (header.str_off_size / 4) as usize;
        let mut str_offsets = Vec::with_capacity(str_cnt);
        for _ in 0..str_cnt {
            str_offsets.push(rd32(off));
            off += 4;
        }
        let str_data_end = off + header.str_data_size as usize;
        let str_data = data[off..str_data_end].to_vec();
        off = str_data_end;

        // Validate string offsets & NUL termination
        for (idx, &soff) in str_offsets.iter().enumerate() {
            let s = soff as usize;
            if s >= str_data.len() {
                return Err(FujihimeError::InvalidScript {
                    reason: format!(
                        "script {script_id:04} str_off[{idx}]={s} >= {}",
                        str_data.len()
                    ),
                });
            }
            if !str_data[s..].contains(&0) {
                return Err(FujihimeError::InvalidScript {
                    reason: format!("script {script_id:04} unterminated string [{idx}]"),
                });
            }
        }

        let arr_cnt = (header.arr_off_size / 4) as usize;
        let lbl_cnt = (header.lbl_table_size / 4) as usize;

        let (arr_offsets, arr_data, label_name_offsets, label_targets, label_names_data) =
            if quirk == Some(GscQuirk::ShortZeroDummyTail) {
                (
                    vec![0u32; arr_cnt],
                    vec![0u16; header.arr_data_u16_cnt as usize],
                    vec![0u32; lbl_cnt],
                    vec![0u32; lbl_cnt],
                    vec![0u8; header.lbl_names_size as usize],
                )
            } else {
                let mut a_offs = Vec::with_capacity(arr_cnt);
                for _ in 0..arr_cnt {
                    a_offs.push(rd32(off));
                    off += 4;
                }
                let mut a_dat = Vec::with_capacity(header.arr_data_u16_cnt as usize);
                for _ in 0..header.arr_data_u16_cnt {
                    let v = u16::from_le_bytes([data[off], data[off + 1]]);
                    a_dat.push(v);
                    off += 2;
                }
                for &idx in a_offs.iter().skip(1) {
                    let u = idx as usize;
                    if u >= a_dat.len() || u + 1 + (a_dat[u] as usize) > a_dat.len() {
                        return Err(FujihimeError::InvalidScript {
                            reason: format!(
                                "script {script_id:04} array offset {u} out of bounds"
                            ),
                        });
                    }
                }

                let mut l_name_offs = Vec::with_capacity(lbl_cnt);
                for _ in 0..lbl_cnt {
                    l_name_offs.push(rd32(off));
                    off += 4;
                }
                let mut l_targets = Vec::with_capacity(lbl_cnt);
                for _ in 0..lbl_cnt {
                    l_targets.push(rd32(off));
                    off += 4;
                }
                let l_dat_end = off + header.lbl_names_size as usize;
                let l_dat = data[off..l_dat_end].to_vec();

                (a_offs, a_dat, l_name_offs, l_targets, l_dat)
            };

        let mut labels_by_name = HashMap::new();
        let mut chs_zeroed_labels = false;
        for i in 1..label_name_offsets.len() {
            let noff = label_name_offsets[i] as usize;
            if noff >= label_names_data.len() {
                return Err(FujihimeError::InvalidScript {
                    reason: format!(
                        "script {script_id:04} label name offset {noff} out of bounds"
                    ),
                });
            }
            let Some(rel_end) = label_names_data[noff..].iter().position(|&b| b == 0) else {
                return Err(FujihimeError::InvalidScript {
                    reason: format!("script {script_id:04} unterminated label name at {noff}"),
                });
            };
            let name = &label_names_data[noff..noff + rel_end];
            if name.is_empty() {
                chs_zeroed_labels = true;
            } else {
                labels_by_name.insert(name.to_vec(), label_targets[i]);
            }
        }

        Ok(Self {
            script_id,
            header,
            code,
            str_offsets,
            str_data,
            arr_offsets,
            arr_data,
            label_name_offsets,
            label_targets,
            label_names_data,
            labels_by_name,
            quirk,
            chs_zeroed_labels,
        })
    }

    /// Repair zeroed CHS recall labels (`1125`, `2058`, `2059`, `2245`, `3177`, `3179`, `3262`,
    /// `4203`, `5110`) by borrowing the label table from the original Japanese script.
    pub fn repair_labels_from(&mut self, orig: &GscScript) {
        if self.chs_zeroed_labels && !orig.labels_by_name.is_empty() {
            self.label_name_offsets = orig.label_name_offsets.clone();
            self.label_targets = orig.label_targets.clone();
            self.label_names_data = orig.label_names_data.clone();
            self.labels_by_name = orig.labels_by_name.clone();
            self.chs_zeroed_labels = false;
        }
    }

    /// Number of string entries in the script (including index 0 empty string).
    pub fn str_count(&self) -> usize {
        self.str_offsets.len()
    }

    /// Raw NUL-trimmed bytes for string `idx` (`0x417a90`).
    pub fn raw_string(&self, idx: u32) -> Option<&[u8]> {
        let off = *self.str_offsets.get(idx as usize)? as usize;
        if off >= self.str_data.len() {
            return None;
        }
        let end = self.str_data[off..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| off + p)
            .unwrap_or(self.str_data.len());
        Some(&self.str_data[off..end])
    }

    /// Decode string `idx` using `encoding` (`CP932` or `GBK`).
    pub fn decode_string(&self, idx: u32, encoding: TextEncoding) -> Result<String> {
        let raw = self
            .raw_string(idx)
            .ok_or_else(|| FujihimeError::InvalidScript {
                reason: format!(
                    "script {:04} string index {idx} out of bounds ({})",
                    self.script_id,
                    self.str_offsets.len()
                ),
            })?;
        let enc = match encoding {
            TextEncoding::Cp932 => SHIFT_JIS,
            TextEncoding::Gbk => GBK,
        };
        let (decoded, _, had_errors) = enc.decode(raw);
        if had_errors {
            return Err(FujihimeError::InvalidScript {
                reason: format!(
                    "script {:04} string[{idx}] failed {encoding:?} decode",
                    self.script_id
                ),
            });
        }
        Ok(decoded.into_owned())
    }

    /// Lookup integer array slice `[elem_0, ..., elem_{len-1}]` for `arr_idx` (`0x417ab0`).
    pub fn get_array(&self, arr_idx: u32) -> Option<&[u16]> {
        let start = *self.arr_offsets.get(arr_idx as usize)? as usize;
        let len = *self.arr_data.get(start)? as usize;
        self.arr_data.get(start + 1..start + 1 + len)
    }

    /// Lookup label target `pc` by raw ASCII label name (`0x4179d0`).
    pub fn find_label(&self, name: &[u8]) -> Option<u32> {
        self.labels_by_name.get(name).copied()
    }

    /// Number of non-empty named labels.
    pub fn label_count(&self) -> usize {
        self.labels_by_name.len()
    }

    /// Perform full linear disassembly and structural validation across all bytecode,
    /// jump targets (allowing `target == code.len()`), label targets, string refs, and array refs.
    pub fn disassemble_all(&self) -> Result<GscDisassembly> {
        let mut pc = 0usize;
        let code_sz = self.code.len();
        let mut insn_starts = HashSet::new();
        let mut instructions = Vec::new();
        let mut hi_count = 0usize;
        let mut cmd_count = 0usize;
        let mut jump_targets = Vec::new();
        let mut str_refs = Vec::new();
        let mut arr_refs = Vec::new();

        while pc < code_sz {
            insn_starts.insert(pc as u32);
            let insn = decode_instruction(&self.code, pc)?;
            pc += insn.byte_len as usize;
            match &insn.kind {
                InstructionKind::Hi(_) => {
                    hi_count += 1;
                }
                InstructionKind::Cmd(cmd) => {
                    cmd_count += 1;
                    for dst in cmd.jump_targets() {
                        jump_targets.push((insn.pc, dst));
                    }
                    for s in cmd.string_refs() {
                        str_refs.push((insn.pc, s));
                    }
                    for a in cmd.array_refs() {
                        arr_refs.push((insn.pc, a));
                    }
                }
            }
            instructions.push(insn);
        }

        if pc != code_sz {
            return Err(FujihimeError::InvalidScript {
                reason: format!(
                    "script {:04} pc end mismatch {pc} != {code_sz}",
                    self.script_id
                ),
            });
        }

        let mut eof_jump_count = 0usize;
        for (src_pc, dst) in &jump_targets {
            if *dst as usize == code_sz {
                eof_jump_count += 1;
            } else if !insn_starts.contains(dst) {
                return Err(FujihimeError::InvalidScript {
                    reason: format!(
                        "script {:04} invalid jump target {dst} from pc={src_pc} (code_sz={code_sz})",
                        self.script_id
                    ),
                });
            }
        }

        for &lc in self.label_targets.iter().skip(1) {
            if !insn_starts.contains(&lc) {
                return Err(FujihimeError::InvalidScript {
                    reason: format!(
                        "script {:04} invalid label target {lc}",
                        self.script_id
                    ),
                });
            }
        }

        for (src_pc, sidx) in &str_refs {
            if (*sidx as usize) >= self.str_offsets.len() {
                return Err(FujihimeError::InvalidScript {
                    reason: format!(
                        "script {:04} invalid str_ref {sidx} at pc={src_pc}",
                        self.script_id
                    ),
                });
            }
        }

        for (src_pc, aidx) in &arr_refs {
            if (*aidx as usize) >= self.arr_offsets.len() {
                return Err(FujihimeError::InvalidScript {
                    reason: format!(
                        "script {:04} invalid arr_ref {aidx} at pc={src_pc}",
                        self.script_id
                    ),
                });
            }
        }

        Ok(GscDisassembly {
            instructions,
            hi_count,
            cmd_count,
            jump_count: jump_targets.len(),
            eof_jump_count,
            str_ref_count: str_refs.len(),
            arr_ref_count: arr_refs.len(),
        })
    }
}
