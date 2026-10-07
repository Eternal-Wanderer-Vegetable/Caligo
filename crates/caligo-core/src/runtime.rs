//! K4-D3 账号 actor:生命周期、身份验证、动作登记与恢复(计划 §6.2/§6.6)。
//!
//! 单 owner 结构:`AccountActor` 的全部状态迁移经 `&mut self` 方法串行发生,
//! 不设内部锁 —— 并发由上层(transport/bridge client, D5)在其 owner 上
//! 串行化。核心不变量:
//! - **准入两层**:调度前(`submit_send`)与 native 调用前(`dispatch_begin`
//!   之后的 bridge 侧校验,LAB 由夹具实现)各有一层身份/代次校验;
//! - **先持久,后生效**:请求先落 journal 才算 accepted;事件先落 journal
//!   才向 bridge 返回 EventAck;任何不确定窗口显式化为 `DeliveryUnknown`;
//! - **不自动重发**:重启后非终态请求只进入 `pending_queries`,由调用方
//!   逐项 QueryRequest;本模块不存在任何"再派发"路径;
//! - **代次隔离**:旧代次请求/事件只进入审计,不进入当前业务链。

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::journal::{Journal, JournalError, JournalRecord, RequestRecord, GapRecord, SyncMode};
use caligo_model::{
    ActionState, ConnectionIdentity, Direction, EventSource, HostIdentity, MsgEvent, NativeMessageId,
    Phase, RejectReason, SendTextRequest, SessionIdentity, SessionKey, SessionKind,
};

/// 运行时参数(计划 §6.4/§6.6 建议初值;实测后于 D11 报告固定)。
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub event_queue_max_items: usize,
    pub event_queue_max_bytes: usize,
    pub action_queue_max: usize,
    pub journal_max_bytes: u64,
    /// 事件去重窗口容量(按 (native_id, 来源类) 计)。
    pub dedup_capacity: usize,
    pub journal_sync: SyncMode,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            event_queue_max_items: 256,
            event_queue_max_bytes: 8 * 1024 * 1024,
            action_queue_max: 32,
            journal_max_bytes: crate::journal::DEFAULT_MAX_BYTES,
            dedup_capacity: 65_536,
            journal_sync: SyncMode::FlushOnly,
        }
    }
}

/// 打开失败。中段损坏时调用方不得删除/重建文件(计划 §7-D3 失败分支)。
#[derive(Debug)]
pub enum OpenError {
    Journal(JournalError),
}

impl core::fmt::Display for OpenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OpenError::Journal(e) => write!(f, "{e}"),
        }
    }
}

/// 打开后的恢复摘要。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OpenReport {
    pub recovered_requests: usize,
    pub recovered_events: usize,
    pub recovered_gaps: usize,
    pub recovered_receipt_audits: usize,
    /// 重启后需要逐项 QueryRequest 的非终态请求(不自动重发)。
    pub pending_queries: Vec<String>,
    pub journal_tail_truncated_bytes: u64,
    pub transition_anomalies: usize,
}

/// bridge 上报的 native 结果(计划 §6.6 receipt 契约的三分类)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeResult {
    /// QQ 服务明确成功;`native_id` 为实际原生消息身份 —— **拿不到关联时
    /// 不得上报 Success**(契约:仅 Promise/生成 ID 不构成成功)。
    Success { native_id: Option<NativeMessageId> },
    Failure { reason: String },
    /// 结果丢失/不确定:dispatch 窗口崩溃等。
    Unknown,
}

/// 取消结果:只有"可证明未执行"才算 cancelled_unsent。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    CancelledUnsent,
    /// 已进入派发/native 阶段,无法证明未执行 → 不取消,等待回执或 unknown。
    TooLate,
    NotFound,
}

/// 事件受理结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventReceipt {
    /// 已持久化并进入交付队列(即对 bridge 的 EventAck 依据)。
    Published { event_seq: u64 },
    /// 重复(同 native_id 的重复接收/更新)已抑制。
    DuplicateSuppressed,
    /// 旧代次证据:已审计持久化,不进入当前业务链。
    AuditedOldGeneration { event_seq: u64 },
}

/// 迟到/旧代次/重复回执的审计结果(不重开动作)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Applied,
    /// 终态上的重复回执:幂等忽略。
    DuplicateIgnored,
    /// DeliveryUnknown 收到明确结果:更正当前已知结果(不重开、不再发送)。
    CorrectedFromUnknown,
    /// 未知请求/旧代次:仅审计。
    AuditedOnly,
}

