//! OneBot V11 动作层(N4 第一批;计划 §7-N4)。
//!
//! 职责:标准动作解析/响应框架(status/retcode/data/echo)+ 动作路由;
//! 发送经 [`SendSink`](trait,由 N3 NativeSession 实现,内部走
//! `qq_wire::send` 已证形状),查询经 [`StoreReader`](trait,由消息存储
//! 实现)。本模块不触碰原生内存、不做网络 IO —— transport(HTTP/WS)
//! 在外层,四种 transport 共用本层(计划:不复制 native 逻辑)。
//!
//! 语义边界(计划 N4):
//! - 未支持动作显式失败(retcode 1404),不静默;
//! - echo 原样返回(任意 JSON 类型);
//! - 发送受理 ≠ 送达:受理成功返回 **status=async**(结果经事件通道),
//!   不伪造 status=ok/message_id。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// OneBot V11 动作响应框架。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActionResponse {
    pub status: &'static str,
    pub retcode: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echo: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wording: Option<String>,
}

impl ActionResponse {
    pub fn ok(data: Value, echo: Option<Value>) -> Self {
        Self { status: "ok", retcode: 0, data: Some(data), echo, wording: None }
    }
    pub fn async_accepted(echo: Option<Value>) -> Self {
        Self { status: "async", retcode: 1, data: None, echo, wording: None }
    }
    pub fn failed(retcode: i32, wording: &str, echo: Option<Value>) -> Self {
        Self { status: "failed", retcode, data: None, echo, wording: Some(wording.to_owned()) }
    }
}

pub const RET_UNSUPPORTED: i32 = 1404;
pub const RET_BAD_PARAMS: i32 = 1400;
pub const RET_BAD_REQUEST: i32 = 1440;

/// 发送载荷(已通过参数校验;wire 编码与原生派发由 sink 承担)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendPayload {
    /// 群 = Some(群码);私聊 = None。
    pub group: Option<u64>,
    pub peer_uid: Option<String>,
    pub text: String,
}

/// 发送汇(N3 NativeSession 实现;此处解耦以便测试)。
pub trait SendSink {
    /// 受理发送;Ok(true) = 已派发(结果走事件通道),Err(原因) = 拒绝。
    fn send(&mut self, payload: &SendPayload) -> Result<bool, String>;
}

/// 消息查询源(消息存储实现)。
pub trait StoreReader {
    /// 按 OneBot message_id 查询;返回 (seq, 平台时间, 文本)。
    fn get_msg(&self, message_id: i64) -> Option<(u64, u64, String)>;
    /// 登录账号(启动时由宿主身份填充)。
    fn self_id(&self) -> u64;
}

/// 解析 message 段:字符串或标准数组,提取纯文本;未支持段显式失败。
pub fn extract_text(message: &Value) -> Result<String, String> {
    match message {
        Value::String(s) => Ok(s.clone()),
        Value::Array(items) => {
            let mut out = String::new();
            for item in items {
                let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "text" => {
                        out.push_str(
                            item.get("data")
                                .and_then(|d| d.get("text"))
                                .and_then(|v| v.as_str())
                                .unwrap_or(""),
                        );
                    }
                    "" => return Err("message 段缺少 type".into()),
                    other => {
                        return Err(format!("未支持消息段 {other}(N6 后续;显式拒绝不静默)"));
                    }
                }
            }
            if out.is_empty() {
                return Err("message 为空".into());
            }
            Ok(out)
        }
        _ => Err("message 须为字符串或段数组".into()),
    }
}

fn get_u64(params: &Value, key: &str) -> Result<u64, ActionResponse> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ActionResponse::failed(RET_BAD_PARAMS, &format!("{key} 必填"), None))
}

fn get_message_text(params: &Value) -> Result<String, ActionResponse> {
    match params.get("message").map(extract_text) {
        Some(Ok(t)) => Ok(t),
        Some(Err(e)) => Err(ActionResponse::failed(RET_BAD_PARAMS, &e, None)),
        None => Err(ActionResponse::failed(RET_BAD_PARAMS, "message 必填", None)),
    }
}

