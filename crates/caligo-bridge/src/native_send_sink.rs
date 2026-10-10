//! 会话发送适配器(N4):OneBot `SendSink` → `qq_wire::send`(已证形状)
//! → NativeSession enqueue_send。
//!
//! cookie 生成 = 每消息新鲜 16B(rdtsc 派生,与 g3 研究路径同源;
//! 服务器不校验来源,r19 实证)。client_seq 传 0(服务器不校验会话 seq,
//! r18 实证 seq=18 vs 群真实 ~4700 仍 PASS);响应 seq 走 0x9c43 记录键。
//! 私聊需要 uin→uid 解析(0x9c55 族),N3 接线后开放 —— 当前显式拒绝不猜。

use crate::native_response::Cookie;
use crate::native_session::{NativeSession, SendAcceptance, SeqSource};
use caligo_model::onebot_traits::SendPayload;

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
        Self { session, ops, wrapper_base, request_counter: 0 }
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
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let body = match payload.group {
            Some(code) => caligo_model::qq_wire::send::encode_group_text(
                code,
                &payload.text,
                0, // client_seq:服务器不校验(r18 实证);响应 seq 走 0x9c43。
                payload.text.len() as u32 | 1,
                ts,
            ),
            None => {
                // 私聊需要 peerUid(u_ 串);uin→uid 解析属 N3 接线,显式拒绝不猜。
                return Err("私聊发送需 peerUid 解析(N3 接线);当前未开放".into());
            }
        }
        .map_err(|e| format!("编码失败: {e}"))?;
        let rid = self.next_request_id();
        let cookie = self.next_cookie();
        Ok(self.session.enqueue_send(self.ops, &rid, cookie, &body))
    }
}

impl<'a, S, O> caligo_model::onebot_traits::SendSink for SessionSendSink<'a, S, O>
where
    S: crate::native_session::SeqSource,
    O: crate::native_session::NativeOps,
{
    fn send(&mut self, payload: &SendPayload) -> Result<bool, String> {
        match self.dispatch(payload)? {
            SendAcceptance::Dispatched { .. } => Ok(true),
            SendAcceptance::NotAttached => Ok(false),
            SendAcceptance::RefusedPreCall { reason }
            | SendAcceptance::AllocationFailed { reason }
            | SendAcceptance::NotReady { reason } => Err(reason),
        }
    }
}
