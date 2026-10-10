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



// ---------------------------------------------------------------------------
// N1(2026-10-10):消息记录 CPB 读取器(键表来源:实例 27108 活体差分,
// 台账 2026-10-10 八/九;访问器 0x1821878c0 capstone 实证)。
//
// CPB 实例:{vptr@0, entries@+8, count u32@+0x10};条目 12B
// {key u32, tagged qword(bit58–61 = 类型;bit60=2 → 低 48 位堆指针)}。
//
// 已证键(0x9c 族 = 消息记录;斜率语义为**会话内局部序数**,绝对值
// 需一条关联消息标定基座 —— 台账 2026-10-10 九的交叉验证):
// - 0x9c54 = senderUid(指针 → ASCII 缓冲)
// - 0x9c55 = peerUid(指针 → TLV {0x0a, len, bytes}:群码/uin 串)
// - 0x9c43 = 2×seq(实测 /2 = 绝对 seq,N3R18 真值精确吻合;2026-10-10
//   早前"局部基座/偏移漂移"结论系算术误读,已更正)
// - 0x9c72 = 2×platform_time(/2 = 绝对 Unix 秒,同上)
// - 0x9c5b = 会话标识(恒定)
// 正文 = 元素 blob(blob 起始 {0x08, len, utf8};键按 0xb02d/0xa2e4/0xa2e5
// 顺序探测 —— 记录形态随消息类型差异,探测序列见 [TEXT_KEY_CANDIDATES])。

/// 消息记录键(实例 27108 / 9.9.33-52230 差分实证;版本漂移按 N5 重验)。
pub mod record_keys {
    pub const SENDER_UID: u32 = 0x9c54;
    pub const PEER_UID: u32 = 0x9c55;
    pub const SEQ_x2: u32 = 0x9c43; // 值 = 2×seq(/2 = 绝对 seq)
    pub const TIME_x2: u32 = 0x9c72; // 值 = 2×platform_time(/2 = Unix 秒)
    pub const SESSION_ID: u32 = 0x9c5b;
    /// 正文 blob 键探测序(首个命中 {0x08, len} 形态者胜)。
    pub const TEXT_KEY_CANDIDATES: [u32; 3] = [0xb02d, 0xa2e4, 0xa2e5];
}

/// 内存读取抽象(常驻路径接 RPM-self;测试接合成内存)。
pub trait MemoryReader {
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool;
}

/// 一条消息记录的可证字段(None = 该键缺失/形态不符,不猜)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecordFields {
    pub sender_uid: Option<String>,
    pub peer_uid: Option<String>,
    pub text: Option<String>,
    /// 绝对消息 seq(0x9c43 / 2;N3R18 真值精确吻合)。
    pub seq: Option<u64>,
    /// 平台时间(Unix 秒;0x9c72 / 2)。
    pub platform_time: Option<u64>,
    pub session_id: Option<u64>,
}

/// CPB 条目集(从内存读出的 {key, tagged} 对)。
pub struct CpbEntries<'a> {
    pub raw: &'a [u8],
}

impl<'a> CpbEntries<'a> {
    /// 从 12B×count 条目缓冲构造(调用方保证长度)。
    pub fn new(raw: &'a [u8]) -> Self {
        Self { raw }
    }

    pub fn iter_keys(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.raw.chunks_exact(12).map(|c| {
            (u32::from_le_bytes(c[0..4].try_into().unwrap()), u64::from_le_bytes(c[4..12].try_into().unwrap()))
        })
    }

