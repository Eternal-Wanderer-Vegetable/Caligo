//! N3 常驻 native 后端(计划 §7-N3 动作 2):把 resident/daemon_client 从
//! 旧 JS/V8 pump 前提迁到 native 后端 —— 复用 IPC v3、跨连接未确认事件
//! 窗口、actor/journal(core 侧零改动),替换的是宿主前提:
//!
//! - **发送**:HostOp::SendText → qq_wire 已证形状(群聊)→
//!   `NativeSession::enqueue_send`(G3NativeOps 完整 FFI;r2 现场实证
//!   服务器连号 seq)。`SendFired` 挂账,CPB 回流匹配(fresh 记录
//!   peer+text 对齐)后 `complete_sends` 终态;未回流由 core 侧 deadline
//!   表达 Unknown(受理 ≠ 回流,不冒充成功)。
//! - **接收**:worker 线程 CPB 差分轮询(`SeenRecords::absorb`;进程内
//!   ~90ms/轮)。本版为**纯轮询** —— ledger(十三)定稿形态(通知处理器
//!   轻量信号 + worker 收集)的严格子集:零回调接线,通知 swap 的常驻
//!   化属后续增量(需独立生命周期评审)。
//! - **泵**:自有线程 + condvar 唤醒(零 uv_async/V8);daemon_client 的
//!   wake = 泵唤醒。g3 FFI 无 JS 上下文前提(远线程调用现场实证),
//!   owner 语义 = 泵线程自声明,bootstrap 在泵线程执行。
//! - **私聊发送显式拒绝**(native code 11;uin→uid 解析属 N3 接线)。
//! - **kind 判别零猜测**:CPB 键 0x9c4a 语义存疑(台账 §353/383),事件
//!   kind 不从内存推断 —— peer 命中部署配置的群号表 → 2(群);否则
//!   0(unknown;core 按"缺必需真实字段"处理,不误路由)。
//!
//! 研究构建专用;注入纪律与 g2/g3-session 同(manifest + 指定实例)。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::daemon_client::{run_worker, DaemonClientConfig, WorkerExit};
use crate::g3_ops::G3NativeOps;
use crate::resident::SendOutcome;
use crate::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use crate::native_receive::{
    collect_record_fields, scan_cpb_objects_inproc, RpmSelfReader, SeenRecords,
};
use crate::native_response::SeqAllocator;
use crate::native_session::NativeSession;
use crate::resident::{EventSourceKind, OwnedEvent, Resident, ResidentLimits};

/// `native_op` 明确失败码(drain → `SendOutcome::Failure`,reason =
/// "native code N";编码区分语义,未发起类与业务失败分开)。
pub mod native_code {
    /// 私聊/未知 peer 路线未接线(显式拒绝,零原生调用)。
    pub const PEER_UNSUPPORTED: u32 = 11;
    /// 群 wire 编码失败。
    pub const ENCODE_FAILED: u32 = 12;
    /// 群号解析失败(非 u64)。
    pub const BAD_GROUP_CODE: u32 = 13;
    /// 会话层未就绪(未发起;非业务失败)。
    pub const NOT_READY: u32 = 14;
    /// seq 分配/登记拒绝(未发起)。
    pub const ALLOCATION_FAILED: u32 = 15;
    /// ops 调用前拒绝(未发起)。
    pub const PRE_CALL_REFUSED: u32 = 16;
}

/// ctx 布局(`caligo_native_resident_run`;loader 写入,本进程内有效):
/// +0x00 wrapper_base (u64)
/// +0x08 report_path (*const u16,NUL 结尾 UTF-16)
/// +0x10 pipe_name (*const u16,daemon **bridge 管道全名**)
/// +0x18 auth_token (*const u8,NUL 结尾 UTF-8)
/// +0x20 account (*const u8,bot uin 十进制串;须与 caligod --account 一致)
/// +0x28 module_baseline (*const u8)
/// +0x30 session_generation (u64)
/// +0x38 group_codes (*const u8,逗号分隔 u64;空 = 全部事件 kind=unknown)
/// +0x40 self_uid (*const u8,可选;空 = direction 一律 incoming)
/// +0x48 poll_ms (u32;CPB 差分轮询周期,默认 1000,下限 200)
/// +0x4c observe_ms (u32;研究有界窗口,0 = 直到 daemon 停止,上限 600s)
#[repr(C)]
pub struct NativeResidentCtx {
    pub wrapper_base: u64,
    pub report_path: *const u16,
    pub pipe_name: *const u16,
    pub auth_token: *const u8,
    pub account: *const u8,
    pub module_baseline: *const u8,
    pub session_generation: u64,
    pub group_codes: *const u8,
    pub self_uid: *const u8,
    pub poll_ms: u32,
    pub observe_ms: u32,
}

