//! MsgPush 最小路由(T17):text/richText 的最小必要字段 + 未知保留。
//!
//! 布局按公开参考(未对 9.9.33 抓包验证):PushMsgBody{1: message},
//! message{1: routing, 2: content, 3: body};routing{1: from_uin,
//! 2: from_uid, 6? group_code(群)}, content{1: type, 4? random,
//! 5? seq, 6? time}, body{1: rich_text}, rich{2: elems},
//! elem{1: text{1: str}}。同正文不同 ID 必须保留(去重在上层按原生 ID)。

use super::identity::{classify_incoming, validate_pairing, IncomingIdentity, SessionKind};
use super::wire::{Reader, WT_LEN, WT_VARINT};
use super::CodecError;

pub const FIELD_PUSH_MESSAGE: u32 = 1;
pub const FIELD_MSG_ROUTING: u32 = 1;
pub const FIELD_MSG_CONTENT: u32 = 2;
pub const FIELD_MSG_BODY: u32 = 3;
pub const FIELD_ROUTING_FROM_UIN: u32 = 1;
pub const FIELD_CONTENT_TYPE: u32 = 1;
pub const FIELD_CONTENT_TIME: u32 = 6;
pub const FIELD_BODY_RICH: u32 = 1;
pub const FIELD_RICH_ELEMS: u32 = 2;
pub const FIELD_ELEM_TEXT: u32 = 1;
pub const FIELD_TEXT_STR: u32 = 1;

pub const FIELD_NUMBERS_VERIFIED: bool = false;

/// 内容类型(公开参考;群文本 82 / 好友文本 1)。
pub const CONTENT_TYPE_GROUP_TEXT: u64 = 82;
pub const CONTENT_TYPE_FRIEND_TEXT: u64 = 1;

/// 解析产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedIncoming {
    pub identity: IncomingIdentity,
    /// 拼接后的文本(全部 text elem;rich 其他元素保留在 unknown 计数)。
    pub text: String,
    /// 跳过的未知字段计数(T18 痕迹)。
    pub skipped_unknown: u64,
}

