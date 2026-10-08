//! 发送回执解析(T19):result/errmsg 与"稳定身份"分层。
//!
//! 合同(计划 §6.6/§13):result=0 / 空错误 / 候选消息 ID 都**不单独构成
//! confirmed_success**;缺稳定身份 → 分类 Unconfirmed(上游按 Unknown
//! 记账),绝不伪造 message_id。

use super::wire::{Reader, WT_LEN, WT_VARINT};
use super::CodecError;

pub const FIELD_RESULT: u32 = 1;
pub const FIELD_ERRMSG: u32 = 2;

/// 字段号未对 9.9.33 验证(见 mod.rs 纪律)。
pub const FIELD_NUMBERS_VERIFIED: bool = false;

/// 回执分类(与 runtime::NativeResult 的边界对齐)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendReceipt {
    /// 业务结果成功**且**拿到稳定原生消息身份。
    Confirmed { native_identity: String },
    /// 业务成功但没有稳定身份 —— 上游必须记 Unknown,不得编造 ID。
    Unconfirmed { note: &'static str },
    /// 业务明确失败。
    Failed { reason: String },
    /// 回执本身不可解析/被截断。
    Unreadable(String),
}

pub fn parse_send_receipt(bytes: &[u8]) -> SendReceipt {
    let mut r = match Reader::new(bytes) {
        Ok(r) => r,
        Err(e) => return SendReceipt::Unreadable(e.to_string()),
    };
    let mut result: Option<u64> = None;
    let mut errmsg: Option<String> = None;
    loop {
        if r.at_end() {
            break;
        }
        let t = match r.tag() {
            Ok(t) => t,
            Err(e) => return SendReceipt::Unreadable(e.to_string()),
        };
        match (t.field, t.wire) {
            (FIELD_RESULT, WT_VARINT) => match r.varint() {
                Ok(v) => result = Some(v),
                Err(e) => return SendReceipt::Unreadable(e.to_string()),
            },
            (FIELD_ERRMSG, WT_LEN) => match r.bytes_value() {
                Ok(b) => errmsg = Some(String::from_utf8_lossy(b).into_owned()),
                Err(e) => return SendReceipt::Unreadable(e.to_string()),
            },
            _ => {
                if r.skip(t).is_err() {
                    return SendReceipt::Unreadable("skip failed".into());
                }
            }
        }
    }
    let result = match result {
        Some(v) => v,
        None => return SendReceipt::Unreadable("result missing".into()),
    };
    if result != 0 {
        return SendReceipt::Failed {
            reason: errmsg.unwrap_or_else(|| format!("result={result}")),
        };
    }
    // result=0:稳定性存疑 —— 没有可信身份字段前一律 Unconfirmed(T19)。
    SendReceipt::Unconfirmed {
        note: match errmsg {
            Some(e) if !e.is_empty() => "ok with errmsg present",
            _ => "ok without stable identity",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::wire::Writer;
    use super::*;

    /// T19:result=成功但缺稳定身份 → Unconfirmed(不伪造 confirmed)。
    #[test]
    fn ok_without_identity_is_unconfirmed() {
        let mut w = Writer::new();
        w.u32_field(FIELD_RESULT, 0);
        assert_eq!(
            parse_send_receipt(&w.finish()),
            SendReceipt::Unconfirmed { note: "ok without stable identity" }
        );
    }

    /// T19:明确失败携带原因。
    #[test]
    fn failure_carries_reason() {
        let mut w = Writer::new();
        w.u32_field(FIELD_RESULT, 120);
        w.bytes_field(FIELD_ERRMSG, b"not in group");
        assert_eq!(
            parse_send_receipt(&w.finish()),
            SendReceipt::Failed { reason: "not in group".into() }
        );
    }

    /// T19:回执缺 result / 截断 → Unreadable(不猜)。
    #[test]
    fn unreadable_variants() {
        assert_eq!(
            parse_send_receipt(&[]),
            SendReceipt::Unreadable("result missing".into())
        );
        let mut w = Writer::new();
        w.bytes_field(FIELD_ERRMSG, b"0123456789");
        let full = w.finish();
        assert!(matches!(parse_send_receipt(&full[..5]), SendReceipt::Unreadable(_)));
    }
}