/// 行为/资源计数(资源有界性验收的数据来源)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RuntimeCounters {
    pub sends_submitted: u64,
    pub sends_accepted: u64,
    pub sends_cancelled_unsent: u64,
    pub dispatch_intents: u64,
    pub native_started: u64,
    pub results_success: u64,
    pub results_failure: u64,
    pub results_unknown: u64,
    pub requests_duplicate_query: u64,
    pub events_persisted: u64,
    pub events_duplicate_suppressed: u64,
    pub events_old_generation_audited: u64,
    pub gaps_recorded: u64,
    pub receipts_duplicate_ignored: u64,
    pub receipts_corrected: u64,
    pub receipts_audited_only: u64,
    pub transition_anomalies: u64,
    pub journal_failures: u64,
}

/// 折叠后的请求条目。
#[derive(Debug, Clone)]
struct RequestEntry {
    payload_hash: u64,
    state: ActionState,
    session_generation: u64,
    target: SessionKey,
    deadline_ms: u64,
    history: Vec<(String, u64)>,
}

/// 接收审计(迟到/旧代次回执、Gap)保留有界样本供查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptAuditEntry {
    pub request_id: String,
    pub session_generation: u64,
    pub detail: String,
    pub at_unix_ms: u64,
}

// —— SessionKey 的 JSON DTO(model 保持零依赖,序列化镜像放本模块) ——

#[derive(Serialize, Deserialize)]
struct SessionKeyDto {
    account: String,
    kind: String,
    peer: String,
}

fn session_to_dto_value(key: &SessionKey) -> serde_json::Value {
    serde_json::to_value(SessionKeyDto {
        account: key.account.0.clone(),
        kind: key.kind.to_string(),
        peer: key.peer.0.clone(),
    })
    .unwrap_or(serde_json::Value::Null)
}

/// 请求查询摘要(QueryRequest 的 core 侧载荷)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSummary {
    pub state: ActionState,
    pub session_generation: u64,
    pub target_repr: String,
    pub deadline_ms: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 账号 actor(计划 §6.2 状态机的 owner)。
pub struct AccountActor {
    pub(crate) phase: Phase,
    session: Option<SessionIdentity>,
    conn: Option<ConnectionIdentity>,
    journal: Journal,
    requests: HashMap<String, RequestEntry>,
    pending: VecDeque<String>,
    events_out: VecDeque<MsgEvent>,
    events_out_bytes: usize,
    event_seq_next: u64,
    delivered_cursor: u64,
    seen_native: HashSet<String>,
    seen_updates: HashSet<String>,
    dedup_fifo: VecDeque<(bool, String)>, // (is_update, native_id),FIFO 驱逐
    dedup_capacity: usize,
    receipt_audits: VecDeque<ReceiptAuditEntry>,
    gaps: VecDeque<GapRecord>,
    pub counters: RuntimeCounters,
    degraded_reason: Option<String>,
    config: RuntimeConfig,
}

impl AccountActor {
    /// 打开并折叠 journal。**中段损坏 → Err**,调用方必须保留原文件并转
    /// Degraded 处置(本模块不做任何删除/重建)。
    pub fn open(
        journal_path: impl Into<std::path::PathBuf>,
        config: RuntimeConfig,
    ) -> Result<(Self, OpenReport), OpenError> {
        let (journal, recovery) =
            Journal::open(journal_path, config.journal_max_bytes, config.journal_sync)
                .map_err(OpenError::Journal)?;
        let mut actor = Self {
            phase: Phase::Detached,
            session: None,
            conn: None,
            journal,
            requests: HashMap::new(),
            pending: VecDeque::new(),
            events_out: VecDeque::new(),
            events_out_bytes: 0,
            event_seq_next: 1,
            delivered_cursor: 0,
            seen_native: HashSet::new(),
            seen_updates: HashSet::new(),
            dedup_fifo: VecDeque::new(),
            dedup_capacity: config.dedup_capacity,
            receipt_audits: VecDeque::new(),
            gaps: VecDeque::new(),
            counters: RuntimeCounters::default(),
            degraded_reason: None,
            config,
        };
        let mut report = OpenReport {
            journal_tail_truncated_bytes: recovery.tail_truncated_bytes,
            ..Default::default()
        };
        for rec in recovery.records {
            match rec {
                JournalRecord::Request(r) => {
                    actor.fold_request(r, &mut report);
                }
                JournalRecord::Event(e) => {
                    report.recovered_events += 1;
                    actor.fold_event(e);
                }
                JournalRecord::EventDelivered { event_seq } => {
                    actor.delivered_cursor = actor.delivered_cursor.max(event_seq);
                }
                JournalRecord::Gap(g) => {
                    report.recovered_gaps += 1;
                    actor.gaps.push_back(g);
                    if actor.gaps.len() > 256 {
                        actor.gaps.pop_front();
                    }
                }
                JournalRecord::ReceiptAudit(a) => {
                    report.recovered_receipt_audits += 1;
                    actor.receipt_audits.push_back(ReceiptAuditEntry {
                        request_id: a.request_id,
                        session_generation: a.session_generation,
                        detail: a.detail,
                        at_unix_ms: a.at_unix_ms,
                    });
                    if actor.receipt_audits.len() > 1024 {
                        actor.receipt_audits.pop_front();
                    }
                }
            }
        }
        report.pending_queries = actor.pending.iter().cloned().collect();
        report.recovered_requests = actor.requests.len();
        Ok((actor, report))
    }

