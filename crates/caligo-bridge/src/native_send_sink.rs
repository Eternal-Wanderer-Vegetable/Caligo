//! 会话发送适配器(N4):OneBot `SendSink` → `qq_wire::send`(已证形状)
//! → NativeSession enqueue_send。
//!
//! cookie 生成 = 每消息新鲜 16B。客户端序号先由 NativeSession 的持久
//! SeqSource 分配，再以同一值编码和登记。私聊 UIN→UID 未证实，明确拒绝。

use crate::native_response::Cookie;
use crate::native_session::{NativeSession, SendAcceptance, SeqSource};
use caligo_model::onebot_traits::{SendDisposition, SendPayload, SendTarget};

/// 发送适配器(分别借用会话状态与原生 ops;生命周期由调用方约束)。
pub struct SessionSendSink<'a, S: SeqSource, O> {
    pub session: &'a mut NativeSession<S>,
    pub ops: &'a mut O,
    pub wrapper_base: u64,
    request_counter: u64,
}

impl<'a, S, O> SessionSendSink<'a, S, O>
where
    S: crate::native_session::SeqSource,
    O: crate::native_session::NativeOps,
{
    pub fn new(session: &'a mut NativeSession<S>, ops: &'a mut O, wrapper_base: u64) -> Self {
        Self {
            session,
            ops,
            wrapper_base,
            request_counter: 0,
        }
    }

    fn next_request_id(&mut self) -> String {
        self.request_counter += 1;
        format!("ob-{}", self.request_counter)
    }

    /// 每消息新鲜 16B cookie(rdtsc 派生;零值会被 register 拒绝并如实上抛)。
    fn next_cookie(&mut self) -> Cookie {
        let mut c = [0u8; 16];
        for (i, chunk) in c.chunks_mut(8).enumerate() {
            let mut t = unsafe { core::arch::x86_64::_rdtsc() } ^ ((i as u64) << 17);
            t ^= t >> 31;
            chunk.copy_from_slice(&t.to_le_bytes()[..chunk.len()]);
        }
        Cookie(c)
    }

    /// 编码 + 派发;返回受理分类。
    pub fn dispatch(&mut self, payload: &SendPayload) -> Result<SendAcceptance, String> {
        let group_id = match payload.target {
            SendTarget::Group { group_id } => group_id,
            SendTarget::Private { user_id } => {
                return Err(format!(
                    "私聊发送暂不支持: user_id={user_id} 尚未解析为 peerUid"
                ))
            }
        };
        let rid = self.next_request_id();
        let cookie = self.next_cookie();
        let text = payload.text.clone();
        Ok(self
            .session
            .enqueue_encoded_send(self.ops, &rid, cookie, move |wire_seq| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                caligo_model::qq_wire::send::encode_group_text(
                    group_id,
                    &text,
                    wire_seq,
                    caligo_model::qq_wire::send::fresh_random(),
                    ts,
                )
                .map_err(|e| format!("编码失败: {e}"))
            }))
    }
}

impl<'a, S, O> caligo_model::onebot_traits::SendSink for SessionSendSink<'a, S, O>
where
    S: crate::native_session::SeqSource,
    O: crate::native_session::NativeOps,
{
    fn send(&mut self, payload: &SendPayload) -> Result<SendDisposition, String> {
        match self.dispatch(payload)? {
            SendAcceptance::Dispatched { .. } => Ok(SendDisposition::AcceptedUnconfirmed),
            SendAcceptance::NotAttached => Err("会话未就绪".into()),
            SendAcceptance::RefusedPreCall { reason }
            | SendAcceptance::AllocationFailed { reason }
            | SendAcceptance::NotReady { reason } => Err(reason),
        }
    }
}
