//! K4-D4 常驻 resident:一次初始化、有界 owned 队列、生命周期 token、
//! owner 线程执行与确定性关闭(计划 §6.2/§6.3/§6.4/§7-D4)。
//!
//! 不变量(对照 K3 审计结论,docs/research/k4-incident-register.md §5):
//! - **每宿主代次 bootstrap 一次**;重复 bootstrap / 关闭后再 bootstrap 拒绝;
//! - **submit 任意线程可调,执行只在 owner 线程**:任何宿主 API 前
//!   (包括第一个)完成 owner/env/current 三重检查,零 current 不放行;
//! - **owned 数据**:队列只保存自有 Rust 结构,不保存宿主借用/栈指针;
//! - **生命周期 token**:回调/请求携带 token;token 过期 → 拒绝且零 native;
//! - **确定性关闭**:未派发项可取消;在途项显式 unknown;listener 对称移除
//!   并确认;关闭后计数回基线;double close 幂等;
//! - 环境失效(env invalid)→ Quarantined:停动作、保留证据,不当正常关闭。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub use crate::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult, NoHostAdapter};

/// 资源限额(计划 §6.4 建议初值;LAB 测试报告固定实际值)。
#[derive(Debug, Clone, Copy)]
pub struct ResidentLimits {
    /// 入站动作队列上限。
    pub ingress_max: usize,
    /// 出站事件队列上限。
    pub events_max: usize,
    /// 单次宿主 drain 上限;有剩余则由调用方继续调度。
    pub drain_batch: usize,
}

impl Default for ResidentLimits {
    fn default() -> Self {
        Self {
            ingress_max: 32,
            events_max: 256,
            drain_batch: 32,
        }
    }
}

/// owned 动作(跨线程提交;不持有宿主数据)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedRequest {
    /// 非发送调度(资源/生命周期验收)。
    Probe { id: u64 },
    /// 参数化发送。native 语义与状态由 core(计划 §6.6)持有;bridge 只
    /// 登记"已见请求"防重复派发。
    SendText { request_id: String, text_len: usize },
}

/// 事件来源回调(D8 去重语义依赖:onRecvMsg=接收,onMsgInfoListUpdate=更新)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSourceKind {
    Recv,
    Update,
}

/// owned 事件(宿主 → core 方向;D8 完整字段,JS 侧已 owned 拷贝)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedEvent {
    pub source: EventSourceKind,
    /// 1=私聊 2=群聊(原生 chatType,opaque 透传)。
    pub chat_type: u32,
    pub peer_uid: String,
    pub peer_uin: String,
    pub sender_uin: String,
    /// 原生消息 ID(opaque 字符串)。
    pub native_id: String,
    /// 完整正文(JS 侧上限 8000 字节,不裁剪语义由 JS 环承担)。
    pub text: String,
    /// 平台时间(原生 msgTime;取不到 None)。
    pub msg_time: Option<u64>,
}

/// 生命周期 token:回调/迟交结果必须携带;关闭后失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleToken(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResidentState {
    /// 未初始化。
    Idle,
    /// 已 bootstrap,接受提交。
    Ready,
    /// 关闭中:拒绝新提交,处理收尾。
    Stopping,
    Closed,
    /// 环境失效等不可证明安全的场景:停动作、保留证据。
    Quarantined,
}

/// 提交结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitVerdict {
    Accepted,
    /// 未初始化。
    NotReady,
    /// 关闭/隔离中,拒绝新提交。
    Closed,
    /// 队列满:明确拒绝(不静默丢弃;调用方按契约处理)。
    QueueFull,
}

/// 派发结果(worker 经 take_results 取走并回传 core;§6.6 receipt 三分类)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// native 成功;native_id 缺失时上层按"未关联"处理,不得冒充成功。
    Success { request_id: String, native_id: Option<String> },
    /// 宿主侧明确失败(如 native 服务报错)。
    Failure { request_id: String, reason: String },
    /// 结果不确定(env 失效/未知错误):执行与否不可证明。
    Unknown { request_id: String },
}

/// 关闭报告(计数回基线的验收数据)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CloseReport {
    pub cancelled_unsent: usize,
    pub marked_unknown: usize,
    pub listener_removed: Option<bool>,
    pub late_callbacks_rejected: usize,
    pub already_closed: bool,
}

/// 行为/资源计数(资源平衡验收数据源)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResidentCounters {
    pub bootstraps: u64,
    pub listener_adds: u64,
    pub listener_removes: u64,
    pub native_ops_total: u64,
    pub native_ops_rejected_precheck: u64,
    pub submitted_total: u64,
    pub dispatched_total: u64,
    pub callbacks_posted: u64,
    pub callbacks_executed: u64,
    pub callbacks_rejected_stale: u64,
    pub stale_submits_rejected: u64,
    pub cancelled_unsent: u64,
    pub marked_unknown: u64,
    pub quarantine_events: u64,
    pub events_gap_dropped: u64,
}

