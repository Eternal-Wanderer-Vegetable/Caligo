//! Caligo core:进程外核心(library 形态,K1 最小形态)。
//!
//! K1 范围:IPC 帧编解码与握手校验([`ipc`])。会话状态、动作队列、消息映射
//! 按 K2-K4 计划逐步落地;K1 不实现任何 QQ 调用。

pub mod ipc;

/// core 构建标识,进入握手 HelloAck。
pub const CORE_BUILD: &str = concat!("caligo-core ", env!("CARGO_PKG_VERSION"));
