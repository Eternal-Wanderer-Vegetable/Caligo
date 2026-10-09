//! QQ wire golden fixtures(N0-3/N1;计划 §7-N0 动作 3)。
//!
//! fixture 纪律:
//! - **身份全部合成**:uid = `u_GOLDENFIX…`、群号 = 700000xxx、文本 =
//!   GOLDEN-*,不含任何真实账号/群号/正文(计划:不能用真实账号当测试常量)。
//! - **结构有源**:期望字节由本文件内的独立微型构建器按已证结构拼出
//!   (与被测编码器无代码共享),逐字节比对 —— golden 不是"编码器输出
//!   自证"。
//! - **归档锚点**:r9/r18 的 encode head(证据 jsonl 的 24B 前缀)中非
//!   账号字段的结构字节在此回放(真实 uid/群号字节被排除,形状字节保留)。

use caligo_model::qq_wire::send::{
    encode_friend_text, encode_group_text, FRIEND_ELEM37, GROUP_ELEM37,
};

// ---- 独立微型 protobuf 构建器(与被测代码无共享) ----

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

fn vfield(out: &mut Vec<u8>, field: u32, v: u64) {
    varint(out, ((field as u64) << 3) as u64);
    varint(out, v);
}

fn lfield(out: &mut Vec<u8>, field: u32, body: &[u8]) {
    varint(out, ((field as u64) << 3) | 2);
    varint(out, body.len() as u64);
    out.extend_from_slice(body);
}

/// 已证私聊结构(r9):routing + CH 三键 + body{1: rich{2:[elem37, text]}} + 4/5/6。
fn expected_friend(uid: &str, text: &str, seq: u32, rand: u32, ts: u64, elem37: &[u8]) -> Vec<u8> {
    let mut w = Vec::new();
    let mut c2c = Vec::new();
    lfield(&mut c2c, 2, uid.as_bytes());
    let mut routing = Vec::new();
    lfield(&mut routing, 1, &c2c);
    lfield(&mut w, 1, &routing);
    let mut ch = Vec::new();
    vfield(&mut ch, 1, 1);
    vfield(&mut ch, 2, 0);
    vfield(&mut ch, 3, 0);
    lfield(&mut w, 2, &ch);
    let mut rich = Vec::new();
    lfield(&mut rich, 2, elem37);
    let mut txt = Vec::new();
    lfield(&mut txt, 1, text.as_bytes());
    let mut elem = Vec::new();
    lfield(&mut elem, 1, &txt);
    lfield(&mut rich, 2, &elem);
    let mut body = Vec::new();
    lfield(&mut body, 1, &rich);
    lfield(&mut w, 3, &body);
    vfield(&mut w, 4, seq as u64);
    vfield(&mut w, 5, rand as u64);
    let mut f6 = Vec::new();
    vfield(&mut f6, 1, ts);
    lfield(&mut w, 6, &f6);
    w
}

/// 已证群结构(r18):routing{2: Grp{1: varint 群号}},**无 ContentHead**。
fn expected_group(code: u64, text: &str, seq: u32, rand: u32, ts: u64, elem37: &[u8]) -> Vec<u8> {
    let mut w = Vec::new();
    let mut grp = Vec::new();
    vfield(&mut grp, 1, code);
    let mut routing = Vec::new();
    lfield(&mut routing, 2, &grp);
    lfield(&mut w, 1, &routing);
    let mut rich = Vec::new();
    lfield(&mut rich, 2, elem37);
    let mut txt = Vec::new();
    lfield(&mut txt, 1, text.as_bytes());
    let mut elem = Vec::new();
    lfield(&mut elem, 1, &txt);
    lfield(&mut rich, 2, &elem);
    let mut body = Vec::new();
    lfield(&mut body, 1, &rich);
    lfield(&mut w, 3, &body);
    vfield(&mut w, 4, seq as u64);
    vfield(&mut w, 5, rand as u64);
    let mut f6 = Vec::new();
    vfield(&mut f6, 1, ts);
    lfield(&mut w, 6, &f6);
    w
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

const TS: u64 = 1_760_000_000; // 合成固定时间(不取 now)

/// G1:私聊 golden 逐字节;头部形状字节回放 r9 归档 head
/// (`0a1c0a1a1218` + uid;真实 uid 字节已排除)。
#[test]
fn golden_friend_text_byte_exact() {
    let uid = "u_GOLDENFIXpeer00000000A"; // 24 字符,与 r9 真实 uid 同长度级
    assert_eq!(uid.len(), 24);
    let text = "GOLDEN-FRIEND-1 中文🈯\n";
    let got = encode_friend_text(2_987_654_321, Some(uid), text, 7, 9, TS).unwrap();
    let want = expected_friend(uid, text, 7, 9, TS, FRIEND_ELEM37);
    assert_eq!(hex(&got), hex(&want), "私聊 golden 逐字节");
    // r9 归档 head 形状:`0a1c0a1a1218` + 24B uid(结构字节,账号字节除外)。
    let head = hex(&got[..6 + 24]);
    assert!(
        head.starts_with("0a1c0a1a1218"),
        "r9 routing shape: {head}"
    );
    assert_eq!(&head[12..], &hex(uid.as_bytes()), "uid leaf position");
}

/// G2:群 golden 逐字节;头部形状回放 r18 归档 head
/// (`0a071205 08 <群号varint> 1a93010a900112 76aa027350006000`;
/// 真实群号字节已排除)。18B 文本 + 同宽 seq/rand/ts 时总长 = r18 的 173B。
#[test]
fn golden_group_text_byte_exact() {
    let code: u64 = 70_000_001; // 合成群号;4 字节 varint 域,与 r18 真实群号同宽度
    let text = "CALIGO-GOLDEN-GRP1"; // 18B,与 r18 文本同长度级
    assert_eq!(text.len(), 18);
    // rand 取 3 字节 varint 区间、ts 5 字节,使尾部宽度与 r18 一致。
    let got = encode_group_text(code, text, 18, 1_000_000, TS).unwrap();
    let want = expected_group(code, text, 18, 1_000_000, TS, GROUP_ELEM37);
    assert_eq!(hex(&got), hex(&want), "群 golden 逐字节");
    assert_eq!(got.len(), 173, "r18 同宽度入参总长");
    // r18 归档 head 形状:`0a071205 08 <code> 1a9301 0a9001 1276 aa027350006000`。
    let code_varint = {
        let mut v = Vec::new();
        varint(&mut v, code);
        v
    };
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&[0x0a, 0x07, 0x12, 0x05, 0x08]);
    prefix.extend_from_slice(&code_varint);
    prefix.extend_from_slice(&[0x1a, 0x93, 0x01, 0x0a, 0x90, 0x01, 0x12, 0x76, 0xaa, 0x02, 0x73, 0x50, 0x00, 0x60, 0x00]);
    assert_eq!(
        hex(&got[..prefix.len()]),
        hex(&prefix),
        "r18 body/elem37 shape"
    );
}