struct ReqRecord {
    state: ReqState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReqState {
    Queued,
    NativeStarted,
    Done,
    CancelledUnsent,
    DeliveryUnknown,
}

struct Core<A: HostAdapter> {
    adapter: A,
    state: ResidentState,
    token: LifecycleToken,
    limits: ResidentLimits,
    ingress: VecDeque<(OwnedRequest, LifecycleToken)>,
    requests: HashMap<String, ReqRecord>,
    events: VecDeque<(OwnedEvent, LifecycleToken)>,
    results_out: VecDeque<SendOutcome>,
    listener_token: Option<u64>,
    counters: ResidentCounters,
    late_rejections: usize,
}

/// 常驻 resident。`Arc<Resident>` 共享;owner 线程语义由内部 Mutex +
/// adapter 的线程检查共同保证(**Mutex 只串行化数据结构;owner 检查在
/// 每次 native 前独立进行**,不因持锁而豁免)。
pub struct Resident<A: HostAdapter> {
    core: Mutex<Core<A>>,
}

impl<A: HostAdapter> Resident<A> {
    pub fn new(adapter: A, limits: ResidentLimits) -> Self {
        Self {
            core: Mutex::new(Core {
                adapter,
                state: ResidentState::Idle,
                token: LifecycleToken(0),
                limits,
                ingress: VecDeque::new(),
                requests: HashMap::new(),
                events: VecDeque::new(),
                results_out: VecDeque::new(),
                listener_token: None,
                counters: ResidentCounters::default(),
                late_rejections: 0,
            }),
        }
    }

    /// 一次性初始化(计划 §6.1:指定宿主初始化一次)。
    ///
    /// 检查顺序:**先** owner/env/current,后任何宿主 API(ListenerAdd)。
    pub fn bootstrap(&self) -> Result<LifecycleToken, HostError> {
        let mut c = self.core.lock().unwrap();
        if c.state != ResidentState::Idle {
            return Err(HostError::Native { code: 1 }); // 已初始化:每代次一次
        }
        c.precheck()?;
        let token = LifecycleToken(next_token());
        match c.adapter.native_op(HostOp::ListenerAdd)? {
            HostOpResult::ListenerAdded { token: ltoken } => {
                c.listener_token = Some(ltoken);
            }
            // 延迟模式(D7):无消息监听可注册;关闭时不做对称移除。
            HostOpResult::ListenerDeferred => {
                c.listener_token = None;
            }
            _ => return Err(HostError::Native { code: 2 }),
        }
        c.counters.listener_adds += 1;
        c.counters.native_ops_total += 1;
        c.counters.bootstraps += 1;
        c.token = token;
        c.state = ResidentState::Ready;
        Ok(token)
    }

    /// 任意线程提交 owned 动作(TSFN 语义:跨线程只入队,不执行)。
    pub fn submit(&self, req: OwnedRequest) -> SubmitVerdict {
        let mut c = self.core.lock().unwrap();
        c.counters.submitted_total += 1;
        match c.state {
            ResidentState::Ready => {}
            ResidentState::Idle => return SubmitVerdict::NotReady,
            ResidentState::Stopping | ResidentState::Closed | ResidentState::Quarantined => {
                c.counters.stale_submits_rejected += 1;
                return SubmitVerdict::Closed;
            }
        }
        // 已见请求防重复派发(计划 §6.6:相同 ID 不再次执行)。
        if let OwnedRequest::SendText { request_id, .. } = &req {
            if c.requests.contains_key(request_id) {
                return SubmitVerdict::Accepted; // 查询语义:不重复入队
            }
            c.requests.insert(
                request_id.clone(),
                ReqRecord { state: ReqState::Queued },
            );
        }
        if c.ingress.len() >= c.limits.ingress_max {
            return SubmitVerdict::QueueFull;
        }
        let tok = c.token;
        c.ingress.push_back((req, tok));
        SubmitVerdict::Accepted
    }

