//! RScript `.gsc` parser, disassembler, and complete bytecode virtual machine.

pub mod gsc;
pub mod opcodes;
pub mod runtime;

pub use gsc::{GscDisassembly, GscHeader, GscQuirk, GscScript};
pub use opcodes::{
    decode_instruction, CommandOp, ExprOperand, HiOp, HiOpKind, Instruction, InstructionKind,
    OperandMode,
};
pub use runtime::{
    expand_script_string, resolve_color_preset, BacklogEntry, ChoicePrompt, EyesOverlayState,
    FilterOverlayState, HaltReason, VmGaugeState, VmHost, VmLayerState, VmRuntime, VmState,
    VmTextboxState, VmYield, BACKLOG_CAPACITY, MAX_CALL_DEPTH, NUM_REGS, TOTAL_VARS,
};
