//! SendMsgReq 普通文本发送编码(field-proven;r9 私聊 / r18 群聊)。
//!
//! 顶层形状(与 caligo-bridge g3 研究编码同源,字节级实证):
//! - 私聊:`{1: RoutingHead{1: C2C{2: uid}}, 2: ContentHead{1:1,2:0,3:0},
//!   3: Body{1: Rich{2: [元素37(26B), 元素文本]}}, 4: seq, 5: rand, 6: {1: ts}}`
//!   —— 路由主键是 **uid**(u_ 串);仅 uin varint 路由可送达但渲染
//!   "不支持查看"(r6 修正)。
//! - 群聊:`{1: RoutingHead{2: Grp{1: varint 群号}}, 3: Body{…同构},
//!   4/5/6 同}` —— **不带 ContentHead**(r17 排除)、群号是 **varint u64
//!   而非字符串**(r18 补齐;两条件缺一不可,r10–r17 十轮全拒)。
//! - 元素37 缺失 → 对端渲染"该消息类型暂不支持查看"(r7 补入后正常)。
//!
//! 群版元素37 = 118B 静态常量(r18 实测可通过;真实为动态生成,
//! 见模块注释 —— 支持域限定当前版本,版本漂移按 N5 重验)。

use super::{pb_len_field, pb_varint_field};

/// 与 caligo-core qq_protocol::send 同名常量同值;本模块为字段号权威源。
pub const PB_F_ROUTING_HEAD: u32 = 1;
pub const PB_F_CONTENT_HEAD: u32 = 2;
pub const PB_F_BODY: u32 = 3;
pub const PB_F_CLIENT_SEQ: u32 = 4;
pub const PB_F_RANDOM: u32 = 5;
/// 键6 = {1: 平台秒}(真实报文实证;缺失不影响送达,补齐为渲染一致项)。
pub const PB_F_SEND_TIME: u32 = 6;
pub const PB_F_FRIEND_UIN: u32 = 1;
pub const PB_F_FRIEND_UID: u32 = 2;
pub const PB_F_CONTENT_TYPE: u32 = 1;
pub const PB_F_BODY_RICH: u32 = 1;
pub const PB_F_RICH_ELEMS: u32 = 2;
pub const PB_F_ELEM_TEXT: u32 = 1;
pub const PB_F_TEXT_STR: u32 = 1;
pub const PB_CONTENT_TYPE_FRIEND_TEXT: u64 = 1;

/// 正文上限(有界;编码拒绝而非截断)。
pub const MAX_TEXT_BYTES: usize = 256 * 1024;

/// 编码错误(最小集;规格扩展在 N6+ 按能力追加)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    TooLarge { limit: usize },
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::TooLarge { limit } => write!(f, "text exceeds {limit} bytes"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// 元素37(私聊/通用标志,26B):自真实 UI 发送报文字节级照抄
/// (capture-main-send-body.jsonl,2026-10-09 主账号实例内存取证)。
pub const FRIEND_ELEM37: &[u8] = &[
    0xaa, 0x02, 0x17, 0x88, 0x01, 0x00, 0x9a, 0x01, 0x11, 0x78, 0x00, 0xc8, 0x01, 0x00, 0xf0,
    0x01, 0x00, 0xf8, 0x01, 0x00, 0x90, 0x02, 0x00, 0x80, 0x06, 0x00,
];

/// 元素37(群版,118B):自真实存储记录字节级照抄
/// (capture-main-group-body.jsonl addr=0x24b715d5f30;r16 修复版)。
pub const GROUP_ELEM37: &[u8] = &[
    0xaa, 0x02, 0x73, 0x50, 0x00, 0x60, 0x00, 0x68, 0x00, 0x88, 0x01, 0x00, 0x9a, 0x01, 0x67,
    0x08, 0x06, 0x20, 0xcb, 0x50, 0x78, 0x00, 0xc8, 0x01, 0x00, 0xf0, 0x01, 0x00, 0xf8, 0x01,
    0x00, 0x90, 0x02, 0x00, 0x98, 0x03, 0x00, 0xa0, 0x03, 0x00, 0xb0, 0x03, 0x00, 0xc0, 0x03,
    0x00, 0xd0, 0x03, 0x00, 0xe8, 0x03, 0x00, 0x8a, 0x04, 0x04, 0x08, 0x02, 0x10, 0x2d, 0x90,
    0x04, 0x80, 0x01, 0xb8, 0x04, 0x00, 0xc0, 0x04, 0x01, 0xca, 0x04, 0x02, 0x10, 0x00, 0xf8,
    0x04, 0xcc, 0x84, 0x08, 0x80, 0x05, 0x3a, 0x88, 0x05, 0x00, 0x80, 0x06, 0x00, 0xb2, 0x06,
    0x0a, 0x0a, 0x08, 0x08, 0x00, 0x10, 0x00, 0x18, 0x00, 0x20, 0x00, 0xd8, 0x06, 0xef, 0x8f,
    0x18, 0xa0, 0x07, 0xc3, 0x8c, 0xa2, 0x8c, 0xae, 0xee, 0xe2, 0xfd, 0x89, 0x01,
];

/// 好友私聊普通文本(field-proven r9 形状)。
///
/// `peer_uid` 存在时路由 = C2C{2: uid}(正确渲染路径);为 None 时退回
/// varint uin 路由 —— 可送达但**对端渲染"不支持查看"**(r6 实证),
/// 生产路径必须提供 uid。
pub fn encode_friend_text(
    peer_uin: u64,
    peer_uid: Option<&str>,
    text: &str,
    client_seq: u32,
    random: u32,
    platform_ts: u64,
) -> Result<Vec<u8>, EncodeError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(EncodeError::TooLarge { limit: MAX_TEXT_BYTES });
    }
    let mut w = Vec::with_capacity(text.len() + 128);
    // RoutingHead{1: C2C{2: uid}};13665a0 实证:无 uin 字段
    // (C2C.1 若为 string 类型,varint 造成 wire 不匹配 → 整包解析失败)。
    let mut routing = Vec::new();
    if let Some(uid) = peer_uid {
        let mut c2c = Vec::new();
        pb_len_field(&mut c2c, PB_F_FRIEND_UID, uid.as_bytes());
        pb_len_field(&mut routing, PB_F_FRIEND_UIN, &c2c);
    } else {
        pb_varint_field(&mut routing, PB_F_FRIEND_UIN, peer_uin);
    }
    pb_len_field(&mut w, PB_F_ROUTING_HEAD, &routing);
    // ContentHead = {1: type, 2: 0, 3: 0}(1360ebc 实证:嵌套推三键;r5 现场过验)。
    let mut content = Vec::new();
    pb_varint_field(&mut content, PB_F_CONTENT_TYPE, PB_CONTENT_TYPE_FRIEND_TEXT);
    pb_varint_field(&mut content, 2, 0);
    pb_varint_field(&mut content, 3, 0);
    pb_len_field(&mut w, PB_F_CONTENT_HEAD, &content);
    push_body_tail(&mut w, FRIEND_ELEM37, text, client_seq, random, platform_ts);
    Ok(w)
}