/// UTF-16 NUL 结尾串读取(g3_session 同款)。
///
/// # Safety
/// `p` 必须指向本进程内可读的 NUL 结尾 UTF-16 缓冲。
unsafe fn wide_str(p: *const u16) -> String {
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

/// UTF-8 NUL 结尾串读取(上限 64KiB)。
///
/// # Safety
/// `p` 必须指向本进程内可读的 NUL 结尾缓冲。
unsafe fn utf8_str(p: *const u8) -> String {
    unsafe {
        let mut len = 0usize;
        while len < 64 * 1024 && *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf8_lossy(std::slice::from_raw_parts(p, len)).into_owned()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// JSONL 阶段行(g2/g3-session 同款;tid + ts + stage + ok + detail)。
/// 线程共享的是 owned String(报告路径读一次,原始指针不出主线程)。
fn stage(report: &str, stage_name: &str, ok: bool, detail: &str) {
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    let line = serde_json::json!({
        "ts": now_ms(), "tid": tid, "stage": stage_name, "ok": ok, "detail": detail
    });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(report) {
        let _ = f.write_all(line.to_string().as_bytes());
        let _ = f.write_all(b"\n");
    }
}

// ---------------------------------------------------------------------------
// kind 判别(纯函数;零猜测 —— 只认部署配置群号表)
// ---------------------------------------------------------------------------

/// peer → `OwnedEvent::chat_type`:命中部署配置群号表 = 2(群);否则 0
/// (unknown;core 按"缺必需真实字段"处理,不误路由为私聊)。
pub fn chat_type_for_peer(peer: &str, groups: &HashSet<u64>) -> u32 {
    match peer.parse::<u64>() {
        Ok(code) if groups.contains(&code) => 2,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// CPB 回流匹配器(SendFired → 终态;诚实:只有回流证据才算送达证据)
// ---------------------------------------------------------------------------

/// 一笔在途发送的回流匹配键。
#[derive(Debug, Clone)]
struct PendingFire {
    peer: String,
    text: String,
    /// 发起时刻(Unix 秒;回流 platform_time 不得早于它 − 容差)。
    since_secs: u64,
    /// 登记时刻(Unix 毫秒;过期清理用)。
    registered_ms: u64,
}

/// SendFired 登记账本(泵线程登记,接收 worker 消费;有界,过期清理)。
#[derive(Default)]
pub struct EchoMatcher {
    pending: Mutex<HashMap<String, PendingFire>>,
    /// 过期未回流被清理的 mid 数(诊断可见;core 侧 deadline 另行 Unknown)。
    pub expired: AtomicU64,
    /// 已回流确认数。
    pub confirmed: AtomicU64,
}

/// 回流窗口:登记超过此时长未匹配即清理(core 侧 deadline 兜底 Unknown)。
const ECHO_TTL_MS: u64 = 180_000;
/// 回流时间容差(秒):平台时间与本地钟存在偏差(实例实测 ~30s)。
const ECHO_TIME_SLACK_SECS: u64 = 120;

impl EchoMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一笔在途发送(mid = SendFired 的候选消息 ID)。
    pub fn register(&self, mid: String, peer: String, text: String) {
        let now = now_ms();
        self.pending.lock().unwrap().insert(
            mid,
            PendingFire {
                peer,
                text,
                since_secs: now / 1000,
                registered_ms: now,
            },
        );
    }

    /// 用一条 fresh CPB 记录匹配在途发送:peer 与 text 全等、
    /// platform_time ≥ 发起时刻 − 容差。命中即摘除(一条回流只确认一笔)。
    pub fn match_record(
        &self,
        peer: Option<&str>,
        text: Option<&str>,
        platform_time: Option<u64>,
    ) -> Option<String> {
        let now = now_ms();
        let mut p = self.pending.lock().unwrap();
        // 过期清理(不阻塞登记路径;诊断计数)。
        let stale: Vec<String> = p
            .iter()
            .filter(|(_, f)| now.saturating_sub(f.registered_ms) > ECHO_TTL_MS)
            .map(|(k, _)| k.clone())
            .collect();
        for k in stale {
            p.remove(&k);
            self.expired.fetch_add(1, Ordering::Relaxed);
        }
        let (Some(peer), Some(text)) = (peer, text) else {
            return None;
        };
        let hit = p.iter().find(|(_, f)| {
            &f.peer == peer
                && &f.text == text
                // 时间缺席(半成品记录)不参与回流确认(保守:不误确认)。
                && platform_time
                    .map(|t| t + ECHO_TIME_SLACK_SECS >= f.since_secs)
                    .unwrap_or(false)
        });
        let mid = hit.map(|(k, _)| k.clone());
        if let Some(m) = &mid {
            p.remove(m);
            self.confirmed.fetch_add(1, Ordering::Relaxed);
        }
        mid
    }

    pub fn pending_len(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
}

// ---------------------------------------------------------------------------
// NativeHostAdapter(owner = 泵线程;零 V8/uv 前提)
// ---------------------------------------------------------------------------

type NativeSeqAlloc = SeqAllocator<Box<dyn FnMut(u64) -> Result<(), std::io::Error> + Send>>;

/// g3 路线的宿主适配器:owner = 泵线程(启动时自声明 tid),env = 锚点
/// 探针(每次进入宿主 API 前重查,不缓存),current_context = 恒可
/// (g3 FFI 无 JS 上下文前提;远线程调用现场实证)。
pub struct NativeHostAdapter {
    wrapper_base: usize,
    owner_tid: Arc<AtomicU64>,
    inner: Mutex<AdapterInner>,
    matcher: Arc<EchoMatcher>,
    /// 请求/候选消息 ID 单调计数(唯一性)。
    id_seq: AtomicU64,
}

struct AdapterInner {
    ops: G3NativeOps,
    session: NativeSession<NativeSeqAlloc>,
}

impl NativeHostAdapter {
    pub fn new(
        wrapper_base: usize,
        session: NativeSession<NativeSeqAlloc>,
        matcher: Arc<EchoMatcher>,
        owner_tid: Arc<AtomicU64>,
    ) -> Self {
        Self {
            wrapper_base,
            owner_tid,
            inner: Mutex::new(AdapterInner {
                ops: G3NativeOps::new(wrapper_base),
                session,
            }),
            matcher,
            id_seq: AtomicU64::new(1),
        }
    }

    fn env_ok(&mut self) -> bool {
        // 每次进入宿主 API 前重查锚点(getter 租约零副作用;不缓存 —— K3 教训)。
        matches!(self.inner.get_mut().unwrap().ops.probe_service(), Ok(_))
    }
}

impl NativeHostAdapter {
    /// 排空消费响应槽 → 账本终局 → (request_id, SendOutcome)。
    /// 映射:code==0 → BusinessAccepted(ack,无 native_id;计划 §178
    /// "ACK 不能变成业务成功"的忠实表达);code!=0 → BusinessRejected
    /// (retCode/errStr 如实)。
    pub fn drain_send_responses(&mut self) -> Vec<(String, SendOutcome)> {
        let mut inner = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for (cookie, code, err) in crate::consumer_probe::drain_responses() {
            let outcome = if code == 0 {
                crate::native_response::ResponseOutcome::BusinessAccepted
            } else {
                crate::native_response::ResponseOutcome::BusinessRejected {
                    ret_code: code,
                    err: err.clone(),
                }
            };
            let completion = inner.session.on_response(&cookie, outcome);
            if let crate::native_response::Completion::Terminated { request_id, outcome } = completion {
                let so = match outcome {
                    crate::native_response::ResponseOutcome::BusinessAccepted => {
                        SendOutcome::Success { request_id: request_id.clone(), native_id: None }
                    }
                    crate::native_response::ResponseOutcome::BusinessSuccess { native_id } => {
                        SendOutcome::Success { request_id: request_id.clone(), native_id: Some(native_id) }
                    }
                    crate::native_response::ResponseOutcome::BusinessRejected { ret_code, err } => {
                        SendOutcome::Failure { request_id: request_id.clone(), reason: format!("retCode={ret_code} err={err}") }
                    }
                    crate::native_response::ResponseOutcome::TransportFailed { reason } => {
                        SendOutcome::Failure { request_id: request_id.clone(), reason }
                    }
                };
                out.push((request_id, so));
            }
            // DuplicateIgnored/StaleGeneration/UnknownCookie = 审计类,不上升。
        }
        out
    }
}

impl HostAdapter for NativeHostAdapter {
    fn owner_thread_id(&self) -> u64 {
        self.owner_tid.load(Ordering::Acquire)
    }
    /// SAFETY: 只读查询当前线程 ID。
    fn current_thread_id(&self) -> u64 {
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
    }
    fn env_valid(&self) -> bool {
        self.inner.lock().unwrap().ops.probe_service().is_ok()
    }
    /// g3 FFI 无 JS current 上下文概念(远线程调用现场实证);恒可。
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        match op {
            HostOp::Probe => {
                if !self.env_ok() {
                    return Err(HostError::EnvInvalid);
                }
                Ok(HostOpResult::ProbeDone)
            }
            // 接收 = CPB 差分轮询(本版零回调接线)→ 延迟模式。
            HostOp::ListenerAdd => Ok(HostOpResult::ListenerDeferred),
            // 延迟模式下 resident 不持有监听器 token,不会到达;到达即如实确认。
            HostOp::ListenerRemove { .. } => Ok(HostOpResult::ListenerRemoved),
            HostOp::SendText {
                chat_type,
                peer_uid,
                text,
            } => {
                if chat_type != 2 {
                    // 显式拒绝(零原生调用):私聊 uin→uid 解析属 N3 接线。
                    return Err(HostError::Native {
                        code: native_code::PEER_UNSUPPORTED,
                    });
                }
                let group_code: u64 = match peer_uid.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(HostError::Native {
                            code: native_code::BAD_GROUP_CODE,
                        })
                    }
                };
                if !self.env_ok() {
                    return Err(HostError::EnvInvalid);
                }
                let inner = self.inner.get_mut().unwrap();
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let body = match caligo_model::qq_wire::send::encode_group_text(
                    group_code,
                    &text,
                    0,
                    // random 每消息新鲜(服务器 (uin, random) 近窗去重,N3R28 实证)。
                    caligo_model::qq_wire::send::fresh_random(),
                    ts,
                ) {
                    Ok(b) => b,
                    Err(_) => {
                        return Err(HostError::Native {
                            code: native_code::ENCODE_FAILED,
                        })
                    }
                };
                // 新鲜 16B cookie(g3-session 同款:时钟派生;零值由 register 拒绝)。
                let cv = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1);
                let mut cookie = crate::native_response::Cookie([0u8; 16]);
                cookie.0[..8].copy_from_slice(&cv.to_le_bytes());
                cookie.0[8..16].copy_from_slice(&(cv.wrapping_mul(0x5DEECE66D)).to_le_bytes());
                let rid = format!(
                    "native-resident-{}",
                    self.id_seq.fetch_add(1, Ordering::Relaxed)
                );
                match inner.session.enqueue_send(&mut inner.ops, &rid, cookie, &body) {
                    crate::native_session::SendAcceptance::Dispatched { .. } => {
                        let mid = format!(
                            "n3r-{}-{}",
                            now_ms(),
                            self.id_seq.fetch_add(1, Ordering::Relaxed)
                        );
                        self.matcher.register(mid.clone(), peer_uid, text);
                        Ok(HostOpResult::SendFired { mid })
                    }
                    crate::native_session::SendAcceptance::NotReady { .. } => {
                        Err(HostError::Native { code: native_code::NOT_READY })
                    }
                    crate::native_session::SendAcceptance::AllocationFailed { .. } => Err(
                        HostError::Native { code: native_code::ALLOCATION_FAILED },
                    ),
                    crate::native_session::SendAcceptance::RefusedPreCall { .. } => Err(
                        HostError::Native { code: native_code::PRE_CALL_REFUSED },
                    ),
                    crate::native_session::SendAcceptance::NotAttached => Err(HostError::EnvInvalid),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// N3 常驻运行(研究构建;有界同步返回):
/// attach → CPB 基线 → 泵线程(bootstrap+drain)→ 接收 worker(CPB 差分
/// 轮询 → 事件注入)→ daemon_client worker(IPC v3)→ observe_ms 或停止。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`NativeResidentCtx`];各字符串指针 NUL 结尾。
pub unsafe fn native_resident_run(ctx: *const NativeResidentCtx) -> u32 {
    // 主线程一次性解引用 ctx(原始指针不跨线程;线程只拿 owned 数据)。
    let (wb, report, pipe_name, auth_token, account, module_baseline, session_generation, group_codes_raw, self_uid, poll_ms, observe_ms) = unsafe {
        (
            (*ctx).wrapper_base as usize,
            wide_str((*ctx).report_path),
            wide_str((*ctx).pipe_name),
            utf8_str((*ctx).auth_token),
            utf8_str((*ctx).account),
            utf8_str((*ctx).module_baseline),
            (*ctx).session_generation,
            utf8_str((*ctx).group_codes),
            utf8_str((*ctx).self_uid),
            (*ctx).poll_ms.clamp(200, 60_000),
            (*ctx).observe_ms.min(600_000),
        )
    };
    let groups: HashSet<u64> = group_codes_raw
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    stage(
        &report,
        "n3res_start",
        true,
        &format!(
            "wb={wb:#x} pipe={pipe_name} gen={session_generation}:{account} groups={} poll={poll_ms} observe={observe_ms}",
            groups.len()
        ),
    );

    // ⓪ consumer 武装(research;报告路径 + wrapper 基址)—— per-send
    // cookie delegate 的 manager/invoke 观测与 fallback 依赖此次登记。
    #[cfg(feature = "research")]
    crate::consumer_probe::init(wb, (*ctx).report_path);

    // ① attach(真实服务对象地址 = 诚实 proof 证据;g3-session r2 同款)。
    let service_addr = {
        let mut probe = G3NativeOps::new(wb);
        match probe.probe_service() {
            Ok(a) => a,
            Err(e) => {
                stage(&report, "n3res_attach", false, &format!("{e:?}"));
                return 1;
            }
        }
    };
    let mut session: NativeSession<NativeSeqAlloc> =
        NativeSession::new(SeqAllocator::new(1, Box::new(|_| Ok(()))));
    if let Err(e) = session.attach(
        &mut G3NativeOps::new(wb),
        crate::native_session::AttachProof {
            module_baseline: module_baseline.clone(),
            anchor_candidates: 1,
            service_addr,
            host_generation: 1,
            session_generation,
        },
    ) {
        stage(&report, "n3res_session", false, &format!("{e:?}"));
        return 2;
    }
    stage(&report, "n3res_session", true, &format!("Ready svc={service_addr:#x}"));

    // ② CPB 基线(首批评见;差分从 baseline 之后起算)。
    let base_objs = scan_cpb_objects_inproc(wb as u64);
    let base_recs = collect_record_fields(&RpmSelfReader, &base_objs, 8192);
    let mut seen = SeenRecords::new();
    seen.absorb(&base_recs);
    stage(
        &report,
        "n3res_baseline",
        true,
        &format!("cpb={} records={}", base_objs.len(), base_recs.len()),
    );

    // ③ 共享件。
    let stop = Arc::new(AtomicBool::new(false));
    let matcher = Arc::new(EchoMatcher::new());
    let owner_tid = Arc::new(AtomicU64::new(0));
    let adapter = NativeHostAdapter::new(wb, session, matcher.clone(), owner_tid.clone());
    let resident = Arc::new(Resident::new(adapter, ResidentLimits::default()));
    let (result_tx, result_rx) = std::sync::mpsc::channel::<crate::resident::SendOutcome>();
    let (event_tx, event_rx) = std::sync::mpsc::channel::<OwnedEvent>();
    let wake_pair = Arc::new((Mutex::new(false), Condvar::new()));

    // ④ 泵线程:bootstrap → drain 循环(锁序/回调纪律由 Resident 承担)。
    {
        let resident = resident.clone();
        let stop_pump = stop.clone();
        let wake_pair = wake_pair.clone();
        let result_tx = result_tx.clone();
        let event_tx = event_tx.clone();
        let report_pump = report.clone();
        let spawner = std::thread::Builder::new()
            .name("caligo-native-pump".into())
            .spawn(move || {
                owner_tid.store(
                    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() } as u64,
                    Ordering::Release,
                );
                // bootstrap 失败只可能是锚点失效(native 路线无 quiet 条件);
                // 每 200ms 重试,10s 上限后如实放弃。
                let t0 = std::time::Instant::now();
                let boot_ok = loop {
                    match resident.bootstrap() {
                        Ok(_) => break true,
                        Err(e) => {
                            if stop_pump.load(Ordering::Relaxed)
                                || t0.elapsed() > std::time::Duration::from_secs(10)
                            {
                                stage(&report_pump, "n3res_bootstrap", false, &format!("give up: {e:?}"));
                                break false;
                            }
                            stage(&report_pump, "n3res_bootstrap_retry", false, &format!("{e:?}"));
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                    }
                };
                if boot_ok {
                    stage(&report_pump, "n3res_bootstrap", true, "Ready(owner=pump)");
                }
                while boot_ok && !stop_pump.load(Ordering::Relaxed) {
                    {
                        let (flag, cv) = &*wake_pair;
                        let guard = flag.lock().unwrap();
                        // 100ms 节拍 + wake 即时唤醒。
                        let _ = cv.wait_timeout(guard, std::time::Duration::from_millis(100));
                    }
                    let _ = resident.drain();
                    // N3 retCode 消费者排空:QQ 分发线程 → 槽 → 账本终局。
                    resident.drain_send_responses();
                    for outcome in resident.take_results() {
                        let _ = result_tx.send(outcome);
                    }
                    for ev in resident.take_events() {
                        let _ = event_tx.send(ev);
                    }
                }
                // 关闭(诚实分类;报告只写计数,不虚假 Clean)。
                match resident.close() {
                    Ok(cr) => stage(
                        &report_pump,
                        "n3res_close",
                        cr.cancelled_unsent == 0 && cr.marked_unknown == 0,
                        &format!(
                            "cancelled={} unknown={} late_rejected={}",
                            cr.cancelled_unsent, cr.marked_unknown, cr.late_callbacks_rejected
                        ),
                    ),
                    Err(e) => stage(&report_pump, "n3res_close", false, &format!("{e:?}")),
                }
            });
        if spawner.is_err() {
            stage(&report, "n3res_spawn", false, "pump thread");
            return 3;
        }
    }

    // ⑤ 接收 worker:CPB 差分轮询 → 回流匹配 + 事件注入。
    {
        let resident = resident.clone();
        let stop_recv = stop.clone();
        let matcher = matcher.clone();
        let report_recv = report.clone();
        let spawner = std::thread::Builder::new()
            .name("caligo-native-recv".into())
            .spawn(move || {
                let mut events_emitted: u64 = 0;
                let mut partial_seen: u64 = 0;
                let mut echo_done: Vec<(String, String)> = Vec::new();
                let mut echo_rejected: Vec<(String, String)> = Vec::new();
                let slice = std::time::Duration::from_millis(100);
                loop {
                    // 分片睡眠:stop 响应 ≤100ms。
                    for _ in 0..(poll_ms as u64 / 100).max(1) {
                        if stop_recv.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(slice);
                    }
                    if stop_recv.load(Ordering::Relaxed) {
                        break;
                    }
                    let objs = scan_cpb_objects_inproc(wb as u64);
                    let recs = collect_record_fields(&RpmSelfReader, &objs, 8192);
                    let fresh = seen.absorb(&recs);
                    for (_obj, rf) in fresh {
                        // 回流匹配(先于事件;自发消息回流确认 SendFired)。
                        if let Some(mid) = matcher.match_record(
                            rf.peer_uid.as_deref(),
                            rf.text.as_deref(),
                            rf.platform_time,
                        ) {
                            match rf.seq {
                                Some(seq) => echo_done.push((mid, format!("cpbseq:{seq}"))),
                                None => echo_rejected.push((mid, "echo record missing seq".into())),
                            }
                        }
                        // 事件:peer+seq 在场才发出(text 可空;半成品对象计诊断)。
                        match (&rf.peer_uid, rf.seq) {
                            (Some(peer), Some(seq)) => {
                                let ev = OwnedEvent {
                                    source: EventSourceKind::Recv,
                                    chat_type: chat_type_for_peer(peer, &groups),
                                    peer_uid: peer.clone(),
                                    peer_uin: peer.clone(),
                                    sender_uin: rf.sender_uid.clone().unwrap_or_default(),
                                    native_id: format!("cpbseq:{seq}"),
                                    text: rf.text.clone().unwrap_or_default(),
                                    msg_time: rf.platform_time,
                                };
                                if resident.ingest_event(ev) {
                                    events_emitted += 1;
                                }
                            }
                            _ => partial_seen += 1,
                        }
                    }
                    if !echo_done.is_empty() || !echo_rejected.is_empty() {
                        resident.complete_sends(
                            &std::mem::take(&mut echo_done),
                            &std::mem::take(&mut echo_rejected),
                        );
                    }
                }
                stage(
                    &report_recv,
                    "n3res_recv_done",
                    true,
                    &format!(
                        "events={events_emitted} partial={partial_seen} echo_ok={} echo_expired={} pending_left={}",
                        matcher.confirmed.load(Ordering::Relaxed),
                        matcher.expired.load(Ordering::Relaxed),
                        matcher.pending_len(),
                    ),
                );
            });
        if spawner.is_err() {
            stop.store(true, Ordering::Relaxed);
            stage(&report, "n3res_spawn", false, "recv thread");
            return 3;
        }
    }

    // ⑥ daemon_client worker(IPC v3;重连绝不重新 bootstrap)。
    let worker_handle = {
        let resident = resident.clone();
        let stop_worker = stop.clone();
        let wake_pair = wake_pair.clone();
        let cfg = DaemonClientConfig {
            pipe_name,
            auth_token,
            account,
            module_baseline,
            session_generation,
            self_uid,
            ..Default::default()
        };
        let spawner = std::thread::Builder::new()
            .name("caligo-daemon-client".into())
            .spawn(move || {
                let wake = move || {
                    let (flag, cv) = &*wake_pair;
                    let mut f = flag.lock().unwrap();
                    *f = true;
                    cv.notify_all();
                    true
                };
                run_worker(cfg, resident, &wake, result_rx, event_rx, stop_worker)
            });
        match spawner {
            Ok(h) => h,
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                stage(&report, "n3res_spawn", false, "daemon worker thread");
                return 3;
            }
        }
    };

    // ⑦ 有界窗口(或直到外部停止;研究注入走有界窗)。
    let deadline = if observe_ms > 0 {
        Some(
            std::time::Instant::now() + std::time::Duration::from_millis(observe_ms as u64),
        )
    } else {
        None
    };
    while !stop.load(Ordering::Relaxed) {
        if let Some(d) = deadline {
            if std::time::Instant::now() >= d {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    stage(&report, "n3res_stopping", true, "stop signal");
    stop.store(true, Ordering::Relaxed);
    {
        let (_, cv) = &*wake_pair;
        cv.notify_all();
    }

    // ⑧ 汇合 worker(守护上限 30s;超时如实记录,不悬挂)。
    let join_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut joined: Option<Result<(crate::daemon_client::WorkerCounters, WorkerExit), String>> =
        None;
    while joined.is_none() {
        if worker_handle.is_finished() {
            joined = Some(worker_handle.join().map_err(|_| "panic".into()));
            break;
        }
        if std::time::Instant::now() > join_deadline {
            stage(
                &report,
                "n3res_join",
                false,
                "daemon worker join timeout(线程随进程回收)",
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if let Some(Ok((counters, exit))) = joined {
        let exit_name = match exit {
            WorkerExit::LocalStop => "LocalStop",
            WorkerExit::CoreStopped => "CoreStopped",
            WorkerExit::HelloRejected => "HelloRejected",
        };
        stage(
            &report,
            "n3res_worker",
            exit == WorkerExit::LocalStop || exit == WorkerExit::CoreStopped,
            &format!(
                "exit={exit_name} connects={} dispatches={} events_sent={} events_acked={} gap_dropped={}",
                counters.connects,
                counters.dispatches,
                counters.events_sent,
                counters.events_acked,
                counters.events_gap_dropped,
            ),
        );
    }
    // 泵/接收线程由 stop 置位自然退出(内部 100ms 节拍);短暂宽限后返回,
    // 其收尾 stage 行随进程继续写完。线程不 detach —— 返回后随进程回收,
    // 研究注入模型下宿主进程长存,join 语义由 stop 标志保证。
    std::thread::sleep(std::time::Duration::from_millis(800));
    stage(&report, "n3res_done", true, "resident window closed");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_only_from_configured_groups() {
        let groups: HashSet<u64> = [263402786u64, 123u64].into();
        assert_eq!(chat_type_for_peer("263402786", &groups), 2);
        assert_eq!(chat_type_for_peer("123", &groups), 2);
        // 未配置群号 / 私聊 uin / 非数字 peer → unknown(零猜测)。
        assert_eq!(chat_type_for_peer("999", &groups), 0);
        assert_eq!(chat_type_for_peer("950238216", &groups), 0);
        assert_eq!(chat_type_for_peer("u_89Q8", &groups), 0);
    }

    #[test]
    fn echo_match_requires_peer_text_and_time() {
        let m = EchoMatcher::new();
        m.register("m1".into(), "263402786".into(), "hello".into());
        let t_ok = (now_ms() / 1000) as u64;
        // peer 不符。
        assert_eq!(m.match_record(Some("999"), Some("hello"), Some(t_ok)), None);
        // text 不符。
        assert_eq!(m.match_record(Some("263402786"), Some("other"), Some(t_ok)), None);
        // 缺时间戳的半成品记录不参与匹配(保守:不误确认)。
        assert_eq!(m.match_record(Some("263402786"), Some("hello"), None), None);
        // 全对 + 时间在场 → 命中并摘除。
        assert_eq!(
            m.match_record(Some("263402786"), Some("hello"), Some(t_ok)),
            Some("m1".into())
        );
        // 摘除后不重复确认。
        assert_eq!(m.match_record(Some("263402786"), Some("hello"), Some(t_ok)), None);
        assert_eq!(m.confirmed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn echo_match_time_slack_and_expiry() {
        // 平台时间早于发起(本地钟偏快)但在容差内 → 仍命中。
        let m = EchoMatcher::new();
        m.register("m2".into(), "g".into(), "t".into());
        let slightly_before = now_ms() / 1000 - 60;
        assert_eq!(
            m.match_record(Some("g"), Some("t"), Some(slightly_before as u64)),
            Some("m2".into())
        );
        // 过期清理:TTL 后的登记不再匹配。
        let m3 = EchoMatcher::new();
        m3.pending.lock().unwrap().insert(
            "old".into(),
            PendingFire {
                peer: "g".into(),
                text: "t".into(),
                since_secs: 0,
                registered_ms: now_ms().saturating_sub(ECHO_TTL_MS + 1_000),
            },
        );
        assert_eq!(m3.match_record(Some("g"), Some("t"), Some(1791639200)), None);
        assert_eq!(m3.expired.load(Ordering::Relaxed), 1);
    }
}
