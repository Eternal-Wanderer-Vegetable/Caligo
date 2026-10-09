//! 接收侧 owned 捕获记录与规范化(N1 起点)。
//!
//! 现状边界(P5 相关性报告 + g2-run-21732-r4-p5.jsonl 归档,2026-10-09):
//! - **已证**:Type S 通知头 +0x00 = 本机账号 libc++ SSO 前缀(同一批次
//!   四条通知逐字节恒定);+0x10 = 逐消息 32 位值(批内逐条不同);头部
//!   0x40 字节内**无发送者 uin**。Type P(指针头)+0x00 指向元素/emoji
//!   通知对象。
//! - **未定证(= 记录级映射 blocked)**:sender uid/uin 的记录内位置、
//!   chat_type、平台时间、稳定 native ID 的偏移。对象图测绘待办见
//!   `evidence/p6-g2/p5-correlation-21732-r4.md`。
//! - **禁止**:从批次共享字符串池靠"发现昵称/文本"组装业务事件(N1);
//!   本模块因此不携带任何正文候选,正文提取属于记录解码器(未实现)。
//!
//! 因此 [`NormalizedNotice`] 的 sender/peer/平台时间/native ID 一律
//! `None`(Option 表达"证据未到"),而不是占位值/空串/本地时间。

/// 通知对象头 0x40 字节(8 qword;回调期内 RPM-self 拥有式复制的产物)。
///
/// 部分不可读时整条为 [`CapturedRecordHead::Unreadable`],不用零值冒充。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedRecordHead {
    /// Type S(标准头):归档 i=0..3 形态 = [SSO 自账号 2 qword,
    /// {0, 逐消息32位}, {0x31, 0x25}, 池指针 ×2]。
    Standard([u64; 8]),
    /// Type P(指针头):+0x00 呈堆指针形态(指向元素/emoji 通知对象)。
    Pointer([u64; 8]),
    /// 读取失败(RPM-self 返回错误);qwords 不可用。
    Unreadable,
}

impl CapturedRecordHead {
    /// 由回调期读得的 8 qword 构造(判定规则见 [`Self::classify`])。
    pub fn from_qwords(q: [u64; 8]) -> Self {
        Self::classify(q)
    }

    /// 形态判定:+0x00 呈用户态堆指针形态(非合法短形态 SSO)→ Type P。
    ///
    /// SSO 短形态首字节 = len<<1(偶数、len ∈ 1..=22)且值落在首 qword
    /// 可表达范围;归档 Type P 的 +0x00 为堆地址。判定只用于分型,不用于
    /// 任何身份结论。
    fn classify(q: [u64; 8]) -> Self {
        let b0 = (q[0] & 0xFF) as u8;
        let sso_len = (b0 >> 1) as usize;
        let sso_like = b0 & 1 == 0 && (1..=22).contains(&sso_len);
        let heap_like = crate::g3::addr_plausible(q[0]);
        if !sso_like && heap_like {
            Self::Pointer(q)
        } else {
            Self::Standard(q)
        }
    }

    /// 头 +0x00 起的 libc++ SSO 前缀解码(已证字段;仅短形态可完整解码)。
    ///
    /// 24B SSO 缓冲跨 q0/q1/q2;短形态 len≤22。返回 `None`:Type P /
    /// Unreadable / 长形态(长形态 data 在指针处,拷贝策略属记录解码器,
    /// 此处不追)。
    pub fn self_account_sso(&self) -> Option<String> {
        match self {
            Self::Standard(q) => {
                let mut buf = [0u8; 24];
                for (i, qw) in q.iter().take(3).enumerate() {
                    buf[i * 8..i * 8 + 8].copy_from_slice(&qw.to_le_bytes());
                }
                let form = buf[0];
                if form & 1 != 0 {
                    return None; // 长形态:不在此层追指针
                }
                let len = (form >> 1) as usize;
                if len == 0 || len > 22 {
                    return None;
                }
                let s = std::str::from_utf8(&buf[1..1 + len]).ok()?;
                if !s.bytes().all(|b| b.is_ascii_digit()) {
                    return None; // 账号串全数字;否则不算已证形态
                }
                Some(s.to_string())
            }
            _ => None,
        }
    }

    /// +0x10 的逐消息 32 位(候选区分子;**未定证为 native ID 组成**,
    /// 不得单独用作去重/排序键)。
    pub fn per_msg32(&self) -> Option<u32> {
        match self {
            Self::Standard(q) => Some(q[2] as u32),
            _ => None,
        }
    }
}

/// 规范化接收通知(N1 合同:已证字段如实,未证字段显式 None)。
///
/// 上层(core/daemon)以此为准入:任何 `None` 字段都意味着该通知**不能**
/// 进入业务事件流(缺必需真实字段的标准事件必须拒绝,计划 N4 同口径),
/// 只能进入诊断计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedNotice {
    /// 本机账号(已证:Type S 头 SSO 前缀;短形态可完整解码)。
    pub self_account: Option<String>,
    /// +0x10 逐消息 32 位(候选;未定证)。
    pub per_msg32: Option<u32>,
    /// 发送者(**未定证 → 恒 None**;对象图测绘收口前不得填值)。
    pub sender: Option<String>,
    /// 会话对端(**未定证 → 恒 None**)。
    pub peer: Option<String>,
    /// 平台时间(**未定证 → 恒 None**;禁止本地时钟补齐)。
    pub platform_time: Option<u64>,
    /// 稳定原生消息 ID(**未定证 → 恒 None**;禁止 hash/正文截断冒充)。
    pub native_id: Option<String>,
}

