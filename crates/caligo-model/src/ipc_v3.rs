//! IPC v3 语义消息(计划 §6.2/C9:真实宿主身份握手)。
//!
//! v3 = v2 + bridge 侧宿主身份进 Hello:
//! - `host_nonce`:QQ bridge 宿主实例随机 nonce(宿主进程内一次生成,
//!   保持到 QQ 退出;PID 数字重用也无法与旧宿主混淆);
//! - `host_process_created_utc`:宿主进程创建时间(RFC3339 UTC;取不到
//!   必须如实为空)。
//!
//! v2 定义原样保留(`ipc_v2`);daemon 只受理 v3 握手 —— 旧消息因缺少
//! 必填字段反序列化失败而显式拒绝,不靠 serde 默认值放行(C9 反例 T05)。
//! frame/EventPayload/OutcomePayload/Role 与 v2 共用,语义不变。

use serde::{Deserialize, Serialize};

/// IPC 语义协议 v3。v2 保持原定义供历史对照;不接受 v2/v1 握手降级。
pub const PROTOCOL_VERSION_V3: u32 = 3;

pub use super::ipc_v2::{EventPayload, OutcomePayload, Role};

/// bridge → core 的 v3 消息(Hello 增加宿主身份;其余与 v2 一致)。
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
        /// 模块基线标识(与 v2 Hello 一致)。
        module_baseline: String,
        /// 宿主实例随机 nonce(进程内一次生成,保持到 QQ 退出)。
        host_nonce: u64,
        /// 宿主进程创建时间(RFC3339 UTC;取不到必须如实为空)。
        host_process_created_utc: String,
    },
    /// 接收事件上报(core 持久化成功 = EventAck 依据)。
    Event {
        event: EventPayload,
    },
    /// bridge 确认已进入 native 调用(执行侧受理后才可发送 —— C5)。
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

pub use super::ipc_v2::{ControlMsg, CoreToBridgeMsg, CoreToControlMsg};

#[cfg(test)]
mod tests {
    use super::*;

    /// v2 Hello 缺少 v3 必填字段 → 反序列化必须失败(不允许默认值放行)。
    #[test]
    fn v2_hello_without_host_identity_fails_to_decode() {
        let v2 = r#"{"t":"hello","d":{
            "protocol_version":2,
            "bridge_build":"b",
            "auth_token":"tok",
            "role":"bridge",
            "session_generation":1,
            "account":"10001",
            "module_baseline":"m"
        }}"#;
        let r: Result<BridgeMsg, _> = serde_json::from_str(v2);
        assert!(r.is_err(), "v2 Hello 不得被 v3 静默接受");
    }

    /// v3 Hello 完整字段可解码。
    #[test]
    fn v3_hello_with_host_identity_decodes() {
        let v3 = r#"{"t":"hello","d":{
            "protocol_version":3,
            "bridge_build":"b",
            "auth_token":"tok",
            "role":"bridge",
            "session_generation":1,
            "account":"10001",
            "module_baseline":"m",
            "host_nonce":123456789,
            "host_process_created_utc":"2026-10-08T00:00:00Z"
        }}"#;
        let msg: BridgeMsg = serde_json::from_str(v3).unwrap();
        match msg {
            BridgeMsg::Hello { host_nonce, host_process_created_utc, .. } => {
                assert_eq!(host_nonce, 123456789);
                assert_eq!(host_process_created_utc, "2026-10-08T00:00:00Z");
            }
            other => panic!("{other:?}"),
        }
    }
}
