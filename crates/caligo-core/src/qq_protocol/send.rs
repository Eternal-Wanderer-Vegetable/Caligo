//! MessageSvc.PbSendMsg 编码(好友私聊 / 普通群文本,最小集)。
//!
//! N1 起:发送侧形状与字段号 = **field-proven**(r9 私聊 / r18 群聊,
//! 实例 24316/27624 送达+渲染);编码本体已迁至共享 codec
//! `caligo_model::qq_wire::send`(bridge/core 共用,golden fixtures 见
//! caligo-model tests/qq_wire_golden.rs)。本模块保留 core 侧类型与错误
//! 映射,是共享 codec 的薄封装。
//!
//! 与旧公开参考形状的差异(以现场实证为准):
//! - 私聊路由主键是 **uid**(C2C{2}),不是 varint uin;
//! - 群请求**不带 ContentHead**(旧形状总带);
//! - 富文本含元素37 + 键6 时间戳(旧形状无)。

use super::{CodecError, MAX_MESSAGE_BYTES};
use caligo_model::qq_wire::send as wire;

/// 字段号已由 r9/r18 现场实证(细节与常量见 caligo_model::qq_wire::send)。
pub const FIELD_NUMBERS_VERIFIED: bool = true;

/// 目标会话(编入 routing head)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendTarget {
    /// 普通群:群号(u64 群码;wire 为 varint,非字符串)。
    Group { group_code: u64 },
    /// 好友私聊:对端 UIN + peerUid。
    /// `uid` 为 None 时退回 varint uin 路由:可送达但对端渲染
    /// "不支持查看"(r6 实证)——生产路径必须提供 uid。
    Friend { uin: u64, uid: Option<String> },
}

/// 编码一条文本发送请求(纯函数;不触网络;时间显式入参)。
///
/// 形状 = SendMsgReq{1: RH, [2: CH(仅私聊)], 3: Body{1: Rich{2: [元素37, 文本]}},
/// 4: seq, 5: rand, 6: {1: ts}}。
pub fn encode_pb_send_msg(
    target: &SendTarget,
    text: &str,
    client_seq: u32,
    random: u32,
    platform_ts: u64,
) -> Result<Vec<u8>, CodecError> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge { limit: MAX_MESSAGE_BYTES });
    }
    let out = match target {
        SendTarget::Group { group_code } => {
            wire::encode_group_text(*group_code, text, client_seq, random, platform_ts)
        }
        SendTarget::Friend { uin, uid } => {
            wire::encode_friend_text(*uin, uid.as_deref(), text, client_seq, random, platform_ts)
        }
    };
    out.map_err(|e| match e {
        wire::EncodeError::TooLarge { limit } => CodecError::TooLarge { limit },
    })
}

#[cfg(test)]
mod tests {
    use super::super::wire::{Reader, WT_LEN, WT_VARINT};
    use super::*;