/// G3(T02):群请求**不带 ContentHead** —— 顶层字段恰好
/// {1,3,4,5,6},字段 2 缺席是 r17/r18 实证约束。
#[test]
fn golden_group_has_no_content_head() {
    let got = encode_group_text(70_000_002, "x", 1, 2, TS).unwrap();
    let mut pos = 0usize;
    let mut fields = Vec::new();
    while pos < got.len() {
        let tag = got[pos];
        pos += 1;
        let field = tag >> 3;
        let wire = tag & 7;
        if wire == 2 {
            let mut len = 0u64;
            let mut shift = 0;
            loop {
                let b = got[pos];
                pos += 1;
                len |= ((b & 0x7F) as u64) << shift;
                if b & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }
            fields.push(field);
            pos += len as usize;
        } else {
            assert_eq!(wire, 0, "顶层只允许 varint/len 字段");
            while got[pos] & 0x80 != 0 {
                pos += 1;
            }
            pos += 1;
            fields.push(field);
        }
    }
    assert_eq!(pos, got.len());
    assert_eq!(fields, vec![1, 3, 4, 5, 6], "群顶层 = {{1,3,4,5,6}},无字段2");
}

/// G4(T03):Unicode 正文完整;相同文本不同 seq/rand 产出不同字节
/// (发送入参参与 wire,不因正文相同而合并)。
#[test]
fn golden_friend_unicode_integrity_and_uniqueness() {
    let uid = "u_GOLDENFIXpeer00000000A";
    let text = "多字节🈯\n两行";
    let a = encode_friend_text(1, Some(uid), text, 1, 2, TS).unwrap();
    let b = encode_friend_text(1, Some(uid), text, 3, 4, TS).unwrap();
    // 正文叶逐字节(UTF-8 不经转码)。
    assert!(a.windows(text.len()).any(|w| w == text.as_bytes()));
    assert_ne!(a, b, "同正文不同 seq/rand 必须不同字节");
}

/// G5:超界正文显式拒绝(不截断、不静默)。
#[test]
fn golden_oversize_text_rejected() {
    let big = "x".repeat(256 * 1024 + 1);
    assert!(encode_friend_text(1, Some("u_x"), &big, 1, 1, TS).is_err());
    assert!(encode_group_text(700_000_003, &big, 1, 1, TS).is_err());
}

/// G6:元素37 常量与归档字节一致(协议常量冻结;改动必须过版本门)。
#[test]
fn elem37_constants_frozen() {
    // 私聊版 26B,声明长 0x17=23:aa 02 17 …。
    assert_eq!(FRIEND_ELEM37.len(), 26);
    assert_eq!(&FRIEND_ELEM37[..3], &[0xaa, 0x02, 0x17]);
    // 群版 118B,声明长 0x73=115(r13 拒因即 88B 截断 + 0x06 错字节)。
    assert_eq!(GROUP_ELEM37.len(), 118);
    assert_eq!(&GROUP_ELEM37[..3], &[0xaa, 0x02, 0x73]);
    assert_eq!(GROUP_ELEM37[5], 0x60, "field12 tag(0x06 为 r13 损坏字节)");
}