fn dispatch_send(sink: &mut dyn SendSink, echo: Option<Value>, payload: SendPayload) -> ActionResponse {
    match sink.send(&payload) {
        Ok(true) => ActionResponse::async_accepted(echo),
        Ok(false) => ActionResponse::failed(1500, "会话未就绪", echo),
        Err(e) => ActionResponse::failed(1500, &e, echo),
    }
}

/// 处理一条动作(action 名 + 参数 + echo)。
pub fn handle_action(
    action: &str,
    params: &Value,
    echo: Option<Value>,
    sink: &mut dyn SendSink,
    store: &dyn StoreReader,
) -> ActionResponse {
    let with_echo = |mut r: ActionResponse, echo: &Option<Value>| {
        r.echo = echo.clone();
        r
    };
    match action {
        "send_group_msg" => {
            let group = match get_u64(params, "group_id") {
                Ok(v) => v,
                Err(r) => return with_echo(r, &echo),
            };
            let text = match get_message_text(params) {
                Ok(t) => t,
                Err(r) => return with_echo(r, &echo),
            };
            dispatch_send(
                sink,
                echo,
                SendPayload { group: Some(group), peer_uid: None, text },
            )
        }
        "send_private_msg" => {
            if let Err(r) = get_u64(params, "user_id") {
                return with_echo(r, &echo);
            }
            let text = match get_message_text(params) {
                Ok(t) => t,
                Err(r) => return with_echo(r, &echo),
            };
            // 私聊路由:uid → peerUid 解析属 N3 接线;载荷此时不带 peer。
            dispatch_send(sink, echo, SendPayload { group: None, peer_uid: None, text })
        }
        "get_msg" => {
            let Some(mid) = params.get("message_id").and_then(|v| v.as_i64()) else {
                return with_echo(ActionResponse::failed(RET_BAD_PARAMS, "message_id 必填", None), &echo);
            };
            match store.get_msg(mid) {
                Some((seq, time, text)) => {
                    let mut data = serde_json::Map::new();
                    data.insert("message_id".into(), Value::from(mid));
                    data.insert("message_seq".into(), Value::from(seq));
                    data.insert("time".into(), Value::from(time));
                    data.insert("raw_message".into(), Value::from(text));
                    ActionResponse::ok(Value::Object(data), echo)
                }
                None => with_echo(ActionResponse::failed(RET_BAD_REQUEST, "message 不存在", None), &echo),
            }
        }
        "get_login_info" => {
            let data = serde_json::json!({
                "user_id": store.self_id(),
                "nickname": ""
            });
            ActionResponse::ok(data, echo)
        }
        "get_version_info" => {
            let data = serde_json::json!({
                "app_name": "Caligo",
                "protocol_version": "v11",
                "platform": "windows"
            });
            ActionResponse::ok(data, echo)
        }
        "get_status" => {
            // 真实状态分解(daemon/QQ attached/session ready)由 N3 接线
            // 填充;此处如实表达能力边界,不固定 online=true。
            let data = serde_json::json!({
                "online": false,
                "good": false,
                "_caligo": "status 分解字段随 N3 接线交付;当前为框架占位"
            });
            ActionResponse::ok(data, echo)
        }
        other => with_echo(ActionResponse::failed(RET_UNSUPPORTED, "未支持动作", None), &echo),
    }
}

#[derive(Debug, Deserialize)]
struct RawAction {
    action: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    echo: Option<Value>,
}

