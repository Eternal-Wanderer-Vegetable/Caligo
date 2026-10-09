//! 共享 QQ wire 编解码(有源;bridge 与 core 共用,防跨 crate 漂移)。
//!
//! 证据等级与出处(2026-10-09):
//! - **发送侧字段号 = field-proven**:`MessageSvc.PbSendMsg` 私聊形状经
//!   r6–r9(实例 24316,送达+渲染)实证;群形状经 r17/r18(实例 27624,
//!   群 263402786,送达+渲染)实证。归档证据
//!   `docs/execution/2026-10-08-native-contract-recovery/evidence/p6-g3/`
//!   (`g3-send-24316-r9.jsonl`、`g3-send-27624-r18.jsonl` 的 encode head)。
//! - 元素37 常量为协议常量(自真实报文字节级照抄,非账号数据):
//!   私聊版 26B(capture-main-send-body.jsonl)、群版 118B
//!   (capture-main-group-body.jsonl,r13–r15 拒因即 88B 截断常量)。
//! - 群版元素37 实际为动态生成(104–118B 浮动,尾部嵌每消息随机值);
//!   r18 中静态 118B 随整体通过 = 服务器不校验其动态性,但**这不写成
//!   长期合同**(计划 N1:限定当前支持域)。
//! - 接收侧解码见 caligo-core `qq_protocol::recv`(字段号仍 verified=false,
//!   待记录级字段实证后迁入本模块)。
//!
//! 纪律:编码纯函数、显式时间入参(不内部取 now);未知字段不编;
//! 超界显式拒绝;`@` 研究编码(r20–r28b 归档,静默拒)不迁入 ——
//! 它阻塞于响应 retCode,不构成生产支持面。

pub mod send;

/// protobuf varint 写入(LEBase128)。
pub(crate) fn pb_varint(out: &mut Vec<u8>, mut v: u64) {
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

/// tag = (field << 3) | wire_type。
pub(crate) fn pb_tag(out: &mut Vec<u8>, field: u32, wire: u32) {
    pb_varint(out, (((field as u64) << 3) | wire as u64) & 0xFFFF_FFFF);
}

/// varint 字段(wire type 0)。
pub(crate) fn pb_varint_field(out: &mut Vec<u8>, field: u32, v: u64) {
    pb_tag(out, field, 0);
    pb_varint(out, v);
}

/// 长度定界字段(wire type 2)。
pub(crate) fn pb_len_field(out: &mut Vec<u8>, field: u32, body: &[u8]) {
    pb_tag(out, field, 2);
    pb_varint(out, body.len() as u64);
    out.extend_from_slice(body);
}
