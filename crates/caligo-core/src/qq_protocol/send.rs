//! MessageSvc.PbSendMsg 编码(好友私聊 / 普通群文本,最小集)。
//!
//! 字段号来自公开协议参考的一致描述,**未对 9.9.33 真实抓包验证**
//! (`FIELD_*_VERIFIED=false`);未知扩展一概不编——编码只产生已知字段。

use super::wire::Writer;
use super::{CodecError, MAX_MESSAGE_BYTES};

/// 公开参考的字段号(待真实抓包核验)。
pub const FIELD_ROUTING_HEAD: u32 = 1;
pub const FIELD_CONTENT_HEAD: u32 = 2;
pub const FIELD_BODY: u32 = 3;
pub const FIELD_CLIENT_SEQ: u32 = 4;
pub const FIELD_RANDOM: u32 = 5;

pub const FIELD_ROUTING_GROUP: u32 = 1; // routing.group.group_code
pub const FIELD_ROUTING_FRIEND: u32 = 1; // routing.friend.uin
pub const FIELD_CONTENT_TYPE: u32 = 1; // content.type: 1=friend 文本, 82=群文本
pub const FIELD_BODY_RICH: u32 = 1; // body.rich_text
pub const FIELD_RICH_ELEMS: u32 = 2; // rich_text.elems
pub const FIELD_ELEM_TEXT: u32 = 1; // elem.text
pub const FIELD_TEXT_STR: u32 = 1; // text.str

pub const CONTENT_TYPE_FRIEND_TEXT: u64 = 1;
pub const CONTENT_TYPE_GROUP_TEXT: u64 = 82;

pub const FIELD_NUMBERS_VERIFIED: bool = false;

/// 目标会话(编入 routing head)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendTarget {
    /// 普通群:群号(u64 群码)。
    Group { group_code: u64 },
    /// 好友私聊:对端 UIN。
    Friend { uin: u64 },
}

/// 编码一条文本发送请求(纯函数;不触网络)。
pub fn encode_pb_send_msg(
    target: &SendTarget,
    text: &str,
    client_seq: u32,
    random: u32,
) -> Result<Vec<u8>, CodecError> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge { limit: MAX_MESSAGE_BYTES });
    }
    let mut w = Writer::new();
    w.nested(FIELD_ROUTING_HEAD, |r| match target {
        SendTarget::Group { group_code } => r.varint_field(FIELD_ROUTING_GROUP, *group_code),
        SendTarget::Friend { uin } => r.varint_field(FIELD_ROUTING_FRIEND, *uin),
    });
    let content_type = match target {
        SendTarget::Group { .. } => CONTENT_TYPE_GROUP_TEXT,
        SendTarget::Friend { .. } => CONTENT_TYPE_FRIEND_TEXT,
    };
    w.nested(FIELD_CONTENT_HEAD, |c| {
        c.u32_field(FIELD_CONTENT_TYPE, content_type as u32);
    });
    w.nested(FIELD_BODY, |b| {
        b.nested(FIELD_BODY_RICH, |rich| {
            rich.nested(FIELD_RICH_ELEMS, |elem| {
                elem.nested(FIELD_ELEM_TEXT, |t| {
                    t.string_field(FIELD_TEXT_STR, text);
                });
            });
        });
    });
    w.u32_field(FIELD_CLIENT_SEQ, client_seq);
    w.u32_field(FIELD_RANDOM, random);
    Ok(w.finish())
}

#[cfg(test)]
mod tests {
    use super::super::wire::{Reader, WT_LEN, WT_VARINT};
    use super::*;

    /// 自生成 fixture 的 roundtrip:字段位置/类型/文本完整(规格未验证前的
    /// 最小回归;真实抓包到位后以 captured fixture 替换)。
    #[test]
    fn roundtrip_group_and_friend() {
        for (target, want_type) in [
            (SendTarget::Group { group_code: 123456789 }, CONTENT_TYPE_GROUP_TEXT),
            (SendTarget::Friend { uin: 10001 }, CONTENT_TYPE_FRIEND_TEXT),
        ] {
            let buf = encode_pb_send_msg(&target, "CALIGO-P5-文本🈯", 7, 9).unwrap();
            let mut r = Reader::new(&buf).unwrap();
            let mut saw = std::collections::HashSet::new();
            let mut got_type = 0u64;
            let mut got_text = String::new();
            while !r.at_end() {
                let t = r.tag().unwrap();
                saw.insert((t.field, t.wire));
                match (t.field, t.wire) {
                    (FIELD_ROUTING_HEAD, WT_LEN) => {
                        let mut sub = r.enter().unwrap();
                        let st = sub.tag().unwrap();
                        assert_eq!(st.field, FIELD_ROUTING_GROUP);
                        let _v = sub.varint().unwrap();
                    }
                    (FIELD_CONTENT_HEAD, WT_LEN) => {
                        let mut sub = r.enter().unwrap();
                        let st = sub.tag().unwrap();
                        assert_eq!(st.field, FIELD_CONTENT_TYPE);
                        got_type = sub.varint().unwrap();
                    }
                    (FIELD_BODY, WT_LEN) => {
                        let mut b = r.enter().unwrap();
                        let bt = b.tag().unwrap();
                        assert_eq!(bt.field, FIELD_BODY_RICH);
                        let mut rich = b.enter().unwrap();
                        let rt = rich.tag().unwrap();
                        assert_eq!(rt.field, FIELD_RICH_ELEMS);
                        let mut elem = rich.enter().unwrap();
                        let et = elem.tag().unwrap();
                        assert_eq!(et.field, FIELD_ELEM_TEXT);
                        let mut txt = elem.enter().unwrap();
                        let tt = txt.tag().unwrap();
                        assert_eq!(tt.field, FIELD_TEXT_STR);
                        got_text = String::from_utf8(txt.bytes_value().unwrap().to_vec()).unwrap();
                    }
                    (FIELD_CLIENT_SEQ, WT_VARINT) => {
                        assert_eq!(r.varint().unwrap(), 7);
                    }
                    (FIELD_RANDOM, WT_VARINT) => {
                        assert_eq!(r.varint().unwrap(), 9);
                    }
                    _ => r.skip(t).unwrap(),
                }
            }
            assert_eq!(got_type, want_type);
            assert_eq!(got_text, "CALIGO-P5-文本🈯", "完整 UTF-8 正文");
            assert!(saw.contains(&(FIELD_CLIENT_SEQ, WT_VARINT)));
        }
    }

    /// 超长正文拒绝(有界)。
    #[test]
    fn oversize_text_rejected() {
        let big = "x".repeat(super::MAX_MESSAGE_BYTES + 1);
        assert!(matches!(
            encode_pb_send_msg(&SendTarget::Friend { uin: 1 }, &big, 0, 0),
            Err(CodecError::TooLarge { .. })
        ));
    }
}
