//! 自有 MSF adapter(计划 §7-P4;路线决定:`k4-native-route-decision.md`)。
//!
//! 职责边界(计划 §3):对象句柄、合法调度、SSO 请求与原生结果、失效与
//! 停止。消息 protobuf/业务回执在进程外 Rust 层(P5),本模块只搬运不透明
//! 字节。**在 G1 通过前,真实发送不接线**——真实 QQ 入口都经 [`NativeLinks`]
//! 提供,生产构造恒 `links=None`(拒绝占位);LAB 由假宿主注入。
//!
//! 锁范围合同(计划 §7-P4):提交路径不持有任何可被 callback 重入的锁 ——
//! claim(代次核对+快照)→ 无锁调用边界 → 结果走独立有界通道。
//! 回调边界 panic 隔离(`native_abi::callback_boundary`)。

use core::ffi::c_void;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use super::native_abi::{self, CallbackMgrLayout, ServicePair, TaggedString24, TransportSubmitFn};
use super::native_handle::{HandleStats, ServiceHandle};

/// 真实 QQ 链接(G1 前恒 None;字段名即证据锚点)。
#[derive(Debug, Clone, Copy)]
pub struct NativeLinks {
    /// transport slot7(0x1B4E4EC;实际目标,R3.1 现场闭合)。
    pub transport_submit: TransportSubmitFn,
}

/// 调度许可(计划 §3:合法调度合同)。G1 前 LAB 用 `unrestricted`;
/// 生产由宿主接线方声明提交线程集,并经 G1 终验。
#[derive(Debug, Clone)]
pub struct SchedulePermit {
    pub allowed_threads: Vec<u64>,
    pub unrestricted_threads: bool,
}

impl SchedulePermit {
    pub fn unrestricted() -> Self {
        Self { allowed_threads: Vec::new(), unrestricted_threads: true }
    }
    pub fn admits(&self, tid: u64) -> bool {
        self.unrestricted_threads || self.allowed_threads.contains(&tid)
    }
}

/// 入口级提交反馈。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitVerdict {
    /// 已调用入口,拿到非零 token。
    Submitted { token: u64 },
    /// transport 未连接(slot7 同步失败路径:ret=0 且 token=0;可探测 not-ready)。
    NotConnected,
    /// 代次/线程/构建任一前置不满足 —— 未触碰原生(T16)。
    RejectedPreCall(&'static str),
    /// 真实链接未接线(G1 前)。
    LinksUnwired,
    /// 命令超短形容量(长形分配路径未定证,拒绝占位)。
    UnsupportedLongCommand,
}

/// 原生完成(回调边界复制后的拥有数据;三分类)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeOutcome {
    Success { request_id: String, native_id: Option<String> },
    Failure { request_id: String, reason: String },
    Unknown { request_id: String },
}

/// 结果通道上限(items;计划 §7-P4:有界)。
pub const OUTCOME_MAX_ITEMS: usize = 256;

/// adapter 计数(审计;LAB 断言数据源)。
#[derive(Debug, Default)]
pub struct MsfCounters {
    pub probes: AtomicU64,
    pub submits_attempted: AtomicU64,
    pub submits_rejected_precall: AtomicU64,
    pub submits_invoked: AtomicU64,
    pub not_connected: AtomicU64,
    pub callbacks_copied: AtomicU64,
    pub callbacks_dropped_overflow: AtomicU64,
    pub callback_panics: AtomicU64,
    pub duplicate_outcomes: AtomicU64,
    pub stale_callbacks_audited: AtomicU64,
}

/// 回调共享态(adapter 与回调线程两侧)。
struct Shared {
    tx: mpsc::SyncSender<NativeOutcome>,
    seen_done: Mutex<VecDeque<String>>,
    /// 已提交(拿到非零 token)但未终态的请求(T14 分类账本)。
    submitted: Mutex<VecDeque<String>>,
    stale: Arc<AtomicBool>,
    counters: Arc<MsfCounters>,
    host_generation: u64,
    session_generation: u64,
}

