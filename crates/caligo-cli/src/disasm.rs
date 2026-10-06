//! F-1 离线分析:基于 iced-x86 的 x64 反汇编子命令(只读文件,不执行任何指令)。
//!
//! 用途:解码 wrapper.node / QQNT.dll 导出函数,实证 `__qq::std` ABI 布局
//! (docs/research/js-context-acquisition.md §9,F-1 路线)。

use iced_x86::{Decoder, DecoderOptions, Instruction};

/// 反汇编指定 RVA 起始的字节序列,输出带 RVA 标注的 Intel 语法列表。
/// RIP 相对操作数与直接分支目标均换算为目标 RVA(基于 `rva` 基准)。
pub fn disasm_at(file: &[u8], file_offset: usize, rva: u32, len: usize) -> Result<String, String> {
    let end = (file_offset + len).min(file.len());
    if file_offset >= end {
        return Err("empty range".into());
    }
    let bytes = &file[file_offset..end];
    let mut decoder = Decoder::with_ip(64, bytes, rva as u64, DecoderOptions::NONE);
    let mut out = String::new();
    let mut instr = Instruction::default();
    while decoder.can_decode() {
        decoder.decode_out(&mut instr);
        if instr.is_invalid() {
            out.push_str(&format!("  {:#010x}  (invalid)\n", instr.ip() as u32));
            continue;
        }
        let start = instr.ip() as usize - rva as usize;
        let hex: String = bytes[start..start + instr.len()]
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();
        let mut line = format!("  {:#010x}  {:<30} {}", instr.ip() as u32, hex, instr);
        // RIP 相对操作数:标注绝对 VA(此处无基址,仅 RVA)与目标偏移。
        for i in 0..instr.op_count() {
            if instr.op_kind(i) == iced_x86::OpKind::Memory
                && instr.memory_base() == iced_x86::Register::RIP
            {
                let target = instr.memory_displacement64();
                line.push_str(&format!("   ; rip-rel -> rva {target:#x}"));
            }
        }
        // 直接分支/call 目标 → RVA。
        let flow = instr.flow_control();
        if matches!(
            flow,
            iced_x86::FlowControl::Call
                | iced_x86::FlowControl::ConditionalBranch
                | iced_x86::FlowControl::UnconditionalBranch
        ) {
            let t = instr.near_branch_target() as u32;
            line.push_str(&format!("   ; -> rva {t:#x}"));
        }
        line.push('\n');
        out.push_str(&line);
    }
    Ok(out)
}