    /// 宿主投递回调(携带捕获时的生命周期 token)。
    pub fn deliver_callback(&self, token: LifecycleToken, ev: OwnedEvent) -> Result<(), HostError> {
        let mut c = self.core.lock().unwrap();
        c.counters.callbacks_posted += 1;
        if c.token != token || !matches!(c.state, ResidentState::Ready | ResidentState::Stopping) {
            c.counters.callbacks_rejected_stale += 1;
            c.late_rejections += 1;
            return Err(HostError::EnvInvalid); // 旧代次回调:拒绝,零 native
        }
        if c.events.len() >= c.limits.events_max {
            // 有界队列:满 → 拒绝该回调(宿主侧重放/Gap 由契约处理)。
            return Err(HostError::Native { code: 3 });
        }
        c.events.push_back((ev, token));
        Ok(())
    }

    /// owner 线程 drain:每项处理前独立完成 owner/env/current 检查。
    /// 返回本轮处理数(≤ drain_batch);有剩余时调用方应继续调度,
    /// **不能等下一条业务消息才唤醒**(计划 §6.4)。
    pub fn drain(&self) -> Result<usize, HostError> {
        let mut c = self.core.lock().unwrap();
        if !matches!(c.state, ResidentState::Ready | ResidentState::Stopping) {
            return Ok(0);
        }
        let mut processed = 0usize;
        while processed < c.limits.drain_batch {
            // 每项前检查(§5.1:第一个宿主 API 之前)。
            if let Err(e) = c.precheck() {
                if processed == 0 {
                    return Err(e);
                }
                return Ok(processed);
            }
            let Some((req, tok)) = c.ingress.pop_front() else {
                break;
            };
            if tok != c.token {
                // 旧代次请求:拒绝且零 native。
                c.counters.stale_submits_rejected += 1;
                continue;
            }
            let op = match &req {
                OwnedRequest::Probe { .. } => HostOp::Probe,
                OwnedRequest::SendText { request_id, text_len } => {
                    if let Some(rec) = c.requests.get_mut(request_id) {
                        rec.state = ReqState::NativeStarted;
                    }
                    HostOp::SendText { text_len: *text_len }
                }
            };
            let r = c.adapter.native_op(op);
            c.counters.native_ops_total += 1;
            match &r {
                Ok(HostOpResult::Sent { native_id }) => {
                    c.counters.dispatched_total += 1;
                    if let OwnedRequest::SendText { request_id, .. } = &req {
                        if let Some(rec) = c.requests.get_mut(request_id) {
                            rec.state = ReqState::Done;
                        }
                        c.results_out.push_back(SendOutcome::Success {
                            request_id: request_id.clone(),
                            native_id: native_id.clone(),
                        });
                    }
                }
                Ok(_) => {
                    c.counters.dispatched_total += 1;
                }
                Err(HostError::Native { code }) => {
                    // 宿主明确报错:执行了但失败(Failure),不是 unknown。
                    if let OwnedRequest::SendText { request_id, .. } = &req {
                        if let Some(rec) = c.requests.get_mut(request_id) {
                            rec.state = ReqState::Done;
                        }
                        c.results_out.push_back(SendOutcome::Failure {
                            request_id: request_id.clone(),
                            reason: format!("native code {code}"),
                        });
                    }
                }
                Err(_) => {
                    // env 失效/线程/上下文问题:执行与否不确定。
                    if let OwnedRequest::SendText { request_id, .. } = &req {
                        if let Some(rec) = c.requests.get_mut(request_id) {
                            rec.state = ReqState::DeliveryUnknown;
                        }
                        c.results_out.push_back(SendOutcome::Unknown {
                            request_id: request_id.clone(),
                        });
                    }
                }
            }
            processed += 1;
        }
        Ok(processed)
    }

    /// owner 线程消费出站事件。
    pub fn take_events(&self) -> Vec<OwnedEvent> {
        let mut c = self.core.lock().unwrap();
        c.events.drain(..).map(|(ev, _)| ev).collect()
    }

    /// 消费派发结果(worker 回传 core;先落账后 ACK 由 core 负责)。
    pub fn take_results(&self) -> Vec<SendOutcome> {
        let mut c = self.core.lock().unwrap();
        c.results_out.drain(..).collect()
    }

    /// 宿主(pump/D8 排空)注入 owned 事件;有界窗口满 → 计数丢弃(不静默)。
    pub fn ingest_event(&self, ev: OwnedEvent) -> bool {
        let mut c = self.core.lock().unwrap();
        if !matches!(c.state, ResidentState::Ready | ResidentState::Stopping) {
            return false;
        }
        if c.events.len() >= c.limits.events_max {
            c.counters.events_gap_dropped += 1;
            return false;
        }
        let tok = c.token;
        c.events.push_back((ev, tok));
        true
    }

    pub fn counters(&self) -> ResidentCounters {
        self.core.lock().unwrap().counters.clone()
    }

    pub fn pending_ingress(&self) -> usize {
        self.core.lock().unwrap().ingress.len()
    }