    fn fold_request(&mut self, r: RequestRecord, report: &mut OpenReport) {
        let mut state = match state_from_name(&r.state) {
            Some(s) => s,
            None => {
                self.counters.transition_anomalies += 1;
                report.transition_anomalies += 1;
                return;
            }
        };
        // 折叠时还原实际原生消息身份(ConfirmedSuccess 的关联是契约字段)。
        if let ActionState::ConfirmedSuccess { native_id } = &mut state {
            if native_id.is_none() {
                *native_id = r.native_id.clone().map(NativeMessageId);
            }
        }
        match self.requests.get_mut(&r.request_id) {
            None => {
                // 首见即终态的审计型记录(DeliveryUnknown / CancelledUnsent)
                // 也合法:崩溃窗口内 core 写了终态、无其他记录。
                let terminal = state.is_terminal();
                self.requests.insert(
                    r.request_id.clone(),
                    RequestEntry {
                        payload_hash: r.payload_hash,
                        state,
                        session_generation: r.session_generation,
                        target: SessionKey::new("", SessionKind::Private, ""), // 折叠时目标不还原(不参与判定)
                        deadline_ms: 0,
                        history: vec![(r.state.clone(), r.at_unix_ms)],
                    },
                );
                if !terminal {
                    self.pending.push_back(r.request_id);
                }
            }
            Some(entry) => {
                if !transition_valid(&entry.state, &state) {
                    self.counters.transition_anomalies += 1;
                    report.transition_anomalies += 1;
                    return;
                }
                entry.history.push((r.state.clone(), r.at_unix_ms));
                let was_terminal = entry.state.is_terminal();
                entry.state = state;
                if entry.state.is_terminal() {
                    self.pending.retain(|id| id != &r.request_id);
                } else if was_terminal {
                    // 终态 → 非终态的迁移只在 DeliveryUnknown→Success/Failure
                    // 更正中合法;corrected 条目不回到 pending(不重开)。
                }
            }
        }
    }

    fn fold_event(&mut self, e: crate::journal::EventRecord) {
        self.seen_native.insert(e.native_id.clone());
        if e.source == "update" {
            self.seen_updates.insert(e.native_id.clone());
            self.dedup_fifo.push_back((true, e.native_id.clone()));
        } else {
            self.dedup_fifo.push_back((false, e.native_id.clone()));
        }
        self.event_seq_next = self.event_seq_next.max(e.event_seq + 1);
        self.evict_dedup_if_needed();
    }

    // ---- 生命周期 ----

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn degraded_reason(&self) -> Option<&str> {
        self.degraded_reason.as_deref()
    }

    pub fn session(&self) -> Option<&SessionIdentity> {
        self.session.as_ref()
    }

    pub fn connection(&self) -> Option<&ConnectionIdentity> {
        self.conn.as_ref()
    }

    /// 绑定会话(Detached/Bootstrapping → Bound)。身份一旦绑定,任何不匹配
    /// 的会话身份在动作准入层被拒绝(SessionStale)。
    pub fn attach_session(&mut self, session: SessionIdentity) -> Result<(), RejectReason> {
        if !matches!(self.phase, Phase::Detached | Phase::Bootstrapping) {
            return Err(RejectReason::NotActive { phase: self.phase });
        }
        self.phase = Phase::Bound;
        self.session = Some(session);
        Ok(())
    }

    /// 连接就绪(Bound → Active)。Active 下重入视为协议错误。
    pub fn mark_connection_ready(&mut self, conn: ConnectionIdentity) -> Result<(), RejectReason> {
        if self.phase != Phase::Bound {
            return Err(RejectReason::NotActive { phase: self.phase });
        }
        self.phase = Phase::Active;
        self.conn = Some(conn);
        Ok(())
    }