impl Shared {
    /// 统一投递:失效审计 → 去重幂等 → 有界通道(溢出显式计数)。
    fn deliver(self: &Arc<Self>, outcome: NativeOutcome) {
        if self.stale.load(Ordering::Relaxed) {
            self.counters.stale_callbacks_audited.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut seen = self.seen_done.lock().unwrap();
        if seen.iter().any(|id| id == outcome.request_id()) {
            self.counters.duplicate_outcomes.fetch_add(1, Ordering::Relaxed);
            return;
        }
        seen.push_back(outcome.request_id().to_string());
        if seen.len() > 1024 {
            seen.pop_front();
        }
        drop(seen);
        // 终态到达:移出在途账本(两条回调入口共用;T14)。
        self.submitted.lock().unwrap().retain(|id| id != outcome.request_id());
        match self.tx.try_send(outcome) {
            Ok(()) => {
                self.counters.callbacks_copied.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::TrySendError::Full(_)) => {
                self.counters.callbacks_dropped_overflow.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }
}

// SAFETY: Shared 的字段均为并发安全或只读原子;counters 指向 adapter
// 存续期内的原子(resident 层保证 adapter 活过全部在途回调;LAB 由测试
// 结构保证)。
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

/// MSF adapter。
pub struct MsfAdapter {
    handle: ServiceHandle,
    links: Option<NativeLinks>,
    permit: SchedulePermit,
    /// transport 对象(现场证据:this+0x60 的值;假宿主注入;G1 前生产为 null)。
    /// 其 +0x60 处为内联连接 pair(1B4E4EC 锁定;R3.1 §10.1)。
    transport: *mut c_void,
    shared: Arc<Shared>,
    /// 独立完成通道的接收端(try_recv 为 &self;resident/测试排空)。
    rx: mpsc::Receiver<NativeOutcome>,
    /// 宿主代次失效后置位:一切回调只审计(T13)。
    stale: Arc<AtomicBool>,
    pub counters: Arc<MsfCounters>,
    stats_ptr: *mut HandleStats,
}

// SAFETY: transport/handle 宿主以代次失效保证安全;LAB 单线程。
unsafe impl Send for MsfAdapter {}

impl MsfAdapter {
    /// 构造。返回 adapter 与结果接收端(rx 恰取一次)。
    ///
    /// # Safety
    /// `transport` 须为有效 transport 对象(this+0x60 的值;假宿主注入),
    /// 或 null(此时一切提交被拒)。
    pub unsafe fn new(
        handle: ServiceHandle,
        transport: *mut c_void,
        links: Option<NativeLinks>,
        permit: SchedulePermit,
        stats_ptr: *mut HandleStats,
    ) -> Self {
        let counters = Arc::new(MsfCounters::default());
        let (tx, rx) = mpsc::sync_channel(OUTCOME_MAX_ITEMS);
        let shared = Arc::new(Shared {
            tx,
            seen_done: Mutex::new(VecDeque::new()),
            submitted: Mutex::new(VecDeque::new()),
            stale: Arc::new(AtomicBool::new(false)),
            counters: Arc::clone(&counters),
            host_generation: handle.generations().0,
            session_generation: handle.generations().1,
        });
        let adapter = Self {
            handle,
            transport,
            links,
            permit,
            shared: Arc::clone(&shared),
            stale: Arc::clone(&shared.stale),
            rx,
            counters,
            stats_ptr,
        };
        adapter
    }

    /// 结果接收端的借用视图(供外部线程阻塞等待;与 take_outcomes 互斥使用)。
    pub fn outcomes_rx(&self) -> &mpsc::Receiver<NativeOutcome> {
        &self.rx
    }

    fn current_tid() -> u64 {
        // SAFETY: 无副作用。
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
    }

    pub fn generations(&self) -> (u64, u64) {
        self.handle.generations()
    }

    /// 就绪探测:transport 存在性(**不发送**)。G1 的可观察 not-ready 信号
    /// 由 slot7 的同步失败路径给出(`submit_send` 的 NotConnected)。
    pub fn probe_ready(&self) -> bool {
        self.counters.probes.fetch_add(1, Ordering::Relaxed);
        !self.transport.is_null() && self.links.is_some()
    }

    /// 提交发送(claim → 无锁调用边界 → 独立完成通道)。
    pub fn submit_send(&self, request_id: &str, command: &[u8], req_body: &[u8]) -> SubmitVerdict {
        // —— claim:全部前置核对;任何失败都不触碰原生(T16)——
        let Some(links) = self.links else {
            return SubmitVerdict::LinksUnwired;
        };
        if !self.permit.admits(Self::current_tid()) {
            self.counters.submits_rejected_precall.fetch_add(1, Ordering::Relaxed);
            return SubmitVerdict::RejectedPreCall("thread not admitted");
        }
        if self.transport.is_null() {
            self.counters.submits_rejected_precall.fetch_add(1, Ordering::Relaxed);
            return SubmitVerdict::RejectedPreCall("transport null");
        }
        let Some(mut cmd) = native_abi::tagged_short(command) else {
            self.counters.submits_rejected_precall.fetch_add(1, Ordering::Relaxed);
            return SubmitVerdict::UnsupportedLongCommand;
        };
        self.counters.submits_attempted.fetch_add(1, Ordering::Relaxed);

        // 回调包装:capture 携带 Arc 共享态(去重/通道/代次)。
        let ctx = Box::into_raw(Box::new(CallbackCtx {
            request_id: request_id.to_string(),
            shared: Arc::clone(&self.shared),
        }));
        let mut mgr = CallbackMgrLayout {
            storage: [0; 0x10],
            manager: Some(cb_manager_op),
            invoker: Some(cb_invoker),
            capture: ctx as *mut c_void,
        };

        // —— 无锁调用边界:以下不持有任何可被回调重入的锁 ——
        let mut body = native_abi::ByteRange24::from_slice(req_body);
        let mut token: u64 = 0;
        let storage = [0u8; 0x40];
        self.counters.submits_invoked.fetch_add(1, Ordering::Relaxed);
        // SAFETY: 对象按 native_abi 布局构造;transport 由宿主保证有效;
        // 输入的同步复制在入口调用内完成(R2 §2 约束);panic 已隔离。
        let ret = unsafe {
            (links.transport_submit)(
                self.transport,
                &mut cmd,
                &mut body,
                &mut mgr,
                &mut token,
                storage.as_ptr() as *mut c_void,
            )
        };
        if ret == 0 && token == 0 {
            // 同步失败(未连接/状态未就绪):ctx 不会被回调,立即回收。
            // SAFETY: 同步失败路径已证不回调(R3.1 §10.1)。
            unsafe { drop(Box::from_raw(ctx)) };
            self.counters.not_connected.fetch_add(1, Ordering::Relaxed);
            return SubmitVerdict::NotConnected;
        }
        self.shared.submitted.lock().unwrap().push_back(request_id.to_string());
        SubmitVerdict::Submitted { token }
    }

    /// 排空完成通道(独立完成通道的消费者;不阻塞)。
    pub fn take_outcomes(&self) -> Vec<NativeOutcome> {
        let mut out = Vec::new();
        while let Ok(o) = self.rx.try_recv() {
            out.push(o);
        }
        out
    }

    /// 回调线程入口(dispatcher worker 侧):复制→去重→通道(T11/T13)。
    /// 旧代次只审计;重复幂等;panic 隔离并计数。
    pub fn on_native_callback(&self, request_id: &str, status: i32, reason: &[u8], native_id: Option<&str>) {
        self.deliver(request_id, status, reason, native_id);
    }

    fn deliver(&self, request_id: &str, status: i32, reason: &[u8], native_id: Option<&str>) {
        let shared = Arc::clone(&self.shared);
        let rid = request_id.to_string();
        let reason_owned = reason.to_vec();
        let native_owned = native_id.map(|s| s.to_string());
        let outcome = native_abi::callback_boundary(move || {
            if status == 0 {
                NativeOutcome::Success { request_id: rid, native_id: native_owned }
            } else if status < 0 {
                NativeOutcome::Unknown { request_id: rid }
            } else {
                NativeOutcome::Failure {
                    request_id: rid,
                    reason: String::from_utf8_lossy(&reason_owned).into_owned(),
                }
            }
        });
        let Some(outcome) = outcome else {
            self.counters.callback_panics.fetch_add(1, Ordering::Relaxed);
            return;
        };
        shared.deliver(outcome);
    }


    /// 宿主代次失效通知:此后一切回调只审计,不再进入当前业务链(T13)。
    pub fn mark_stale(&self) {
        self.stale.store(true, Ordering::Relaxed);
    }

    /// 已完成 request_id 镜像(T14 分类账本的输入)。
    pub fn completed_ids(&self) -> Vec<String> {
        self.shared.seen_done.lock().unwrap().iter().cloned().collect()
    }

    /// 在途(已提交未终态)请求(T14):关闭时逐项分类 Unknown。
    pub fn pending_ids(&self) -> Vec<String> {
        let submitted = self.shared.submitted.lock().unwrap();
        let done = self.shared.seen_done.lock().unwrap();
        submitted.iter().filter(|id| !done.contains(id)).cloned().collect()
    }

}

impl NativeOutcome {
    pub fn request_id(&self) -> &str {
        match self {
            NativeOutcome::Success { request_id, .. }
            | NativeOutcome::Failure { request_id, .. }
            | NativeOutcome::Unknown { request_id } => request_id,
        }
    }
}

fn rx_out(rx: mpsc::Receiver<NativeOutcome>) -> mpsc::Receiver<NativeOutcome> {
    rx
}

// —— 回调侧(机器级形状;真实 dispatcher 以这些签名调用) ——

struct CallbackCtx {
    request_id: String,
    shared: Arc<Shared>,
}

/// manager operation(op=0 销毁/移动自清理):释放 capture。
unsafe extern "system" fn cb_manager_op(mgr: *mut c_void, operation: u32) {
    // SAFETY: mgr 由 submit_send 的 Box::into_raw 提供。
    unsafe {
        let m = mgr as *mut CallbackMgrLayout;
        if operation == 0 && !(*m).capture.is_null() {
            drop(Box::from_raw((*m).capture as *mut CallbackCtx));
            (*m).capture = core::ptr::null_mut();
        }
    }
}

/// invoker(五参形状,R2 §3):rcx=nested callback, edx=常数, r8d=status,
/// r9=24 字节 tagged reason(调用期内有效), stack5=24 字节 body 容器。
/// **边界内立即复制为拥有数据**,不经返回保存裸指针。
unsafe extern "system" fn cb_invoker(
    invoker: *mut c_void,
    _nested: usize,
    status: u32,
    reason: *mut c_void,
    body: *mut c_void,
) {
    // SAFETY: capture 由 manager 协议管理;panic 不跨边界。
    unsafe {
        let m = invoker as *mut CallbackMgrLayout;
        let ctx = (*m).capture as *const CallbackCtx;
        if ctx.is_null() {
            return;
        }
        // 复制 reason(short-form;long-form 未定证 → 空处理并维持形状)。
        let reason_owned: Vec<u8> = if reason.is_null() {
            Vec::new()
        } else {
            let t = &*(reason as *const TaggedString24);
            t.bytes().to_vec()
        };
        let body_len = if body.is_null() {
            0usize
        } else {
            let r = &*(body as *const native_abi::ByteRange24);
            r.len()
        };
        // body 内容此处只计长(P5 解码属于拥有层;真实容器读取 P6 前定证)。
        let _ = body_len;
        let outcome = if status == 0 {
            NativeOutcome::Success {
                request_id: (*ctx).request_id.clone(),
                native_id: None, // 稳定业务身份关联属 P5;不伪造
            }
        } else if (status as i32) < 0 {
            NativeOutcome::Unknown { request_id: (*ctx).request_id.clone() }
        } else {
            NativeOutcome::Failure {
                request_id: (*ctx).request_id.clone(),
                reason: String::from_utf8_lossy(&reason_owned).into_owned(),
            }
        };
        (*ctx).shared.deliver(outcome);
    }
}
