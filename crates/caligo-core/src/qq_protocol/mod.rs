//! 最小 QQ 文本协议编解码(计划 §7-P5;T17–T20)。
//!
//! 规格来源纪律(source-register S13/S16):字段号取自公开协议参考
//! (go-cqhttp/Lagrange 等社区文档对 MessageSvc.PbSendMsg / MsgPush 的
//! 一致描述),**在拿到真实 9.9.33 抓包前一律标记 `verified: false`**;
//! wire 层与语义层对字段值不做任何"默认值补齐"——未知字段保留原始
//! 字节并计数,截断/坏 tag 显式拒绝(T18)。
//!
//! 模块边界:本模块只做字节 ↔ 结构;身份/回执语义在 [`identity`] 与
//! [`receipt`]。fixture 全部为脱敏自生成(provenance 见测试文件头)。

pub mod identity;
pub mod receipt;
pub mod recv;
pub mod send;
pub mod wire;

/// 通用错误(T18:有界处理;非法输入拒绝而非吞掉)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// 声明长度超出剩余字节(截断包)。
    Truncated { need: usize, have: usize },
    /// 无效 wire tag(field number 0 或 wire type 3/4 组)。
    BadTag(u32),
    /// varint 超过 10 字节(畸形)。
    VarintTooLong,
    /// 输入超过模块级上限(防御性;PbSendMsg/MsgPush 实际远小于此)。
    TooLarge { limit: usize },
    /// 语义层拒绝(缺必需字段等)。
    MissingField(&'static str),
    /// 嵌套深度超限(畸形递归)。
    TooDeep,
}

impl core::fmt::Display for CodecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CodecError::Truncated { need, have } => write!(f, "truncated: need {need}, have {have}"),
            CodecError::BadTag(t) => write!(f, "bad wire tag {t:#x}"),
            CodecError::VarintTooLong => write!(f, "varint exceeds 10 bytes"),
            CodecError::TooLarge { limit } => write!(f, "input exceeds {limit} bytes"),
            CodecError::MissingField(name) => write!(f, "missing required field {name}"),
            CodecError::TooDeep => write!(f, "nesting too deep"),
        }
    }
}

impl std::error::Error for CodecError {}

/// 单条消息的模块级上限(有界;超限拒绝)。
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// 最大嵌套深度。
pub const MAX_DEPTH: usize = 16;

#[cfg(test)]
mod tests {
    #[test]
    fn error_display_smoke() {
        let e = super::CodecError::Truncated { need: 4, have: 1 };
        assert!(e.to_string().contains("truncated"));
    }
}