/// 解析一条 MsgPush(仅支持群/好友文本;其他类型按 Unsupported 拒绝,
/// 不产生半解析事件)。
pub fn parse_msg_push(
    account: &str,
    bytes: &[u8],
    group_hint: Option<u64>,
) -> Result<ParsedIncoming, CodecError> {
    let mut top = Reader::new(bytes)?;
    let mut message: Option<Reader<'_>> = None;
    while !top.at_end() {
        let t = top.tag()?;
        if (t.field, t.wire) == (FIELD_PUSH_MESSAGE, WT_LEN) {
            message = Some(top.enter()?);
            break;
        }
        top.skip(t)?;
    }
    let mut msg = message.ok_or(CodecError::MissingField("push.message"))?;
    let mut skipped = 0u64;

    let mut from_uin: u64 = 0;
    let mut content_type: u64 = 0;
    let mut platform_time: Option<u64> = None;
    let mut text = String::new();
    let mut saw_rich = false;

    while !msg.at_end() {
        let t = msg.tag()?;
        match (t.field, t.wire) {
            (FIELD_MSG_ROUTING, WT_LEN) => {
                let mut r = msg.enter()?;
                while !r.at_end() {
                    let rt = r.tag()?;
                    if (rt.field, rt.wire) == (FIELD_ROUTING_FROM_UIN, WT_VARINT) {
                        from_uin = r.varint()?;
                    } else {
                        r.skip(rt)?;
                        skipped += 1;
                    }
                }
            }
            (FIELD_MSG_CONTENT, WT_LEN) => {
                let mut c = msg.enter()?;
                while !c.at_end() {
                    let ct = c.tag()?;
                    match (ct.field, ct.wire) {
                        (FIELD_CONTENT_TYPE, WT_VARINT) => content_type = c.varint()?,
                        (FIELD_CONTENT_TIME, WT_VARINT) => platform_time = Some(c.varint()?),
                        _ => {
                            c.skip(ct)?;
                            skipped += 1;
                        }
                    }
                }
            }
            (FIELD_MSG_BODY, WT_LEN) => {
                let mut b = msg.enter()?;
                while !b.at_end() {
                    let bt = b.tag()?;
                    if (bt.field, bt.wire) == (FIELD_BODY_RICH, WT_LEN) {
                        saw_rich = true;
                        let mut rich = b.enter()?;
                        while !rich.at_end() {
                            let rt = rich.tag()?;
                            if (rt.field, rt.wire) == (FIELD_RICH_ELEMS, WT_LEN) {
                                let mut elem = rich.enter()?;
                                while !elem.at_end() {
                                    let et = elem.tag()?;
                                    if (et.field, et.wire) == (FIELD_ELEM_TEXT, WT_LEN) {
                                        let mut txt = elem.enter()?;
                                        while !txt.at_end() {
                                            let tt = txt.tag()?;
                                            if (tt.field, tt.wire) == (FIELD_TEXT_STR, WT_LEN) {
                                                text.push_str(&String::from_utf8_lossy(
                                                    txt.bytes_value()?,
                                                ));
                                            } else {
                                                txt.skip(tt)?;
                                            }
                                        }
                                    } else {
                                        elem.skip(et)?;
                                        skipped += 1;
                                    }
                                }
                            } else {
                                rich.skip(rt)?;
                                skipped += 1;
                            }
                        }
                    } else {
                        b.skip(bt)?;
                        skipped += 1;
                    }
                }
            }
            _ => {
                msg.skip(t)?;
                skipped += 1;
            }
        }
    }

    if from_uin == 0 {
        return Err(CodecError::MissingField("routing.from_uin"));
    }
    if !saw_rich {
        return Err(CodecError::MissingField("body.rich_text"));
    }
    let kind = match content_type {
        CONTENT_TYPE_GROUP_TEXT => SessionKind::Group,
        CONTENT_TYPE_FRIEND_TEXT => SessionKind::Private,
        other => {
            // 未支持类型显式拒绝(不产生半解析事件;上层可记 Gap)。
            return Err(CodecError::MissingField(match other {
                _ => "supported content type",
            }));
        }
    };
    // 群消息:peer 群号来自路由 hint(上层经 transport 上下文注入;
    // 公开参考中群号亦在 routing 内,字段未验证前由调用方提供)。
    let (peer, sender) = match kind {
        SessionKind::Group => {
            let g = group_hint.ok_or(CodecError::MissingField("group_code hint"))?;
            (g, from_uin)
        }
        SessionKind::Private => (from_uin, from_uin),
    };
    let identity = classify_incoming(account, kind, peer, sender, platform_time)?;
    validate_pairing(&identity)?;
    Ok(ParsedIncoming {
        identity,
        text,
        skipped_unknown: top.skipped_unknown + skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::super::wire::Writer;
    use super::*;

    /// 合成一条 MsgPush(自生成 fixture;真实抓包到位后替换)。
    fn synth_push(from_uin: u64, ctype: u64, time: u64, text: &str) -> Vec<u8> {
        let mut msg = Writer::new();
        msg.nested(FIELD_MSG_ROUTING, |r| {
            r.varint_field(FIELD_ROUTING_FROM_UIN, from_uin);
        });
        msg.nested(FIELD_MSG_CONTENT, |c| {
            c.u32_field(FIELD_CONTENT_TYPE, ctype as u32);
            c.u32_field(FIELD_CONTENT_TIME, time as u32);
        });
        msg.nested(FIELD_MSG_BODY, |b| {
            b.nested(FIELD_BODY_RICH, |rich| {
                rich.nested(FIELD_RICH_ELEMS, |elem| {
                    elem.nested(FIELD_ELEM_TEXT, |t| {
                        t.string_field(FIELD_TEXT_STR, text);
                    });
                });
            });
        });
        let mut top = Writer::new();
        top.bytes_field(FIELD_PUSH_MESSAGE, &msg.finish());
        top.finish()
    }

    /// T17:群文本解析出正确身份 + 完整正文(Unicode)。
    #[test]
    fn group_text_identity_and_full_text() {
        let bytes = synth_push(888, CONTENT_TYPE_GROUP_TEXT, 1700000000, "你好🈯world");
        let p = parse_msg_push("10001", &bytes, Some(777)).unwrap();
        assert_eq!(p.identity.session.kind, SessionKind::Group);
        assert_eq!(p.identity.session.peer, "777");
        assert_eq!(p.identity.sender, "888");
        assert_eq!(p.identity.platform_time, Some(1700000000));
        assert_eq!(p.text, "你好🈯world");
    }

    /// T17:同正文两条不同 from/seq 保留为两条(去重在上层按原生 ID,
    /// 本层不合并)。
    #[test]
    fn same_text_two_pushes_both_parse() {
        let a = synth_push(888, CONTENT_TYPE_GROUP_TEXT, 1, "dup");
        let b = synth_push(999, CONTENT_TYPE_GROUP_TEXT, 2, "dup");
        let pa = parse_msg_push("10001", &a, Some(777)).unwrap();
        let pb = parse_msg_push("10001", &b, Some(777)).unwrap();
        assert_eq!(pa.text, pb.text);
        assert_ne!(pa.identity.sender, pb.identity.sender, "不同发送者必须保留");
    }

    /// T18:未知字段跳过并留痕;截断拒绝。
    #[test]
    fn unknown_fields_and_truncation() {
        let mut msg = Writer::new();
        msg.varint_field(50, 1); // routing 前的未知字段
        msg.nested(FIELD_MSG_ROUTING, |r| {
            r.varint_field(FIELD_ROUTING_FROM_UIN, 888);
        });
        msg.nested(FIELD_MSG_CONTENT, |c| {
            c.u32_field(FIELD_CONTENT_TYPE, CONTENT_TYPE_GROUP_TEXT as u32);
        });
        msg.nested(FIELD_MSG_BODY, |b| {
            b.nested(FIELD_BODY_RICH, |rich| {
                rich.nested(FIELD_RICH_ELEMS, |elem| {
                    elem.nested(FIELD_ELEM_TEXT, |t| {
                        t.string_field(FIELD_TEXT_STR, "x");
                    });
                });
            });
        });
        let mut top = Writer::new();
        top.bytes_field(FIELD_PUSH_MESSAGE, &msg.finish());
        let buf = top.finish();
        let p = parse_msg_push("10001", &buf, Some(777)).unwrap();
        assert!(p.skipped_unknown >= 1, "未知字段必须留痕");
        // 截断:去尾 3 字节 → Truncated。
        assert!(matches!(
            parse_msg_push("10001", &buf[..buf.len() - 3], Some(777)),
            Err(CodecError::Truncated { .. }) | Err(CodecError::MissingField(_))
        ));
    }

    /// 未支持内容类型显式拒绝(不产半解析事件)。
    #[test]
    fn unsupported_type_rejected() {
        let bytes = synth_push(888, 999, 1, "x");
        assert!(parse_msg_push("10001", &bytes, Some(777)).is_err());
    }
}
