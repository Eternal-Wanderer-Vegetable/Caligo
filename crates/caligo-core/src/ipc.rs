//! 自有 IPC 的 K1 最小实现:长度边界帧 + 握手校验(计划 §6.2)。
//!
//! 本模块只做与传输无关的部分:
//! - 帧编解码:`[u32 magic][u32 len(LE)][payload]`,最大帧 1 MiB(拟定初值,非实测结论);
//!   解码器接受任意分块的字节流,半包等待、超长与魔数错误立即报错。
//! - 握手:`Hello`/`HelloAck`,校验协议版本、模块基线(manifest 标识)与账号,
//!   对应计划 §5"版本与模块摘要确认 → 接入""有效账号 → 操作"两条约束的纯逻辑层。
//!
//! 命名管道传输、配对认证(会话随机认证信息)与有界事件队列在 K2+ 落地;
//! 帧内不携带地址或跨进程指针(计划 §6.2 禁止)。

use serde::{Deserialize, Serialize};

/// 与 `caligo-bridge` 的 `PROTOCOL_VERSION` 保持一致;由 CLI 侧测试交叉断言。
pub const PROTOCOL_VERSION: u32 = 1;
/// 最大帧(payload)字节数。拟定初值:1 MiB。
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
/// 帧头魔数,"CLG1"。
pub const FRAME_MAGIC: u32 = 0x314C4743;

/// 帧编解码错误。半包不是错误(解码器等待更多字节);以下为确定性错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// 魔数不匹配:流错位或非本协议数据。致命,调用方应复位解码器。
    MagicMismatch(u32),
    /// 声明的 payload 长度超过 [`MAX_FRAME_SIZE`]。致命。
    FrameTooLarge(usize),
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::MagicMismatch(got) => write!(f, "frame magic mismatch: {got:#010x}"),
            FrameError::FrameTooLarge(got) => {
                write!(f, "frame too large: {got} > {MAX_FRAME_SIZE}")
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// 把一个 payload 编码为完整帧。
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_FRAME_SIZE {
        return Err(FrameError::FrameTooLarge(payload.len()));
    }
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// 流式帧解码器:容忍半包,产出完整 payload。
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 送入一段字节;完整帧的 payload 追加到 `out`。
    ///
    /// 返回 `Err` 时解码器状态不可信,调用方应丢弃重建(流已错位)。
    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<Vec<u8>>) -> Result<(), FrameError> {
        self.buf.extend_from_slice(chunk);
        loop {
            if self.buf.len() < 8 {
                return Ok(());
            }
            let magic = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
            if magic != FRAME_MAGIC {
                return Err(FrameError::MagicMismatch(magic));
            }
            let len = u32::from_le_bytes([self.buf[4], self.buf[5], self.buf[6], self.buf[7]])
                as usize;
            if len > MAX_FRAME_SIZE {
                return Err(FrameError::FrameTooLarge(len));
            }
            if self.buf.len() < 8 + len {
                return Ok(()); // 半包:等待更多字节
            }
            let mut rest = self.buf.split_off(8 + len);
            let frame_payload = self.buf.split_off(8);
            out.push(frame_payload);
            self.buf = std::mem::take(&mut rest);
        }
    }
}

/// bridge → core 握手请求(计划 §6.2 握手字段的最小实现)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub protocol_version: u32,
    /// bridge 构建标识(如 "caligo-bridge 0.1.0")。
    pub bridge_build: String,
    /// 期望匹配的版本目录基线标识(manifest id,如 "qq-9.9.33-52230")。
    pub module_baseline: String,
    /// 会话代次。
    pub run_id: u64,
    /// 真实账号。K1 probe 阶段来自探测报告;K2 起必须来自已确认的入口。
    pub account: String,
    /// 能力集(如 ["probe-readonly"])。
    pub capabilities: Vec<String>,
}

/// core → bridge 握手应答。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloAck {
    pub protocol_version: u32,
    pub core_build: String,
    pub run_id: u64,
    pub accepted: bool,
    pub reject_reason: Option<String>,
}

/// core 侧握手期望。账号为 `None` 表示尚未绑定账号(只读探测阶段)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeExpectation<'a> {
    pub protocol_version: u32,
    pub module_baseline: &'a str,
    pub account: Option<&'a str>,
}

/// 拒绝原因,逐项对应计划 §5 的接入约束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// 协议版本不匹配 → 拒绝,不降级协商。
    ProtocolVersionMismatch { got: u32, want: u32 },
    /// 模块基线与 manifest 不匹配 → 拒绝 attach(计划 T01 的逻辑层)。
    ModuleBaselineMismatch { got: String, want: String },
    /// 账号不匹配 → 拒绝,与其他账号隔离(计划 T02 的逻辑层)。
    AccountMismatch { got: String, want: String },
}

impl core::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RejectReason::ProtocolVersionMismatch { got, want } => {
                write!(f, "protocol version mismatch: got {got}, want {want}")
            }
            RejectReason::ModuleBaselineMismatch { got, want } => {
                write!(f, "module baseline mismatch: got {got:?}, want {want:?}")
            }
            RejectReason::AccountMismatch { got, want } => {
                write!(f, "account mismatch: got {got:?}, want {want:?}")
            }
        }
    }
}

impl std::error::Error for RejectReason {}

/// 校验 Hello。通过则返回 accepted 的 [`HelloAck`];否则返回拒绝原因。
///
/// 校验顺序:协议版本 → 模块基线 → 账号。任一不匹配即拒绝,
/// 不做"试调用"(计划 §5:未匹配适配清单拒绝 attach/发送)。
pub fn validate_hello(
    hello: &Hello,
    expect: &HandshakeExpectation<'_>,
    core_build: &str,
) -> Result<HelloAck, RejectReason> {
    if hello.protocol_version != expect.protocol_version {
        return Err(RejectReason::ProtocolVersionMismatch {
            got: hello.protocol_version,
            want: expect.protocol_version,
        });
    }
    if hello.module_baseline != expect.module_baseline {
        return Err(RejectReason::ModuleBaselineMismatch {
            got: hello.module_baseline.clone(),
            want: expect.module_baseline.to_string(),
        });
    }
    if let Some(want) = expect.account {
        if hello.account != want {
            return Err(RejectReason::AccountMismatch {
                got: hello.account.clone(),
                want: want.to_string(),
            });
        }
    }
    Ok(HelloAck {
        protocol_version: expect.protocol_version,
        core_build: core_build.to_string(),
        run_id: hello.run_id,
        accepted: true,
        reject_reason: None,
    })
}
