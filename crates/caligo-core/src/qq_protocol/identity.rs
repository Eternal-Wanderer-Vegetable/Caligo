//! 身份映射守卫(T20):UIN/UID/群号/方向/平台时间。
//!
//! 合同:映射是**显式且可失败**的——任何字段缺失/可疑都产生 Unknown
//! 或拒绝,不串身份、不把本地时间当 platform_time、不用空值默认通过。

use super::CodecError;

/// 会话键(与 caligo_model::SessionKey 语义对齐的独立 DTO;P5 内不依赖
/// model,防止编解码层被运行时状态污染)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKey {
    pub account: String,
    pub kind: SessionKind,
    pub peer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Private,
    Group,
}

/// 接收事件的解析产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingIdentity {
    pub session: SessionKey,
    /// 发送者(群内成员 UIN / 私聊对端 UIN)。
    pub sender: String,
    /// 方向:self_sent 需要发送者==登录账号的**独立证据**(见 [`classify`])。
    pub self_sent: bool,
    /// 平台时间(原始;None 必须如实为 None,禁止本地时钟补齐)。
    pub platform_time: Option<u64>,
}

/// 映射参数:登录账号用于方向判定;除此之外不做任何发明。
pub fn classify_incoming(
    account: &str,
    kind: SessionKind,
    peer_id: u64,
    sender_id: u64,
    platform_time: Option<u64>,
) -> Result<IncomingIdentity, CodecError> {
    if account.is_empty() {
        return Err(CodecError::MissingField("account"));
    }
    if peer_id == 0 {
        return Err(CodecError::MissingField("peer"));
    }
    if sender_id == 0 {
        return Err(CodecError::MissingField("sender"));
    }
    // T20:群聊的 peer 是群号、sender 是成员;私聊的 peer 与 sender 是同一
    // 主体(对端),此时方向不由 sender==account 单独决定 —— self_sent 必须
    // 来自外部的独立证据(如 content_head 的 self 标志或回执关联),此处
    // 一律 false 并显式标注,不猜。
    let self_sent = false;
    Ok(IncomingIdentity {
        session: SessionKey {
            account: account.to_string(),
            kind,
            peer: peer_id.to_string(),
        },
        sender: sender_id.to_string(),
        self_sent,
        platform_time,
    })
}

/// 群/私聊 peer 与 sender 的独立性校验(T20):群聊中 sender != peer 群号
/// (群号与成员号空间不同,相等即可疑);私聊中 sender 即对端。
pub fn validate_pairing(id: &IncomingIdentity) -> Result<(), CodecError> {
    match id.session.kind {
        SessionKind::Group => {
            if id.session.peer == id.sender {
                return Err(CodecError::MissingField("sender != group peer"));
            }
            Ok(())
        }
        SessionKind::Private => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T20:正常映射不串身份。
    #[test]
    fn mapping_keeps_identities_apart() {
        let g = classify_incoming("10001", SessionKind::Group, 777, 888, Some(1234)).unwrap();
        assert_eq!(g.session.peer, "777");
        assert_eq!(g.sender, "888");
        assert_eq!(g.platform_time, Some(1234));
        let f = classify_incoming("10001", SessionKind::Private, 888, 888, None).unwrap();
        assert_eq!(f.session.peer, "888");
        assert_eq!(f.sender, "888");
        assert!(!f.self_sent, "方向不靠 sender==account 猜");
        assert_eq!(f.platform_time, None, "取不到就如实为 None");
    }

    /// T20:群号==成员号 可疑组合拒绝。
    #[test]
    fn group_peer_equals_sender_rejected() {
        let g = classify_incoming("10001", SessionKind::Group, 777, 777, None).unwrap();
        assert!(validate_pairing(&g).is_err());
    }

    /// T20:空账号/零 peer/零 sender 拒绝(不用空值默认通过)。
    #[test]
    fn missing_fields_rejected() {
        assert!(classify_incoming("", SessionKind::Group, 1, 2, None).is_err());
        assert!(classify_incoming("10001", SessionKind::Group, 0, 2, None).is_err());
        assert!(classify_incoming("10001", SessionKind::Group, 1, 0, None).is_err());
    }
}