    /// 已证形状合同(r9/r18):私聊 = RH{1: C2C{2: uid}} + CH 三键 + 元素37;
    /// 群 = RH{2: Grp{1: varint}},**无 CH**。golden 逐字节在
    /// caligo-model tests/qq_wire_golden.rs;此处验证 core 映射层不变形。
    #[test]
    fn proven_shape_group_and_friend() {
        // 群:顶层 {1,3,4,5,6},无字段 2;路由 RH{2: Grp{1: varint code}}。
        let g = encode_pb_send_msg(
            &SendTarget::Group { group_code: 70_000_001 },
            "CALIGO-P5-文本🈯",
            7,
            9,
            1_760_000_000,
        )
        .unwrap();
        let mut r = Reader::new(&g).unwrap();
        let mut top = std::collections::HashSet::new();
        let mut grp_code = 0u64;
        let mut got_text = String::new();
        while !r.at_end() {
            let t = r.tag().unwrap();
            top.insert((t.field, t.wire));
            match (t.field, t.wire) {
                (1, WT_LEN) => {
                    let mut rh = r.enter().unwrap();
                    let rt = rh.tag().unwrap();
                    assert_eq!(rt.field, 2, "群路由 = RH.2 Grp");
                    let mut grp = rh.enter().unwrap();
                    let gt = grp.tag().unwrap();
                    assert_eq!(gt.field, 1);
                    grp_code = grp.varint().unwrap();
                }
                (3, WT_LEN) => {
                    let mut b = r.enter().unwrap();
                    let bt = b.tag().unwrap();
                    assert_eq!(bt.field, 1, "body = {{1: rich}}(r8 修正)");
                    let mut rich = b.enter().unwrap();
                    let mut saw_elem37 = false;
                    let mut first = true;
                    while !rich.at_end() {
                        let et = rich.tag().unwrap();
                        assert_eq!(et.field, 2, "rich.elems");
                        if first {
                            // 第一元素 = 元素37 群版(118B,aa 02 73…)。
                            let raw = rich.bytes_value().unwrap();
                            saw_elem37 = raw.starts_with(&[0xaa, 0x02, 0x73]);
                            first = false;
                            continue;
                        }
                        let mut elem = rich.enter().unwrap();
                        let et2 = elem.tag().unwrap();
                        assert_eq!(et2.field, 1);
                        let mut txt = elem.enter().unwrap();
                        let tt = txt.tag().unwrap();
                        assert_eq!(tt.field, 1);
                        got_text = String::from_utf8(txt.bytes_value().unwrap().to_vec()).unwrap();
                    }
                    assert!(saw_elem37, "元素37 缺失 → 对端不渲染");
                }
                (4, WT_VARINT) => assert_eq!(r.varint().unwrap(), 7),
                (5, WT_VARINT) => assert_eq!(r.varint().unwrap(), 9),
                (6, WT_LEN) => {
                    let mut f6 = r.enter().unwrap();
                    let ft = f6.tag().unwrap();
                    assert_eq!(ft.field, 1);
                    assert_eq!(f6.varint().unwrap(), 1_760_000_000);
                }
                _ => r.skip(t).unwrap(),
            }
        }
        assert_eq!(grp_code, 70_000_001);
        assert_eq!(got_text, "CALIGO-P5-文本🈯", "完整 UTF-8 正文");
        assert!(!top.contains(&(2, WT_LEN)), "群请求不带 ContentHead(r17/r18 实证)");
    }

    #[test]
    fn proven_shape_friend_routes_by_uid() {
        let f = encode_pb_send_msg(
            &SendTarget::Friend { uin: 10001, uid: Some("u_GOLDENFIXpeer00000000A".into()) },
            "hi",
            1,
            2,
            1_760_000_000,
        )
        .unwrap();
        let mut r = Reader::new(&f).unwrap();
        let t = r.tag().unwrap();
        assert_eq!((t.field, t.wire), (1, WT_LEN));
        let mut rh = r.enter().unwrap();
        let rt = rh.tag().unwrap();
        assert_eq!(rt.field, 1, "私聊路由 = RH.1 C2C");
        let mut c2c = rh.enter().unwrap();
        let ct = c2c.tag().unwrap();
        assert_eq!(ct.field, 2, "C2C{{2: uid}}(r6:非 varint uin)");
        assert_eq!(c2c.bytes_value().unwrap(), b"u_GOLDENFIXpeer00000000A");
        // CH 存在且为三键 {1:1, 2:0, 3:0}。
        let t2 = r.tag().unwrap();
        assert_eq!((t2.field, t2.wire), (2, WT_LEN));
        let mut ch = r.enter().unwrap();
        let mut keys = Vec::new();
        while !ch.at_end() {
            let k = ch.tag().unwrap();
            keys.push((k.field, ch.varint().unwrap()));
        }
        assert_eq!(keys, vec![(1, 1), (2, 0), (3, 0)]);
    }

    /// uid 缺席的降级路径可编码(可送达但不渲染;r6),类型保持 uin 路由。
    #[test]
    fn friend_without_uid_falls_back_to_uin_route() {
        let f = encode_pb_send_msg(
            &SendTarget::Friend { uin: 10001, uid: None },
            "hi",
            1,
            2,
            1_760_000_000,
        )
        .unwrap();
        let mut r = Reader::new(&f).unwrap();
        let t = r.tag().unwrap();
        assert_eq!((t.field, t.wire), (1, WT_LEN));
        let mut rh = r.enter().unwrap();
        let rt = rh.tag().unwrap();
        assert_eq!((rt.field, rt.wire), (1, WT_VARINT), "无 uid → varint uin");
        assert_eq!(rh.varint().unwrap(), 10001);
    }

    /// 超长正文拒绝(有界)。
    #[test]
    fn oversize_text_rejected() {
        let big = "x".repeat(super::MAX_MESSAGE_BYTES + 1);
        assert!(matches!(
            encode_pb_send_msg(
                &SendTarget::Friend { uin: 1, uid: None },
                &big,
                0,
                0,
                1_760_000_000
            ),
            Err(CodecError::TooLarge { .. })
        ));
    }
}