    /// core/管道重连:只更换连接代次;**不重注册监听、不重发动作**。
    /// 返回需要逐项 QueryRequest 的非终态请求 id。
    pub fn reconnect(&mut self, conn: ConnectionIdentity) -> Result<Vec<String>, RejectReason> {
        if !matches!(self.phase, Phase::Active | Phase::Degraded) {
            return Err(RejectReason::NotActive { phase: self.phase });
        }
        if let Some(old) = &self.conn {
            if conn.core_instance_nonce == old.core_instance_nonce
                && conn.connection_epoch <= old.connection_epoch
            {
                return Err(RejectReason::ConnectionStale);
            }
        }
        self.phase = Phase::Active;
        self.degraded_reason = None;
        self.conn = Some(conn);
        Ok(self.pending.iter().cloned().collect())
    }

    /// 进入 Degraded(队列/日志/会话故障):停止新发送。
    pub fn degrade(&mut self, reason: impl Into<String>) {
        self.phase = Phase::Degraded;
        self.degraded_reason = Some(reason.into());
    }

    /// 停止入口(计划 §6.7):关新动作准入;可证明未执行的排队项取消。
    pub fn stop(&mut self) -> Result<usize, RejectReason> {
        if matches!(self.phase, Phase::Stopping | Phase::Closed | Phase::Quarantined) {
            return Ok(0);
        }
        self.phase = Phase::Stopping;
        let mut cancelled = 0;
        let ids: Vec<String> = self
            .pending
            .iter()
            .filter(|id| {
                self.requests
                    .get(*id)
                    .map(|e| matches!(e.state, ActionState::Queued | ActionState::Accepted))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        for id in ids {
            if self
                .transition(&id, ActionState::CancelledUnsent, None)
                .is_ok()
            {
                cancelled += 1;
                self.counters.sends_cancelled_unsent += 1;
            }
        }
        Ok(cancelled)
    }

    /// 关闭(Stopping → Closed):仍在途的派发项显式标记 DeliveryUnknown
    /// (结果未知,不是失败,也不是"未发送")。
    pub fn close(&mut self) -> Result<usize, RejectReason> {
        if self.phase == Phase::Closed {
            return Ok(0);
        }
        if self.phase != Phase::Stopping {
            return Err(RejectReason::NotActive { phase: self.phase });
        }
        let ids: Vec<String> = self.pending.iter().cloned().collect();
        let mut marked = 0;
        for id in ids {
            if self.transition(&id, ActionState::DeliveryUnknown, None).is_ok() {
                marked += 1;
            }
        }
        self.phase = Phase::Closed;
        Ok(marked)
    }

    // ---- 动作面 ----

    /// 提交发送(两层准入的第一层)。相同 id + 相同 payload → 查询语义,
    /// 返回当前状态,不重复执行;相同 id + 不同 payload → 冲突拒绝。
    pub fn submit_send(&mut self, req: SendTextRequest) -> Result<ActionState, RejectReason> {
        if self.phase != Phase::Active {
            return Err(RejectReason::NotActive { phase: self.phase });
        }
        let session = self.session.as_ref().ok_or(RejectReason::SessionStale)?;
        if &req.session_identity != session {
            return Err(RejectReason::SessionStale);
        }
        self.counters.sends_submitted += 1;
        if let Some(entry) = self.requests.get(&req.request_id) {
            if entry.payload_hash != req.payload_hash {
                return Err(RejectReason::RequestIdPayloadConflict);
            }
            // 查询语义:不再次执行。
            self.counters.requests_duplicate_query += 1;
            return Ok(entry.state.clone());
        }
        if self.pending.len() >= self.config.action_queue_max {
            return Err(RejectReason::CapacityExhausted { queue: "actions" });
        }
        self.persist_request(&req.request_id, &req, &ActionState::Queued, None)
            .map_err(|e| {
                self.journal_failure(e);
                RejectReason::JournalFailure
            })?;
        let entry = RequestEntry {
            payload_hash: req.payload_hash,
            state: ActionState::Queued,
            session_generation: req.session_identity.session_generation,
            target: req.target.clone(),
            deadline_ms: req.deadline_ms,
            history: vec![("Queued".into(), now_ms())],
        };
        self.requests.insert(req.request_id.clone(), entry);
        self.pending.push_back(req.request_id);
        self.counters.sends_accepted += 1;
        Ok(ActionState::Queued)
    }

    /// 派发起点:持久记录 dispatch_intent 后调用方才把动作交给 bridge。
    /// —— "意图已记录、桥确认未收到"的窗口由此显式化(计划 §6.6)。
    pub fn dispatch_begin(&mut self, request_id: &str) -> Result<(), RejectReason> {
        self.require_mutable(request_id)?;
        self.transition(request_id, ActionState::DispatchIntent, None)
            .map_err(|e| {
                self.counters.transition_anomalies += 1;
                e
            })?;
        self.counters.dispatch_intents += 1;
        Ok(())
    }

    /// bridge 确认已进入 native 调用(native_started)。
    pub fn bridge_started(&mut self, request_id: &str) -> Result<(), RejectReason> {
        self.require_mutable(request_id)?;
        self.transition(request_id, ActionState::NativeStarted, None)?;
        self.counters.native_started += 1;
        Ok(())
    }

    /// bridge 上报结果。迟到/重复/旧代次按幂等审计处理,不重开动作。
    pub fn bridge_result(
        &mut self,
        request_id: &str,
        result: NativeResult,
    ) -> Result<ReceiptOutcome, RejectReason> {
        let now = now_ms();
        let new_state = match &result {
            NativeResult::Success { native_id } => ActionState::ConfirmedSuccess {
                native_id: native_id.clone(),
            },
            NativeResult::Failure { reason } => ActionState::ConfirmedFailure {
                reason: reason.clone(),
            },
            NativeResult::Unknown => ActionState::DeliveryUnknown,
        };
        let Some(entry) = self.requests.get(request_id) else {
            // 未知请求的回执:仅审计。
            self.audit_receipt(request_id, 0, format!("late receipt for unknown request: {new_state:?}"), now);
            self.counters.receipts_audited_only += 1;
            return Ok(ReceiptOutcome::AuditedOnly);
        };
        let gen = entry.session_generation;
        let current_state = entry.state.clone();
        match (&current_state, &new_state) {
            (ActionState::DeliveryUnknown, ActionState::DeliveryUnknown) => {
                self.counters.receipts_duplicate_ignored += 1;
                Ok(ReceiptOutcome::DuplicateIgnored)
            }
            (ActionState::DeliveryUnknown, s) if matches!(s, ActionState::ConfirmedSuccess { .. } | ActionState::ConfirmedFailure { .. }) =>
            {
                // 明确迟到回执:更正"当前已知结果",不重开、不重发。
                self.persist_state(request_id, gen, &new_state, native_of(&new_state))
                    .map_err(|e| {
                        self.journal_failure(e);
                        RejectReason::JournalFailure
                    })?;
                let entry = self.requests.get_mut(request_id).unwrap();
                entry.state = new_state.clone();
                entry.history.push((state_name(&entry.state).to_string(), now));
                self.counters.receipts_corrected += 1;
                self.match_result_counter(&new_state);
                Ok(ReceiptOutcome::CorrectedFromUnknown)
            }
            (s, _) if s.is_terminal() => {
                self.audit_receipt(
                    request_id,
                    gen,
                    format!("duplicate receipt {new_state:?} on terminal {s:?}"),
                    now,
                );
                self.counters.receipts_duplicate_ignored += 1;
                Ok(ReceiptOutcome::DuplicateIgnored)
            }
            _ => {
                self.persist_state(request_id, gen, &new_state, native_of(&new_state))
                    .map_err(|e| {
                        self.journal_failure(e);
                        RejectReason::JournalFailure
                    })?;
                let entry = self.requests.get_mut(request_id).unwrap();
                entry.state = new_state.clone();
                entry.history.push((state_name(&entry.state).to_string(), now));
                if entry.state.is_terminal() {
                    self.pending.retain(|id| id != request_id);
                }
                self.match_result_counter(&new_state);
                Ok(ReceiptOutcome::Applied)
            }
        }
    }

    /// 取消:仅 bridge 确认从未开始调用才允许 CancelledUnsent;core 单方面
    /// 不得断言(计划 §6.6)。
    pub fn cancel_queued(&mut self, request_id: &str) -> Result<CancelOutcome, RejectReason> {
        let Some(entry) = self.requests.get(request_id) else {
            return Ok(CancelOutcome::NotFound);
        };
        if !matches!(entry.state, ActionState::Queued | ActionState::Accepted) {
            return Ok(CancelOutcome::TooLate);
        }
        self.transition(request_id, ActionState::CancelledUnsent, None)?;
        self.counters.sends_cancelled_unsent += 1;
        Ok(CancelOutcome::CancelledUnsent)
    }

    pub fn query_request(&self, request_id: &str) -> Option<ActionState> {
        self.requests.get(request_id).map(|e| e.state.clone())
    }

    /// QueryRequest 的完整摘要(状态 + 代次 + 目标 + 等待策略)。
    pub fn request_summary(&self, request_id: &str) -> Option<RequestSummary> {
        self.requests.get(request_id).map(|e| RequestSummary {
            state: e.state.clone(),
            session_generation: e.session_generation,
            target_repr: e.target.to_string(),
            deadline_ms: e.deadline_ms,
        })
    }

    pub fn request_history(
        &self,
        request_id: &str,
    ) -> Option<Vec<(String, u64)>> {
        self.requests.get(request_id).map(|e| e.history.clone())
    }

    /// 重启后需要逐项 QueryRequest 的非终态请求(不自动重发、不自动取消)。
    pub fn pending_queries(&self) -> Vec<String> {
        self.pending.iter().cloned().collect()
    }

    /// 请求去重账本容量(计划 §6.6:容量耗尽拒绝新发送,不为腾空间删除
    /// 仍可重提的 ID)。
    pub fn request_ledger_len(&self) -> usize {
        self.requests.len()
    }

    // ---- 事件面 ----

    /// 受理接收事件:先持久化(= EventAck 依据),再入交付队列。
    /// 去重按 账号+会话+原生ID(+来源类);同正文不同 ID 一律保留。
    pub fn on_event(&mut self, mut ev: MsgEvent) -> Result<EventReceipt, RejectReason> {
        let current_gen = self.session.as_ref().map(|s| s.session_generation);
        let is_current = current_gen == Some(ev.session_generation);
        if !is_current {
            // 旧代次证据:持久化(审计),不进入当前业务链。
            let seq = self.alloc_seq();
            ev.event_seq = seq;
            self.persist_event(&ev)
                .map_err(|e| {
                    self.journal_failure(e);
                    RejectReason::JournalFailure
                })?;
            self.counters.events_old_generation_audited += 1;
            return Ok(EventReceipt::AuditedOldGeneration { event_seq: seq });
        }

        let dup_native = self.seen_native.contains(&ev.native_id.0);
        match ev.source {
            EventSource::Recv | EventSource::HistoryBackfill => {
                if dup_native {
                    self.counters.events_duplicate_suppressed += 1;
                    return Ok(EventReceipt::DuplicateSuppressed);
                }
            }
            EventSource::Update => {
                if self.seen_updates.contains(&ev.native_id.0) {
                    self.counters.events_duplicate_suppressed += 1;
                    return Ok(EventReceipt::DuplicateSuppressed);
                }
            }
        }

        // 交付队列限额(items/bytes 任一到达 → Gap + 拒绝;bridge 依契约保留
        // replay 窗口,不是静默丢弃)。
        let ev_bytes = ev.text.len() + ev.sender.len() + ev.native_id.0.len() + 128;
        if self.events_out.len() + 1 > self.config.event_queue_max_items
            || self.events_out_bytes + ev_bytes > self.config.event_queue_max_bytes
        {
            self.record_gap(
                self.event_seq_next,
                None,
                "event queue overflow (items/bytes limit)",
            )?;
            self.degrade("event queue overflow");
            return Err(RejectReason::CapacityExhausted { queue: "events" });
        }

        let seq = self.alloc_seq();
        ev.event_seq = seq;
        self.persist_event(&ev)
            .map_err(|e| {
                self.journal_failure(e);
                RejectReason::JournalFailure
            })?;
        // 持久化成功后才登记去重与入队(= ACK 前移点)。
        self.seen_native.insert(ev.native_id.0.clone());
        let is_update = ev.source == EventSource::Update;
        if is_update {
            self.seen_updates.insert(ev.native_id.0.clone());
        }
        self.dedup_fifo.push_back((is_update, ev.native_id.0.clone()));
        self.evict_dedup_if_needed();
        self.events_out.push_back(ev);
        self.events_out_bytes += ev_bytes;
        self.counters.events_persisted += 1;
        Ok(EventReceipt::Published { event_seq: seq })
    }

    /// 下游(测试客户端/后续 OneBot)拉取事件:取出即持久化交付游标。
    pub fn drain_deliverables(&mut self, max: usize) -> Vec<MsgEvent> {
        let n = max.min(self.events_out.len());
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            if let Some(ev) = self.events_out.pop_front() {
                self.events_out_bytes -= ev.text.len() + ev.sender.len() + ev.native_id.0.len() + 128;
                self.delivered_cursor = self.delivered_cursor.max(ev.event_seq);
                let _ = self.journal.append(&JournalRecord::EventDelivered {
                    event_seq: ev.event_seq,
                });
                out.push(ev);
            }
        }
        out
    }

    pub fn delivered_cursor(&self) -> u64 {
        self.delivered_cursor
    }

    pub fn pending_event_queue_len(&self) -> usize {
        self.events_out.len()
    }

    pub fn gaps(&self) -> Vec<GapRecord> {
        self.gaps.iter().cloned().collect()
    }

    pub fn receipt_audits(&self) -> Vec<ReceiptAuditEntry> {
        self.receipt_audits.iter().cloned().collect()
    }

    // ---- 内部 ----

    fn alloc_seq(&mut self) -> u64 {
        let s = self.event_seq_next;
        self.event_seq_next += 1;
        s
    }

    fn evict_dedup_if_needed(&mut self) {
        while self.dedup_fifo.len() > self.dedup_capacity {
            if let Some((is_update, id)) = self.dedup_fifo.pop_front() {
                if is_update {
                    self.seen_updates.remove(&id);
                } else {
                    self.seen_native.remove(&id);
                }
            }
        }
    }

    fn require_mutable(&mut self, request_id: &str) -> Result<(), RejectReason> {
        match self.requests.get(request_id) {
            None => {
                self.audit_receipt(request_id, 0, "op on unknown request".into(), now_ms());
                Err(RejectReason::SessionStale) // 未知请求按无效处理并审计
            }
            Some(e) if e.state.is_terminal() => Err(RejectReason::RequestIdPayloadConflict),
            Some(_) => Ok(()),
        }
    }

    fn transition(
        &mut self,
        request_id: &str,
        new_state: ActionState,
        native_id: Option<NativeMessageId>,
    ) -> Result<(), RejectReason> {
        let gen = match self.requests.get(request_id) {
            Some(e) => e.session_generation,
            None => return Err(RejectReason::SessionStale),
        };
        let old_name;
        {
            let entry = self.requests.get(request_id).unwrap();
            old_name = state_name(&entry.state);
            if !transition_valid(&entry.state, &new_state) {
                self.counters.transition_anomalies += 1;
                return Err(RejectReason::RequestIdPayloadConflict);
            }
        }
        self.persist_state(request_id, gen, &new_state, native_id.clone())
            .map_err(|e| {
                self.journal_failure(e);
                RejectReason::JournalFailure
            })?;
        let entry = self.requests.get_mut(request_id).unwrap();
        entry.state = new_state;
        entry.history.push((state_name(&entry.state).to_string(), now_ms()));
        if entry.state.is_terminal() {
            self.pending.retain(|id| id != request_id);
        }
        let _ = old_name;
        Ok(())
    }

    fn persist_request(
        &mut self,
        request_id: &str,
        req: &SendTextRequest,
        state: &ActionState,
        native_id: Option<NativeMessageId>,
    ) -> Result<(), JournalError> {
        self.persist_state_full(
            request_id,
            req.payload_hash,
            req.session_identity.session_generation,
            state,
            native_id,
        )
    }

    fn persist_state(
        &mut self,
        request_id: &str,
        gen: u64,
        state: &ActionState,
        native_id: Option<NativeMessageId>,
    ) -> Result<(), JournalError> {
        let hash = self
            .requests
            .get(request_id)
            .map(|e| e.payload_hash)
            .unwrap_or(0);
        self.persist_state_full(request_id, hash, gen, state, native_id)
    }

    fn persist_state_full(
        &mut self,
        request_id: &str,
        hash: u64,
        gen: u64,
        state: &ActionState,
        native_id: Option<NativeMessageId>,
    ) -> Result<(), JournalError> {
        self.journal.append(&JournalRecord::Request(RequestRecord {
            v: 1,
            request_id: request_id.to_string(),
            payload_hash: hash,
            state: state_name(state).to_string(),
            native_id: native_id.map(|n| n.0),
            at_unix_ms: now_ms(),
            session_generation: gen,
        }))
    }

    fn persist_event(&mut self, ev: &MsgEvent) -> Result<(), JournalError> {
        self.journal.append(&JournalRecord::Event(
            crate::journal::EventRecord {
                v: 1,
                event_seq: ev.event_seq,
                session_generation: ev.session_generation,
                session: session_to_dto_value(&ev.session),
                direction: match ev.direction {
                    Direction::Incoming => "incoming".into(),
                    Direction::SelfSent => "self_sent".into(),
                },
                sender: ev.sender.clone(),
                native_id: ev.native_id.0.clone(),
                text: ev.text.clone(),
                platform_time: ev.platform_time,
                observed_at_unix_ms: ev.observed_at_unix_ms,
                source: match ev.source {
                    EventSource::Recv => "recv",
                    EventSource::Update => "update",
                    EventSource::HistoryBackfill => "history_backfill",
                }
                .into(),
            },
        ))
    }

    fn record_gap(
        &mut self,
        from_seq: u64,
        to_seq: Option<u64>,
        reason: &str,
    ) -> Result<(), RejectReason> {
        let g = GapRecord {
            v: 1,
            from_event_seq: from_seq,
            to_event_seq: to_seq,
            reason: reason.to_string(),
            at_unix_ms: now_ms(),
        };
        self.journal
            .append(&JournalRecord::Gap(g.clone()))
            .map_err(|e| {
                self.journal_failure(e);
                RejectReason::JournalFailure
            })?;
        self.gaps.push_back(g);
        if self.gaps.len() > 256 {
            self.gaps.pop_front();
        }
        self.counters.gaps_recorded += 1;
        Ok(())
    }

    fn audit_receipt(&mut self, request_id: &str, gen: u64, detail: String, at: u64) {
        let entry = ReceiptAuditEntry {
            request_id: request_id.to_string(),
            session_generation: gen,
            detail,
            at_unix_ms: at,
        };
        let _ = self
            .journal
            .append(&JournalRecord::ReceiptAudit(crate::journal::ReceiptAuditRecord {
                v: 1,
                request_id: entry.request_id.clone(),
                session_generation: entry.session_generation,
                detail: entry.detail.clone(),
                at_unix_ms: entry.at_unix_ms,
            }));
        self.receipt_audits.push_back(entry);
        if self.receipt_audits.len() > 1024 {
            self.receipt_audits.pop_front();
        }
    }

    fn journal_failure(&mut self, _e: JournalError) {
        self.counters.journal_failures += 1;
        // 落盘失败 → 关闭发送入口(Degraded),保留原文件。
        self.degrade("journal write failure");
    }

    fn match_result_counter(&mut self, s: &ActionState) {
        match s {
            ActionState::ConfirmedSuccess { .. } => self.counters.results_success += 1,
            ActionState::ConfirmedFailure { .. } => self.counters.results_failure += 1,
            ActionState::DeliveryUnknown => self.counters.results_unknown += 1,
            _ => {}
        }
    }
}

/// HostIdentity 便捷构造(测试/接线用;nonce 由调用方提供 OS 随机值)。
pub fn host_identity(pid: u32, created_utc: &str, baseline: &str, build: &str, nonce: u64) -> HostIdentity {
    HostIdentity {
        pid,
        process_created_utc: created_utc.to_string(),
        module_baseline: baseline.to_string(),
        bridge_build: build.to_string(),
        host_nonce: nonce,
    }
}

fn state_name(s: &ActionState) -> &'static str {
    match s {
        ActionState::Accepted => "Accepted",
        ActionState::Queued => "Queued",
        ActionState::DispatchIntent => "DispatchIntent",
        ActionState::NativeStarted => "NativeStarted",
        ActionState::ConfirmedSuccess { .. } => "ConfirmedSuccess",
        ActionState::ConfirmedFailure { .. } => "ConfirmedFailure",
        ActionState::CancelledUnsent => "CancelledUnsent",
        ActionState::DeliveryUnknown => "DeliveryUnknown",
    }
}

