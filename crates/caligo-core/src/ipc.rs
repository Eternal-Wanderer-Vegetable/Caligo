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
///
/// K4-D5 前置修复(计划 §10 ipc.rs 行):解码器**不把任意大小的输入整块
/// 复制进缓存** —— 内部缓存严格有界(`8 + MAX_FRAME_SIZE` 字节),超出
/// 部分直接从输入切片上取用;超长帧只读头即拒绝,不缓冲其内容。
#[derive(Debug, Default)]
pub struct FrameDecoder {
    /// 当前帧的组装缓冲(header + 已收到的 payload 前缀);长度恒 ≤ 8 + MAX。
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前缓存字节数(测试/观测用;恒 ≤ 8 + [`MAX_FRAME_SIZE`])。
    pub fn pending_bytes(&self) -> usize {
        self.buf.len()
    }

    /// 送入一段字节;完整帧的 payload 追加到 `out`。
    ///
    /// 返回 `Err` 时解码器状态不可信,调用方应丢弃重建(流已错位)。
    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<Vec<u8>>) -> Result<(), FrameError> {
        let mut ci = 0usize;
        // 1) 补齐 header(若上一轮停在半包头)。
        if self.buf.len() < 8 {
            let take = (8 - self.buf.len()).min(chunk.len() - ci);
            self.buf.extend_from_slice(&chunk[ci..ci + take]);
            ci += take;
            if self.buf.len() < 8 {
                return Ok(());
            }
        }
        loop {
            // 不变量:buf 非空时,buf[0..8] 是完整 header。
            let (magic, len) = parse_header(&self.buf);
            if magic != FRAME_MAGIC {
                return Err(FrameError::MagicMismatch(magic));
            }
            if len > MAX_FRAME_SIZE {
                return Err(FrameError::FrameTooLarge(len));
            }
            let want_total = 8 + len;
            let mut payload_whole_in_chunk = false;
            if self.buf.len() < want_total {
                let need = want_total - self.buf.len();
                let avail = chunk.len() - ci;
                if self.buf.len() == 8 && avail >= need {
                    // 快路径:payload 完全位于 chunk,不经过组装缓冲。
                    payload_whole_in_chunk = true;
                } else {
                    let take = need.min(avail);
                    if take == 0 {
                        return Ok(());
                    }
                    self.buf.extend_from_slice(&chunk[ci..ci + take]);
                    ci += take;
                    if self.buf.len() < want_total {
                        return Ok(());
                    }
                }
            }
            if payload_whole_in_chunk {
                out.push(chunk[ci..ci + len].to_vec());
                ci += len;
            } else {
                let payload = self.buf.split_off(8);
                out.push(payload);
            }
            // 继续下一帧:重建组装缓冲,先补 header(不足 8 字节则等待)。
            self.buf.clear();
            let take = 8.min(chunk.len() - ci);
            self.buf.extend_from_slice(&chunk[ci..ci + take]);
            ci += take;
            if self.buf.len() < 8 {
                return Ok(());
            }
        }
    }
}

fn parse_header(buf: &[u8]) -> (u32, usize) {
    (
        u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
        u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize,
    )
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

// ---------------------------------------------------------------------------
// K4-D5:IPC v2 语义层(计划 §6.4)。保留 CLG1 framing 与 1 MiB 上限;
// 语义协议升 v2,v1 不悄悄兼容。帧内 payload = JSON(`Envelope`)。
// 消息类型固定集;未知动作一律 Reject;不开放 arbitrary JS 执行。
// ---------------------------------------------------------------------------

/// IPC 语义协议 v2。v1(`PROTOCOL_VERSION`)仅供 K1 probe 握手回归测试。
pub const PROTOCOL_VERSION_V2: u32 = 2;

/// 客户端连接角色。core 同一管道体系服务两类角色,各自独立管道实例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// bridge 宿主侧(接收 Dispatch、上报 Event/结果)。
    Bridge,
    /// 测试客户端/控制面(提交 SendText、查询、Drain、Stop)。
    Control,
}