/// 解析一条 OneBot 动作 JSON(transport 共用)。
pub fn parse_action(raw: &str) -> Result<(String, Value, Option<Value>), String> {
    let ra: RawAction = serde_json::from_str(raw).map_err(|e| format!("动作 JSON 解析失败: {e}"))?;
    Ok((ra.action, ra.params, ra.echo))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSink {
        ready: bool,
        last: Option<SendPayload>,
    }
    impl SendSink for FakeSink {
        fn send(&mut self, p: &SendPayload) -> Result<bool, String> {
            if !self.ready {
                return Ok(false);
            }
            self.last = Some(p.clone());
            Ok(true)
        }
    }

    struct FakeStore;
    impl StoreReader for FakeStore {
        fn get_msg(&self, mid: i64) -> Option<(u64, u64, String)> {
            if mid == 42 {
                Some((398263, 1791621805, "N3R21-DONE".into()))
            } else {
                None
            }
        }
        fn self_id(&self) -> u64 {
            1694717255
        }
    }

    #[test]
    fn send_group_msg_async_and_payload() {
        let mut sink = FakeSink { ready: true, last: None };
        let raw = r#"{"action":"send_group_msg","params":{"group_id":263402786,"message":"N3R22-OB"},"echo":"e1"}"#;
        let (action, params, echo) = parse_action(raw).unwrap();
        let r = handle_action(&action, &params, echo, &mut sink, &FakeStore);
        assert_eq!(r.status, "async", "受理 ≠ 送达:async 不伪造 ok");
        assert_eq!(r.echo, Some(serde_json::json!("e1")), "echo 原样返回");
        let p = sink.last.unwrap();
        assert_eq!(p.group, Some(263402786));
        assert_eq!(p.text, "N3R22-OB");
    }

    #[test]
    fn segment_array_and_unsupported_segment_rejected() {
        let mut sink = FakeSink { ready: true, last: None };
        let (a, p, e) = parse_action(
            r#"{"action":"send_group_msg","params":{"group_id":1,"message":[{"type":"text","data":{"text":"你好"}},{"type":"image","data":{}}]}}"#,
        )
        .unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.retcode, RET_BAD_PARAMS, "image 段显式拒绝(N6 前)");
        assert!(r.wording.as_deref().unwrap_or("").contains("image"));
    }

    #[test]
    fn get_msg_from_store_and_missing() {
        let mut sink = FakeSink { ready: true, last: None };
        let (a, p, e) = parse_action(r#"{"action":"get_msg","params":{"message_id":42}}"#).unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.status, "ok");
        assert_eq!(r.data.as_ref().unwrap()["message_seq"], 398263);
        assert_eq!(r.data.as_ref().unwrap()["raw_message"], "N3R21-DONE");
        let (a, p, e) = parse_action(r#"{"action":"get_msg","params":{"message_id":1}}"#).unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.retcode, RET_BAD_REQUEST);
    }

    #[test]
    fn unsupported_and_bad_params_and_not_ready() {
        let mut sink = FakeSink { ready: true, last: None };
        let (a, p, e) = parse_action(r#"{"action":"set_group_ban","params":{}}"#).unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.retcode, RET_UNSUPPORTED, "未支持动作显式失败");
        let (a, p, e) = parse_action(r#"{"action":"send_group_msg","params":{"message":"x"}}"#).unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.retcode, RET_BAD_PARAMS, "缺 group_id");
        // 会话未就绪:Ok(false) → 不伪造受理。
        let mut sink = FakeSink { ready: false, last: None };
        let (a, p, e) = parse_action(r#"{"action":"send_group_msg","params":{"group_id":1,"message":"x"}}"#).unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.status, "failed");
        assert_eq!(r.retcode, 1500);
    }

    #[test]
    fn login_and_version_shapes() {
        let mut sink = FakeSink { ready: true, last: None };
        let (a, p, e) = parse_action(r#"{"action":"get_login_info"}"#).unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.data.as_ref().unwrap()["user_id"], 1694717255);
        let (a, p, e) = parse_action(r#"{"action":"get_version_info"}"#).unwrap();
        let r = handle_action(&a, &p, e, &mut sink, &FakeStore);
        assert_eq!(r.data.as_ref().unwrap()["protocol_version"], "v11");
    }
}