/// 普通群文本(field-proven r18 形状;`group_code` 为 0 无意义,调用方保证)。
pub fn encode_group_text(
    group_code: u64,
    text: &str,
    client_seq: u32,
    random: u32,
    platform_ts: u64,
) -> Result<Vec<u8>, EncodeError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(EncodeError::TooLarge { limit: MAX_TEXT_BYTES });
    }
    let mut w = Vec::with_capacity(text.len() + 128);
    // RoutingHead{2: Grp{1: varint groupCode}}:13665a0 chat_type==2 分支
    // 字节级语义 = peer 串经 1e9f8b 转 u64 → Grp.field1 varint(非字符串)。
    let mut routing = Vec::new();
    let mut grp = Vec::new();
    pb_varint_field(&mut grp, 1, group_code);
    pb_len_field(&mut routing, 2, &grp);
    pb_len_field(&mut w, PB_F_ROUTING_HEAD, &routing);
    // 群请求不带 ContentHead(r17 排除;r10–r16 十轮全带全拒)。
    push_body_tail(&mut w, GROUP_ELEM37, text, client_seq, random, platform_ts);
    Ok(w)
}

/// Body{1: Rich{2: [元素37, 元素文本]}} + 键4(seq) + 键5(rand) + 键6 {1: ts}。
///
/// body 一层包装(r8 定位:rich 直接放 Body 下对端找不到富文本)。
fn push_body_tail(
    w: &mut Vec<u8>,
    elem37: &[u8],
    text: &str,
    client_seq: u32,
    random: u32,
    platform_ts: u64,
) {
    let mut rich = Vec::new();
    super::pb_len_field(&mut rich, PB_F_RICH_ELEMS, elem37);
    let mut elem = Vec::new();
    let mut txt = Vec::new();
    pb_len_field(&mut txt, PB_F_TEXT_STR, text.as_bytes());
    pb_len_field(&mut elem, PB_F_ELEM_TEXT, &txt);
    super::pb_len_field(&mut rich, PB_F_RICH_ELEMS, &elem);
    let mut body_nested = Vec::new();
    pb_len_field(&mut body_nested, PB_F_BODY_RICH, &rich);
    pb_len_field(w, PB_F_BODY, &body_nested);
    pb_varint_field(w, PB_F_CLIENT_SEQ, client_seq as u64);
    pb_varint_field(w, PB_F_RANDOM, random as u64);
    let mut f6 = Vec::new();
    pb_varint_field(&mut f6, 1, platform_ts);
    pb_len_field(w, PB_F_SEND_TIME, &f6);
}