    /// 正常关闭(owner 线程):取消未执行、显式化在途、对称移除监听器。
    /// double close 幂等。
    pub fn close(&self) -> Result<CloseReport, HostError> {
        let mut c = self.core.lock().unwrap();
        let mut report = CloseReport {
            already_closed: matches!(c.state, ResidentState::Closed),
            ..Default::default()
        };
        if c.state == ResidentState::Closed {
            return Ok(report);
        }
        if c.state == ResidentState::Quarantined {
            // 隔离态不允许"洗白"为正常关闭。
            return Err(HostError::EnvInvalid);
        }
        c.state = ResidentState::Stopping;
        // 1) 取消从未执行的入站项(可证明未派发)。
        while let Some((req, _)) = c.ingress.pop_front() {
            if let OwnedRequest::SendText { request_id, .. } = &req {
                if let Some(rec) = c.requests.get_mut(request_id) {
                    if rec.state == ReqState::Queued {
                        rec.state = ReqState::CancelledUnsent;
                        report.cancelled_unsent += 1;
                        c.counters.cancelled_unsent += 1;
                    }
                }
            } else {
                report.cancelled_unsent += 1;
            }
        }
        // 2) 在途(native 已开始、结果未回)→ 显式 unknown;不宣称未发送。
        let mut unknown_now: u64 = 0;
        for rec in c.requests.values_mut() {
            if rec.state == ReqState::NativeStarted {
                rec.state = ReqState::DeliveryUnknown;
                unknown_now += 1;
            }
        }
        report.marked_unknown = unknown_now as usize;
        c.counters.marked_unknown += unknown_now;
        // 3) 对称移除监听器(owner 线程;必须确认结果)。
        if let Some(tok) = c.listener_token.take() {
            // 关闭中的 precheck:环境可能已坏;坏了则进 Quarantined。
            if let Err(e) = c.precheck() {
                c.quarantine();
                return Err(e);
            }
            match c.adapter.native_op(HostOp::ListenerRemove { token: tok }) {
                Ok(HostOpResult::ListenerRemoved) => {
                    c.counters.listener_removes += 1;
                    c.counters.native_ops_total += 1;
                    report.listener_removed = Some(true);
                }
                Ok(_) => {
                    report.listener_removed = Some(false);
                }
                Err(e) => {
                    // 移除失败:无法证明清理完成 → Quarantined。
                    c.quarantine();
                    return Err(e);
                }
            }
        }
        // 4) 残余出站事件丢弃计数已在 late_rejections 体现;队列清空。
        c.late_rejections += c.events.len();
        c.events.clear();
        c.state = ResidentState::Closed;
        Ok(report)
    }

    /// 隔离(计划 §6.2):停动作、停自动恢复、保留证据;不可当作正常关闭。
    pub fn quarantine_reason(&self) -> bool {
        self.core.lock().unwrap().state == ResidentState::Quarantined
    }

    /// 当前生命周期 token(宿主捕获回调用)。
    pub fn token(&self) -> LifecycleToken {
        self.core.lock().unwrap().token
    }

    /// 已见请求状态(查询语义;与 core 的 ActionState 正交)。
    pub fn request_state(&self, request_id: &str) -> Option<&'static str> {
        let c = self.core.lock().unwrap();
        c.requests.get(request_id).map(|r| match r.state {
            ReqState::Queued => "queued",
            ReqState::NativeStarted => "native_started",
            ReqState::Done => "done",
            ReqState::CancelledUnsent => "cancelled_unsent",
            ReqState::DeliveryUnknown => "delivery_unknown",
        })
    }
}

impl<A: HostAdapter> Core<A> {
    /// 一切宿主 API 之前的三重检查:owner 线程 → env 有效 → current 上下文。
    /// **检查顺序固定;零/空 current 不得放行**(计划 §5.1)。
    fn precheck(&mut self) -> Result<(), HostError> {
        if self.adapter.current_thread_id() != self.adapter.owner_thread_id() {
            self.counters.native_ops_rejected_precheck += 1;
            return Err(HostError::NotOwnerThread);
        }
        if !self.adapter.env_valid() {
            self.counters.native_ops_rejected_precheck += 1;
            return Err(HostError::EnvInvalid);
        }
        if !self.adapter.current_context_ok() {
            self.counters.native_ops_rejected_precheck += 1;
            return Err(HostError::NoCurrentContext);
        }
        Ok(())
    }

    fn quarantine(&mut self) {
        self.state = ResidentState::Quarantined;
        self.counters.quarantine_events += 1;
    }
}

/// 全局 token 空间(进程内唯一,跨 Resident 实例不重复)。
fn next_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
