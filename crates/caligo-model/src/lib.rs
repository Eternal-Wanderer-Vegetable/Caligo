//! Caligo 数据模型:账号、会话、方向与会话代次。
//!
//! 首轮约束(计划 §6):不承载 QQ 对象或裸指针,不绑定 OneBot。
//! 原生消息标识的实际字段组合(如 seq/random/内部 UID)必须等 K2/K3 现场调查
//! 确认,本 crate 只提供不透明占位,禁止在调查前编造。

use std::fmt;

/// QQ 账号 ID。
///
/// 以字符串保存:不对号码取值范围、前导零或数值上限做未经证实的假设。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(pub String);

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 会话种类。同数字的群号与好友号必须可区分(验收反例 T03)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionKind {
    /// 好友私聊。
    Private,
    /// 普通群聊。
    Group,
}

impl fmt::Display for SessionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionKind::Private => f.write_str("private"),
            SessionKind::Group => f.write_str("group"),
        }
    }
}

/// 会话对端 ID(好友号或群号)。以字符串保存,理由同 [`AccountId`]。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Peer(pub String);

/// 会话键:账号 + 种类 + 对端。
///
/// 计划 §6.3:同数字群号和好友号不能混淆;账号 ID 独立于连接代次。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub account: AccountId,
    pub kind: SessionKind,
    pub peer: Peer,
}

impl SessionKey {
    pub fn new(account: impl Into<String>, kind: SessionKind, peer: impl Into<String>) -> Self {
        Self {
            account: AccountId(account.into()),
            kind,
            peer: Peer(peer.into()),
        }
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 种类前缀显式进入键的表示,防同数字 peer 在日志与存储层混淆。
        write!(f, "{}:{}:{}", self.kind, self.account, self.peer.0)
    }
}

/// 会话代次:QQ 退出、重登或会话失效后递增,用于使旧回调与旧请求失效(计划 §6.3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(pub u64);

/// 消息方向。
///
/// 计划 §6.3:本人发送不伪装成 incoming 用户消息;方向由原生来源字段判定,
/// 不能按正文是否与自己发送过的文本相同来推断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// 他人发来的消息。
    Incoming,
    /// 本账号(本人)发出的消息,含手动发送与程序发送回执。
    SelfSent,
}

/// 原生消息引用:不透明占位。
///
/// K1 阶段唯一确认有效的字段是会话键。原生标识组合待 K2/K3 调查;在此之前
/// 不提供任何可用于去重、关联或排序的字段,防止上游误用。
/// `#[non_exhaustive]`:未来补入原生标识字段时,外部构造与匹配不破坏编译。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeMessageRef {
    pub session: SessionKey,
}

impl NativeMessageRef {
    /// 占位构造。K2/K3 调查确认实际原生标识后,此构造将被具体字段替换。
    pub fn placeholder(session: SessionKey) -> Self {
        Self { session }
    }
}

/// 自有文本消息(完整 UTF-8)。媒体与其他段类型按计划属于后续条目。
#[derive(Debug, Clone)]
pub struct TextMessage {
    pub session: SessionKey,
    pub direction: Direction,
    pub body: String,
    pub native_ref: NativeMessageRef,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_numeric_peer_private_and_group_are_distinct_sessions() {
        // T03 同数字 private/group peer:两个不同会话。
        let private = SessionKey::new("10001", SessionKind::Private, "123456");
        let group = SessionKey::new("10001", SessionKind::Group, "123456");
        assert_ne!(private, group);
        assert_ne!(private.to_string(), group.to_string());
        assert!(private.to_string().starts_with("private:"));
        assert!(group.to_string().starts_with("group:"));
    }

    #[test]
    fn account_is_part_of_the_session_key() {
        let a = SessionKey::new("10001", SessionKind::Private, "123456");
        let b = SessionKey::new("10002", SessionKind::Private, "123456");
        assert_ne!(a, b);
    }

    #[test]
    fn native_message_ref_is_placeholder_only() {
        // K1:不提供 seq/random/UID 组合;构造只接受会话键。
        let key = SessionKey::new("10001", SessionKind::Private, "123456");
        let r1 = NativeMessageRef::placeholder(key.clone());
        let r2 = NativeMessageRef::placeholder(key);
        assert_eq!(r1, r2);
    }
}
