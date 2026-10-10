//! OneBot 动作层共用类型(N4 下沉;core 与 bridge 共用,防依赖环)。

use serde_json::Value;

/// 发送载荷(已通过参数校验;wire 编码与原生派发由实现方承担)。
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