fn state_from_name(name: &str) -> Option<ActionState> {
    match name {
        "Accepted" => Some(ActionState::Accepted),
        "Queued" => Some(ActionState::Queued),
        "DispatchIntent" => Some(ActionState::DispatchIntent),
        "NativeStarted" => Some(ActionState::NativeStarted),
        "ConfirmedSuccess" => Some(ActionState::ConfirmedSuccess { native_id: None }),
        "ConfirmedFailure" => Some(ActionState::ConfirmedFailure {
            reason: String::new(),
        }),
        "CancelledUnsent" => Some(ActionState::CancelledUnsent),
        "DeliveryUnknown" => Some(ActionState::DeliveryUnknown),
        _ => None,
    }
}

fn native_of(s: &ActionState) -> Option<NativeMessageId> {
    match s {
        ActionState::ConfirmedSuccess { native_id } => native_id.clone(),
        _ => None,
    }
}

/// 合法迁移表(计划 §6.6 状态图;终态 DeliveryUnknown 可被明确结果更正)。
fn transition_valid(from: &ActionState, to: &ActionState) -> bool {
    use ActionState::*;
    matches!(
        (from, to),
        (Accepted, Queued)
            | (Accepted, CancelledUnsent)
            | (Queued, DispatchIntent)
            | (Queued, CancelledUnsent)
            | (DispatchIntent, NativeStarted)
            | (DispatchIntent, ConfirmedSuccess { .. })
            | (DispatchIntent, ConfirmedFailure { .. })
            | (DispatchIntent, DeliveryUnknown)
            | (NativeStarted, ConfirmedSuccess { .. })
            | (NativeStarted, ConfirmedFailure { .. })
            | (NativeStarted, DeliveryUnknown)
            // 迟到明确回执更正未知结果(不重开动作)。
            | (DeliveryUnknown, ConfirmedSuccess { .. })
            | (DeliveryUnknown, ConfirmedFailure { .. })
    )
}