/// bridge → core 的 v2 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum BridgeMsg {
    Hello {
        protocol_version: u32,
        bridge_build: String,
        /// 认证凭据(hex;OS 随机,经 bootstrap 私有通道交付)。
        auth_token: String,
        role: Role,
        /// 期望接入的会话代次(bridge 侧已核实的会话身份)。
        session_generation: u64,
        account: String,
        /// 模块基线标识(T01 的逻辑层,与 v1 Hello 一致)。
        module_baseline: String,
    },
    /// 接收事件上报(core 持久化成功 = EventAck 依据)。
    Event {
        event: EventPayload,
    },
    /// bridge 确认已进入 native 调用。
    NativeStarted {
        request_id: String,
    },
    /// 发送结果(receipt 契约三分类,与 runtime::NativeResult 对应)。
    SendResult {
        request_id: String,
        outcome: OutcomePayload,
    },
    Health {
        ingress_pending: usize,
        native_ops_total: u64,
    },
    Stop {},
}

/// core → bridge 的 v2 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum CoreToBridgeMsg {
    HelloAck {
        protocol_version: u32,
        core_build: String,
        connection_epoch: u64,
        accepted: bool,
        reject_reason: Option<String>,
    },
    /// 派发参数化发送(带持久 request_id)。
    Dispatch {
        request_id: String,
        target: serde_json::Value,
        text: String,
    },
    HealthAck {},
    /// 事件受理回执(core 已持久化;suppressed=true 表示重复已抑制,
    /// bridge 据此前移缓存 —— 计划 §6.5:ACK 只在持久化后前移)。
    EventAck {
        event_seq: u64,
        suppressed: bool,
    },
    Stopped {},
    Reject {
        reason: String,
    },
}

/// control → core 的 v2 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum ControlMsg {
    Hello {
        protocol_version: u32,
        client_build: String,
        auth_token: String,
        role: Role,
    },
    SendText {
        request_id: String,
        target: serde_json::Value,
        text: String,
        deadline_ms: u64,
    },
    QueryRequest {
        request_id: String,
    },
    CancelQueued {
        request_id: String,
    },
    /// 拉取已持久化事件(交付游标前移)。
    DrainEvents {
        max: usize,
    },
    Health {},
    Stop {},
}

/// core → control 的 v2 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum CoreToControlMsg {
    HelloAck {
        protocol_version: u32,
        core_build: String,
        connection_epoch: u64,
        accepted: bool,
        reject_reason: Option<String>,
    },
    /// 受理确认(先持久后受理;拒绝时 reason 属"未受理"而非发送失败)。
    SendAccepted {
        request_id: String,
        state: String,
    },
    QueryResult {
        request_id: String,
        state: Option<String>,
        native_id: Option<String>,
    },
    CancelResult {
        request_id: String,
        /// cancelled_unsent / too_late / not_found。
        verdict: String,
    },
    EventsBatch {
        events: Vec<EventPayload>,
    },
    HealthAck {},
    Stopped {},
    Reject {
        reason: String,
    },
}

/// 事件载荷(与 journal::EventRecord 字段对齐;session 为 SessionKey DTO)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventPayload {
    pub event_seq: u64,
    pub session_generation: u64,
    pub session: serde_json::Value,
    pub direction: String,
    pub sender: String,
    pub native_id: String,
    pub text: String,
    pub platform_time: Option<u64>,
    pub observed_at_unix_ms: u64,
    pub source: String,
}

/// 发送结果三分类(计划 §6.6)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum OutcomePayload {
    Success {
        native_id: Option<String>,
    },
    Failure {
        reason: String,
    },
    Unknown,
}

/// v2 握手校验(bridge/control 共用)。校验顺序:协议版本 → 认证 → 角色。
/// 任一失败即拒绝,不做降级协商。
pub fn validate_hello_v2(
    protocol_version: u32,
    role: Role,
    expect_role: Role,
    core_build: &str,
) -> Result<(u32, String), RejectReason> {
    if protocol_version != PROTOCOL_VERSION_V2 {
        return Err(RejectReason::ProtocolVersionMismatch {
            got: protocol_version,
            want: PROTOCOL_VERSION_V2,
        });
    }
    // 认证(token)由调用方以 validate_auth 完成并先行拒绝;
    // 本函数只做版本与角色门控。
    if role != expect_role {
        return Err(RejectReason::AccountMismatch {
            got: format!("role:{role:?}"),
            want: format!("role:{expect_role:?}"),
        });
    }
    Ok((PROTOCOL_VERSION_V2, core_build.to_string()))
}

/// 认证比较(非常量时间即可接受的程度:同用户本机攻击面不在本阶段威胁
/// 边界内;此处仍用逐字节比较避免短路差异,详见 §6.4)。
pub fn validate_auth(presented: &str, expected_hex: &str) -> bool {
    let a = presented.as_bytes();
    let b = expected_hex.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