/// 头 → 规范化通知(纯映射;不做任何池扫描/正文提取/字段发明)。
pub fn normalize(head: &CapturedRecordHead) -> NormalizedNotice {
    NormalizedNotice {
        self_account: head.self_account_sso(),
        per_msg32: head.per_msg32(),
        sender: None,
        peer: None,
        platform_time: None,
        native_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成 Type S 头(归档形态:SSO 自账号 + 逐消息32位 + 0x31/0x25 + 池指针)。
    /// 账号/指针/32位值全部合成,不含真实数据;24B SSO 跨 q0/q1/q2 拼装。
    fn synth_standard(acct: &str, per_msg: u32) -> [u64; 8] {
        assert!((1..=22).contains(&acct.len()), "SSO 短形态容量 22B");
        let mut buf = [0u8; 24];
        buf[0] = (acct.len() as u8) << 1;
        buf[1..1 + acct.len()].copy_from_slice(acct.as_bytes());
        let mut q = [0u64; 8];
        for (i, chunk) in buf.chunks_exact(8).enumerate() {
            q[i] = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        q[2] = per_msg as u64;
        q[3] = 0x31;
        q[4] = 0x25;
        q[5] = 0x1000_0000_1000; // 合成"池指针"(形态学合成)
        q[6] = 0x1000_0000_2000;
        q
    }

    /// 已证字段:同批次自账号恒定、逐消息 32 位逐条不同(归档 i=0..3 形态)。
    #[test]
    fn self_account_constant_per_msg_varies() {
        let heads: Vec<_> = [0x1111_1111u32, 0x2222_2222, 0x3333_3333, 0x4444_4444]
            .iter()
            .map(|&m| CapturedRecordHead::from_qwords(synth_standard("1234567890", m)))
            .collect();
        let accts: Vec<_> = heads.iter().map(|h| h.self_account_sso()).collect();
        assert_eq!(accts[0], Some("1234567890".into()));
        assert!(accts.windows(2).all(|w| w[0] == w[1]), "同批次自账号恒定");
        let msgs: Vec<_> = heads.iter().map(|h| h.per_msg32()).collect();
        assert_ne!(msgs[0], msgs[1], "逐消息 32 位逐条不同");
    }

    /// 规范化不发明:sender/peer/时间/native ID 恒 None(证据未到)。
    #[test]
    fn normalization_fabricates_nothing() {
        let n = normalize(&CapturedRecordHead::from_qwords(synth_standard("1234567890", 7)));
        assert_eq!(n.self_account.as_deref(), Some("1234567890"));
        assert_eq!(n.per_msg32, Some(7));
        assert_eq!(n.sender, None, "sender 未定证");
        assert_eq!(n.peer, None, "peer 未定证");
        assert_eq!(n.platform_time, None, "平台时间未定证,禁止本地时钟补齐");
        assert_eq!(n.native_id, None, "native ID 未定证,禁止冒充");
    }

    /// 非数字 SSO 前缀不算已证账号形态 → None(不猜)。
    #[test]
    fn non_digit_sso_rejected() {
        let mut q = synth_standard("1234567890", 1);
        let mut buf = [0u8; 24];
        buf[0] = 5 << 1;
        buf[1..6].copy_from_slice(b"abcde");
        for (i, chunk) in buf.chunks_exact(8).enumerate() {
            q[i] = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        // q2 起(池指针等)保持 synth 形态即可,SSO 判定只看前 3 qword 的串字段。
        assert_eq!(CapturedRecordHead::from_qwords(q).self_account_sso(), None);
    }

    /// Type P(指针头)分型;不产 SSO 结论。
    #[test]
    fn pointer_head_classified() {
        let mut q = synth_standard("1234567890", 1);
        q[0] = 0x1f6f_e0f4_480; // 合成堆指针形态
        let head = CapturedRecordHead::from_qwords(q);
        assert!(matches!(head, CapturedRecordHead::Pointer(_)));
        assert_eq!(head.self_account_sso(), None);
        assert_eq!(head.per_msg32(), None);
    }

    /// 同 per_msg32 不同头不去重语义:规范化输出保留原值,比对留给上层
    /// (本层不做任何合并;T05 同文不同 ID 不去重的接收侧对应物)。
    #[test]
    fn normalize_does_not_dedup() {
        let a = normalize(&CapturedRecordHead::from_qwords(synth_standard("1234567890", 7)));
        let b = normalize(&CapturedRecordHead::from_qwords(synth_standard("1234567890", 7)));
        assert_eq!(a, b, "同头重复规范化应一致(由上层按 ID 去重)");
        assert!(a.native_id.is_none(), "无稳定 ID 时上层不得据此合并");
    }
}