    pub fn find(&self, key: u32) -> Option<u64> {
        self.iter_keys().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}

/// ASCII 缓冲读取(≤64B 可打印;NUL/非打印截断)。
fn ascii_at(rd: &dyn MemoryReader, addr: u64) -> Option<String> {
    let mut buf = [0u8; 64];
    if !rd.read(addr, &mut buf) {
        return None;
    }
    let end = buf.iter().take_while(|&&b| b >= 0x20 && b < 0x7f).count();
    if end < 4 {
        return None;
    }
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

/// peerUid TLV 读取({0x0a, len, bytes};非该形态退回 ASCII)。
fn peer_at(rd: &dyn MemoryReader, addr: u64) -> Option<String> {
    let mut buf = [0u8; 64];
    if !rd.read(addr, &mut buf) {
        return None;
    }
    if buf[0] == 0x0a && buf[1] > 0 && buf[1] as usize <= 62 {
        let n = buf[1] as usize;
        if buf[2..2 + n].iter().all(|&b| b.is_ascii_graphic() || b == b' ') {
            return Some(String::from_utf8_lossy(&buf[2..2 + n]).into_owned());
        }
    }
    ascii_at(rd, addr)
}

/// 正文 blob 读取({0x08, len, utf8};UTF-8 len 以字节计,可 > 127)。
fn text_at(rd: &dyn MemoryReader, addr: u64) -> Option<String> {
    let mut buf = [0u8; 512];
    if !rd.read(addr, &mut buf) {
        return None;
    }
    // blob 前可有少量非正文前缀;在前 8 字节内探测 {0x08, len} 形态。
    for skip in 0..8usize {
        if buf[skip] == 0x08 && buf[skip + 1] > 0 {
            let n = buf[skip + 1] as usize;
            if skip + 2 + n <= buf.len() && n >= 1 {
                let bytes = &buf[skip + 2..skip + 2 + n];
                if std::str::from_utf8(bytes).is_ok() {
                    return Some(String::from_utf8_lossy(bytes).into_owned());
                }
            }
        }
    }
    None
}

/// 从 CPB 对象提取记录字段(经任意 [`MemoryReader`])。
///
/// `cpb_obj` = CPB 实例地址(vptr@0;entries@[+8];count u32@[+0x10])。
/// 有界:count > 80 或 entries 不可读 → 全 None(不猜)。
pub fn read_record_fields(rd: &dyn MemoryReader, cpb_obj: u64) -> RecordFields {
    let mut f = RecordFields::default();
    let mut hdr = [0u8; 12];
    if !rd.read(cpb_obj + 8, &mut hdr[0..8]) || !rd.read(cpb_obj + 0x10, &mut hdr[8..12]) {
        return f;
    }
    let entries = u64::from_le_bytes(hdr[0..8].try_into().unwrap());
    let count = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
    if count == 0 || count > 80 || !(0x1_0000..0x7FFF_FFFE_FFFF).contains(&entries) || entries & 7 != 0 {
        return f;
    }
    let mut eb = vec![0u8; count as usize * 12];
    if !rd.read(entries, &mut eb) {
        return f;
    }
    let table = CpbEntries::new(&eb);
    // 0x9c43/0x9c72 = 2×值(低位 0);/2 还原绝对 seq / 平台时间。
    if let Some(tagged) = table.find(record_keys::SEQ_x2) {
        f.seq = Some((tagged & 0x00FF_FFFF_FFFF_FFFF) / 2);
    }
    if let Some(tagged) = table.find(record_keys::TIME_x2) {
        f.platform_time = Some((tagged & 0x00FF_FFFF_FFFF_FFFF) / 2);
    }
    if let Some(tagged) = table.find(record_keys::SESSION_ID) {
        f.session_id = Some(tagged & 0x0000_FFFF_FFFF_FFFF);
    }
    if let Some(tagged) = table.find(record_keys::SENDER_UID) {
        if cpb_payload_is_heap(tagged) {
            f.sender_uid = ascii_at(rd, tagged & 0x0000_FFFF_FFFF_FFFF);
        }
    }
    if let Some(tagged) = table.find(record_keys::PEER_UID) {
        if cpb_payload_is_heap(tagged) {
            f.peer_uid = peer_at(rd, tagged & 0x0000_FFFF_FFFF_FFFF);
        }
    }
    for key in record_keys::TEXT_KEY_CANDIDATES {
        if let Some(tagged) = table.find(key) {
            if cpb_payload_is_heap(tagged) {
                if let Some(t) = text_at(rd, tagged & 0x0000_FFFF_FFFF_FFFF) {
                    f.text = Some(t);
                    break;
                }
            }
        }
    }
    f
}



// ---------------------------------------------------------------------------
// N3(2026-10-10 续):diff 式接收原语。
//
// 接线形态:通知处理器只做轻量信号(head 入环 + 置位),worker 线程收到
// 触发后跑 CPB 收集并经 [`SeenRecords::absorb`] 差分 —— 新出现/seq 前进的
// 记录 = 新消息事件。回调纪律不破(回调内零重活)。

/// 已见记录账本(键 = CPB 对象地址,值 = 最近 seq)。
#[derive(Default)]
pub struct SeenRecords {
    seen: std::collections::HashMap<u64, u64>,
}

impl SeenRecords {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// 吸收本批收集结果,返回新出现或 seq 前进的记录(即新消息)。
    /// seq 回退视为已见(不回滚 —— 排序以 monotonic 前进为准)。
    pub fn absorb(&mut self, current: &[(u64, RecordFields)]) -> Vec<(u64, RecordFields)> {
        let mut fresh = Vec::new();
        for (obj, f) in current {
            let seq = f.seq.unwrap_or(0);
            match self.seen.get(obj) {
                Some(&old) if old >= seq => {}
                _ => {
                    self.seen.insert(*obj, seq);
                    fresh.push((*obj, f.clone()));
                }
            }
        }
        fresh
    }
}

fn cpb_payload_is_heap(tagged: u64) -> bool {
    let p = tagged & 0x0000_FFFF_FFFF_FFFF;
    (0x1_0000..0x7FFF_FFFE_FFFF).contains(&p) && p & 7 == 0
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

#[cfg(test)]
mod record_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 合成内存:区域表 + 读取器。
    struct FakeMem(BTreeMap<u64, Vec<u8>>);
    impl FakeMem {
        fn put(&mut self, addr: u64, data: &[u8]) {
            self.0.insert(addr, data.to_vec());
        }
    }
    impl MemoryReader for FakeMem {
        fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
            // 区域内任意偏移读取(逐字节走区域映射)。
            for (&base, data) in &self.0 {
                if addr >= base && addr + buf.len() as u64 <= base + data.len() as u64 {
                    let off = (addr - base) as usize;
                    buf.copy_from_slice(&data[off..off + buf.len()]);
                    return true;
                }
            }
            false
        }
    }

    /// N3R17-K3 归档条目表(2026-10-10 实测,实例 0x1d772af5f28;真值
    /// seq=397942 time=1791607545,r17-gt.json)。
    const K3_ENTRIES: [u8; 15 * 12] = build_entries(&[
        (0x9c41, 0x01d772855d00u64),
        (0x9c43, 0xc24ecu64),
        (0x9c4a, 0x4u64),
        (0x9c54, 0x01d772ac4780u64),
        (0x9c55, 0x01d772854b60u64),
        (0x9c5b, 0x1f666a44u64),
        (0x9c72, 0xd59385f2u64),
        (0xa2e4, 0x01d772854650u64),
        (0xa2e5, 0x01d773e38310u64),
        (0xa2e6, 0x0u64),
        (0xa2e7, 0x320000000000u64),
        (0xa2e8, 0x310000000000u64),
        (0xa2ef, 0x727b4u64),
        (0xa2f0, 0x0u64),
        (0xa2f1, 0x01d773f5cc00u64),
    ]);

    const fn build_entries(pairs: &[(u32, u64)]) -> [u8; 15 * 12] {
        // 测试期常量构建(条目按 key 升序 = 实测序)。
        let mut out = [0u8; 15 * 12];
        let mut i = 0;
        while i < pairs.len() {
            let k = pairs[i].0.to_le_bytes();
            let v = pairs[i].1.to_le_bytes();
            let mut j = 0;
            while j < 4 {
                out[i * 12 + j] = k[j];
                out[i * 12 + 4 + j] = v[j];
                out[i * 12 + 8 + j] = v[j + 4];
                j += 1;
            }
            i += 1;
        }
        out
    }

    #[test]
    fn record_fields_extracted_from_archived_entries() {
        let mut mem = FakeMem(BTreeMap::new());
        let cpb = 0x1d772af5f28u64;
        let mut obj = vec![0u8; 0x40];
        obj[0..8].copy_from_slice(&0x01d7_0000_4261_5c8u64.to_le_bytes()); // vptr(合成)
        obj[8..16].copy_from_slice(&0x5000_0000u64.to_le_bytes()); // entries
        obj[0x10..0x14].copy_from_slice(&15u32.to_le_bytes()); // count
        mem.put(cpb, &obj);
        mem.put(0x5000_0000, &K3_ENTRIES);
        // sender:ASCII 缓冲(合成 uid)。
        let mut snd = vec![0u8; 128];
        snd[..24].copy_from_slice(b"u_1918755fTESTSELF00000 ");
        mem.put(0x1d772ac4780, &snd);
        // peer:TLV {0x0a, 9, "700000001"}(群码形态,合成)。
        let mut peer = vec![0u8; 128];
        peer[..11].copy_from_slice(b"
	700000001");
        mem.put(0x1d772854b60, &peer);
        // 正文:元素 blob {0x08, len, utf8}。
        let mut txt = vec![0u8; 600];
        txt[..10].copy_from_slice(b"N3R17-K3");
        mem.put(0x1d772854650, &txt);

        let f = read_record_fields(&mem, cpb);
        assert_eq!(f.seq, Some(397942), "K3 seq 绝对值(0xc24ec/2;台账九数据,算术已更正)");
        assert_eq!(f.platform_time, Some(1791607545), "K3 平台时间(0xd59385f2/2;精确吻合 r17 真值)");
        assert_eq!(f.session_id, Some(0x1f666a44));
        assert_eq!(f.sender_uid.as_deref(), Some("u_1918755fTESTSELF00000"));
        assert_eq!(f.peer_uid.as_deref(), Some("700000001"), "peer TLV 解码");
        assert_eq!(f.text.as_deref(), Some("N3R17-K3"), "元素 blob 正文");
    }

    /// 有界合同:count 超界/entries 非指针 → 全 None(不猜)。
    #[test]
    fn record_reader_bounded() {
        let mut mem = FakeMem(BTreeMap::new());
        let cpb = 0x6000_0000u64;
        let mut o = vec![0u8; 0x20];
        o[8..16].copy_from_slice(&0x5000_1000u64.to_le_bytes());
        o[0x10..0x14].copy_from_slice(&9999u32.to_le_bytes()); // 超界 count
        mem.put(cpb, &o);
        let f = read_record_fields(&mem, cpb);
        assert_eq!(f, RecordFields::default(), "超界 count → 全 None");
    }

    #[test]
    fn scan_and_collect_from_synthetic_region() {
        let wrapper = 0x7fff17000000u64;
        let mut region = vec![0u8; 0x200];
        region[0x10..0x18].copy_from_slice(&(wrapper + 0x4261_5C8).to_le_bytes());
        region[0x100..0x108].copy_from_slice(&(wrapper + 0x4261_5C8).to_le_bytes());

        let mut cpbs = Vec::new();
        scan_cpb_in_region(&region, 0x9000_0000, wrapper, &mut cpbs);
        assert_eq!(cpbs, vec![0x9000_0010, 0x9000_0100]);

        let mut mem = FakeMem(BTreeMap::new());
        let mut hdr_a = vec![0u8; 0x14];
        hdr_a[0..8].copy_from_slice(&0u64.to_le_bytes()); // entries = 0 → 跳过
        hdr_a[8..12].copy_from_slice(&5u32.to_le_bytes());
        mem.put(0x9000_0010 + 8, &hdr_a);
        let mut hdr_b = vec![0u8; 0x14];
        hdr_b[0..8].copy_from_slice(&0x7000_0000u64.to_le_bytes());
        hdr_b[8..12].copy_from_slice(&1u32.to_le_bytes());
        mem.put(0x9000_0100 + 8, &hdr_b);
        let mut eb = vec![0u8; 12];
        eb[0..4].copy_from_slice(&0x9c54u32.to_le_bytes());
        eb[4..12].copy_from_slice(&0x7001_0000u64.to_le_bytes());
        mem.put(0x7000_0000, &eb);
        let mut snd = vec![0u8; 64];
        snd[..10].copy_from_slice(b"u_TESTSEND");
        mem.put(0x7001_0000, &snd);

        let got = collect_record_fields(&mem, &cpbs, 10);
        assert_eq!(got.len(), 1, "无 sender 的实例跳过");
        assert_eq!(got[0].1.sender_uid.as_deref(), Some("u_TESTSEND"));
    }

    #[test]
    fn seen_records_absorb_yields_only_fresh() {
        let mut seen = SeenRecords::new();
        let mk = |obj: u64, seq: Option<u64>| (obj, RecordFields { seq, ..Default::default() });
        // 首批:两条新 + 一条无 seq(记为 0)。
        let b1 = vec![mk(1, Some(100)), mk(2, Some(101)), mk(3, None)];
        let fresh = seen.absorb(&b1);
        assert_eq!(fresh.len(), 3, "首批评见");
        // 第二批:已见(不变)+ seq 前进 + 全新。
        let b2 = vec![mk(1, Some(100)), mk(2, Some(103)), mk(4, Some(7))];
        let fresh = seen.absorb(&b2);
        assert_eq!(fresh.len(), 2);
        assert_eq!(fresh[0].0, 2, "seq 前进 = 新消息");
        assert_eq!(fresh[1].0, 4);
        // 第三批:seq 回退 → 不产出(不回滚)。
        let b3 = vec![mk(2, Some(102))];
        assert!(seen.absorb(&b3).is_empty(), "seq 回退不回滚");
        assert_eq!(seen.len(), 4);
    }

}


// ---------------------------------------------------------------------------
// N3 接线预备(2026-10-10):进程内读取与 CPB 收集。
//
// time/seq 语义(2026-10-10 修正):0x9c43/0x9c72 直接携带绝对
// seq/平台时间(×2 形态;/2 还原,N3R18 真值精确吻合)—— 无需基座标定。
// 早前"局部基座/漂移"结论系算术误读,已在台账更正。

/// 进程内 RPM-self 读取器(g2 同款内核校验读;坏页返回 false 不抛异常)。
pub struct RpmSelfReader;

impl MemoryReader for RpmSelfReader {
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut got = 0usize;
        // SAFETY: 自进程 RPM;缓冲由调用方持有。
        let ok = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                addr as *const core::ffi::c_void,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut got,
            )
        };
        ok != 0 && got == buf.len()
    }
}

/// 在单个已提交区域缓冲内扫 CPB 实例(vptr == wrapper_base + 0x42615c8)。
/// 8 字节步进(实例 vptr 天然 qword 对齐)。
pub fn scan_cpb_in_region(buf: &[u8], region_base: u64, wrapper_base: u64, out: &mut Vec<u64>) {
    let needle = (wrapper_base + 0x4261_5C8).to_le_bytes();
    let mut i = 0;
    while i + 8 <= buf.len() {
        if buf[i..i + 8] == needle {
            out.push(region_base + i as u64);
        }
        i += 8;
    }
}

/// 收集 CPB 实例的记录字段(有界;无 sender 且无正文的实例跳过 ——
/// 它们是其它对象族的属性表,不是消息记录)。
pub fn collect_record_fields(
    rd: &dyn MemoryReader,
    cpbs: &[u64],
    max: usize,
) -> Vec<(u64, RecordFields)> {
    let mut out = Vec::new();
    for &obj in cpbs.iter().take(max) {
        let f = read_record_fields(rd, obj);
        if f.sender_uid.is_some() || f.text.is_some() || f.peer_uid.is_some() {
            out.push((obj, f));
        }
        if out.len() >= max {
            break;
        }
    }
    out
}
