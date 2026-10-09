//! 常驻 native 会话层(N3 第 1 步;计划 §7-N3 动作 1)。
//!
//! 职责:把 N1 的接收规范化与 N2 的响应账本组装为**有门控的状态机**,
//! 为 resident/daemon 迁移提供独立边界(计划 §3:native adapter 先提供
//! 独立 NativeSession 边界,再接线)。真实地址只在
//! 模块 hash/profile/锚点单候选/线程/代次门全部通过后经
//! [`AttachProof`] 进入 —— 本模块自身不做 FFI 调用;真实 ops 由 N3
//! 后续接线实现 [`NativeOps`],合同测试用 fake。
//!
//! 状态合同(计划 §5-3/§7-N3):
//! - 受理(Accepted)≠ 已派发 ≠ 已开始原生调用 ≠ 业务成功 —— 分开表达;
//! - unsupported/门未过 → **零原生调用**(`counters.native_ops == 0`,T12);
//! - 注册响应消费者先于发起(`enqueue_send` 内部先 `register` 再 submit);
//! - 代次失效后旧结果只审计;关闭排空显式 Unknown,不自动重发;
//! - Clean 仅在原生侧确认排空时给出,否则 Quarantined(T13)。

use crate::native_response::{
    Cookie, Completion, ResponseLedger, ResponseOutcome,
};

/// 附加证据(调用方已完成门核对后传入;本模块如实登记,不重复核对)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachProof {
    /// 模块基线标识(如 "qq-9.9.33-52230";hash 核对属调用方)。
    pub module_baseline: String,
    /// 锚点候选数(必须为 1;多候选 = 不确定,不得附加)。
    pub anchor_candidates: usize,
    /// 服务对象地址(已过锚点核对;本模块只透传给 ops)。
    pub service_addr: usize,
    pub host_generation: u64,
    pub session_generation: u64,
}

impl AttachProof {
    pub fn validate(&self) -> Result<(), AttachError> {
        if self.anchor_candidates != 1 {
            return Err(AttachError::AmbiguousAnchor(self.anchor_candidates));
        }
        if self.service_addr == 0 {
            return Err(AttachError::NullService);
        }
        if self.module_baseline.is_empty() {
            return Err(AttachError::MissingBaseline);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachError {
    /// 锚点多候选(拒绝;T18 多候选场景)。
    AmbiguousAnchor(usize),
    NullService,
    MissingBaseline,
    /// 已在会话中(重复 attach 拒绝;单宿主单 active bridge)。
    AlreadyAttached,
    /// ops 报告底层未就绪(transport 未连接等)。
    NotReady(String),
}

/// 会话阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionPhase {
    Detached,
    Ready,
    /// 代次失效(QQ 重登/会话重绑);等待新 proof 重新附加。
    Invalidated,
    /// 已关闭;bool = Clean(原生排空确认)/Quarantined(未确认)。
    Closed { clean: bool },
}

/// 发送受理分类(受理 ≠ 派发 ≠ 原生开始 ≠ 业务成功)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendAcceptance {
    /// 已受理:cookie 已预登记,已提交 ops(原生调用已发起;结果走账本)。
    Dispatched { cookie: Cookie, seq: u64 },
    /// ops 拒绝于调用前(零原生接触);终态失败,账本条目回收。
    RefusedPreCall { reason: String },
    /// 底层未就绪(transport 未连接等);**非失败非成功**,未发起;
    /// 账本条目回收,可重试(上层决定)。
    NotReady { reason: String },
    /// 持久化分配失败(seq/落盘);未发起(T09:未持久化的值绝不发出去)。
    AllocationFailed { reason: String },
    /// 会话不在 Ready 状态。
    NotAttached,
}

/// 关闭报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseReport {
    pub clean: bool,
    /// 排空时仍在途、被表达为 Unknown 的 request_id。
    pub unknown_requests: Vec<String>,
    pub native_ops_total: u64,
}

/// 计数(T12:native_ops 只在真实 ops 提交时递增;unsupported 必须为 0)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionCounters {
    pub native_ops: u64,
    pub dispatched: u64,
    pub refused_pre_call: u64,
    pub not_ready: u64,
    pub allocation_failed: u64,
    pub invalidated: u64,
}

/// 原生操作抽象(真实实现 N3 接线;fake 用于合同测试)。
pub trait NativeOps {
    /// 零副作用就绪探针(attach 用;不提交任何请求)。
    fn probe_ready(&mut self, service_addr: usize) -> Result<(), OpsError>;
    /// 提交一条已编码请求。实现必须:真实调用前核对 ready;返回后不得
    /// 再触碰 body(cookie 所有权语义由真实层定)。
    fn submit(&mut self, service_addr: usize, cookie: &Cookie, body: &[u8]) -> Result<(), OpsError>;
}

/// ops 层错误(未发起 = 零原生接触)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpsError {
    /// 调用前置不满足(未触碰原生)。
    PreCall(&'static str),
    /// 底层未就绪(transport 未连接;未发起)。
    NotReady(String),
}

/// 常驻会话(纯调度与账本;无锁、单线程所有,由 resident worker 持有)。
#[derive(Debug)]
pub struct NativeSession<S: SeqSource> {
    proof: Option<AttachProof>,
    phase: SessionPhase,
    ledger: ResponseLedger,
    seq: S,
    pub counters: SessionCounters,
}

/// seq 来源(与 native_response::SeqAllocator 语义一致;独立 trait 便于注入)。
pub trait SeqSource {
    /// 持久化先于交付;失败 = 未发起(T09)。
    fn allocate(&mut self) -> Result<u64, std::io::Error>;
}

impl<P: FnMut(u64) -> Result<(), std::io::Error>> SeqSource for crate::native_response::SeqAllocator<P> {
    fn allocate(&mut self) -> Result<u64, std::io::Error> {
        crate::native_response::SeqAllocator::allocate(self)
    }
}

impl<S: SeqSource> NativeSession<S> {
    pub fn new(seq: S) -> Self {
        Self {
            proof: None,
            phase: SessionPhase::Detached,
            ledger: ResponseLedger::new(),
            seq,
            counters: SessionCounters::default(),
        }
    }

    pub fn phase(&self) -> &SessionPhase {
        &self.phase
    }

    pub fn ledger(&mut self) -> &mut ResponseLedger {
        &mut self.ledger
    }

    /// 附加:门核对(validation)→ ops 探针 → Ready。
    /// 失败路径零原生调用(T12)。
    pub fn attach<O: NativeOps>(
        &mut self,
        ops: &mut O,
        proof: AttachProof,
    ) -> Result<(), AttachError> {
        proof.validate()?;
        if self.proof.is_some() {
            return Err(AttachError::AlreadyAttached);
        }
        // 底层就绪探针:零副作用(不提交任何请求)。
        if let Err(OpsError::NotReady(r)) = ops.probe_ready(proof.service_addr) {
            return Err(AttachError::NotReady(r));
        }
        self.ledger.set_generations(proof.host_generation, proof.session_generation);
        self.proof = Some(proof);
        self.phase = SessionPhase::Ready;
        Ok(())
    }

    /// 发起一条普通文本发送(纯文本编码在上层;本层只管受理/登记/派发)。
    ///
    /// 顺序合同:seq 分配 → cookie 生成 → **预登记** → ops 提交。
    /// 任一前置失败都不发起、账本不留悬挂条目。
    pub fn enqueue_send<O: NativeOps>(
        &mut self,
        ops: &mut O,
        request_id: &str,
        cookie: Cookie,
        body: &[u8],
    ) -> SendAcceptance {
        if self.phase != SessionPhase::Ready {
            return SendAcceptance::NotAttached;
        }
        // T09:先分配并持久化 seq;失败 = 拒绝发起。
        let seq = match self.seq.allocate() {
            Ok(s) => s,
            Err(e) => {
                self.counters.allocation_failed += 1;
                return SendAcceptance::AllocationFailed { reason: e.to_string() };
            }
        };
        // 预登记先于发起(计划 §5-2)。登记失败(重复 cookie/request)= 调用方
        // 缺陷,不发起。
        if let Err(e) = self.ledger.register(request_id, cookie) {
            self.counters.refused_pre_call += 1;
            return SendAcceptance::RefusedPreCall { reason: format!("register: {e:?}") };
        }
        let proof = self.proof.as_ref().expect("phase Ready implies proof");
        match ops.submit(proof.service_addr, &cookie, body) {
            Ok(()) => {
                self.counters.native_ops += 1;
                self.counters.dispatched += 1;
                SendAcceptance::Dispatched { cookie, seq }
            }
            Err(OpsError::PreCall(reason)) => {
                // 零原生接触;回收账本条目(登记撤销 = 以终态失败终结,上层可见)。
                self.ledger.complete(
                    &cookie,
                    ResponseOutcome::TransportFailed { reason: format!("pre-call: {reason}") },
                );
                self.ledger.take_terminal();
                self.counters.refused_pre_call += 1;
                SendAcceptance::RefusedPreCall { reason: reason.to_string() }
            }
            Err(OpsError::NotReady(reason)) => {
                // 未发起;回收登记(可重试语义由上层决定)。
                self.ledger.complete(&cookie, ResponseOutcome::TransportFailed { reason: format!("not-ready: {reason}") });
                self.ledger.take_terminal();
                self.counters.not_ready += 1;
                SendAcceptance::NotReady { reason }
            }
        }
    }

    /// 响应到达(转发给账本;inline/重复/乱序/迟到统一口径)。
    pub fn on_response(&mut self, cookie: &Cookie, outcome: ResponseOutcome) -> Completion {
        self.ledger.complete(cookie, outcome)
    }

    /// 代次失效:阶段迁移 + 在途全部表达为 Unknown(不自动重发)。
    pub fn invalidate(&mut self, reason: &str) -> Vec<String> {
        if self.phase == SessionPhase::Ready {
            self.phase = SessionPhase::Invalidated;
            self.counters.invalidated += 1;
            self.ledger.drain(reason).into_iter().map(|(id, _)| id).collect()
        } else {
            Vec::new()
        }
    }

    /// 关闭:停止受理,排空在途 → Unknown;Clean 仅当调用方确认原生排空。
    pub fn close(&mut self, reason: &str, native_drain_confirmed: bool) -> CloseReport {
        let unknown = self
            .ledger
            .drain(reason)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        self.phase = SessionPhase::Closed { clean: native_drain_confirmed };
        CloseReport {
            clean: native_drain_confirmed,
            unknown_requests: unknown,
            native_ops_total: self.counters.native_ops,
        }
    }

    /// 终态取走(交付给 core;交付后迟到回调转 UnknownCookie 审计)。
    pub fn take_terminal(&mut self) -> Vec<(String, ResponseOutcome)> {
        self.ledger.take_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_response::SeqAllocator;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn cookie(n: u8) -> Cookie {
        let mut c = [0u8; 16];
        c[0] = n;
        Cookie(c)
    }

    fn proof() -> AttachProof {
        AttachProof {
            module_baseline: "qq-9.9.33-52230".into(),
            anchor_candidates: 1,
            service_addr: 0x1000,
            host_generation: 1,
            session_generation: 1,
        }
    }

    /// Fake ops:记录调用序;可配置 PreCall 拒绝/NotReady。
    struct FakeOps {
        calls: Rc<RefCell<Vec<String>>>, // "probe" | "submit:<svc>:<cookie0>:<len>"
        fail: Option<OpsError>,
    }
    impl FakeOps {
        fn new() -> (Self, Rc<RefCell<Vec<String>>>) {
            let calls = Rc::new(RefCell::new(Vec::new()));
            (Self { calls: calls.clone(), fail: None }, calls)
        }
    }
    impl NativeOps for FakeOps {
        fn probe_ready(&mut self, _svc: usize) -> Result<(), OpsError> {
            self.calls.borrow_mut().push("probe".into());
            Ok(())
        }
        fn submit(&mut self, svc: usize, cookie: &Cookie, body: &[u8]) -> Result<(), OpsError> {
            self.calls.borrow_mut().push(format!("submit:{svc}:{}:{}", cookie.0[0], body.len()));
            match &self.fail {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        }
    }

    fn seq_from(start: u64) -> SeqAllocator<Box<dyn FnMut(u64) -> Result<(), std::io::Error>>> {
        SeqAllocator::new(start, Box::new(|_| Ok(())))
    }

    /// T12:门未过(多候选)→ 零原生调用;阶段不变。
    #[test]
    fn attach_gate_failure_means_zero_native_ops() {
        let (mut ops, calls) = FakeOps::new();
        let mut s = NativeSession::new(seq_from(1));
        let bad = AttachProof { anchor_candidates: 2, ..proof() };
        assert_eq!(s.attach(&mut ops, bad), Err(AttachError::AmbiguousAnchor(2)));
        assert_eq!(s.phase(), &SessionPhase::Detached);
        assert_eq!(s.counters.native_ops, 0);
        assert!(calls.borrow().is_empty());
    }

    /// 附加成功 → Ready;enqueue:seq 先持久化、cookie 先登记、再提交。
    #[test]
    fn dispatch_order_seq_register_then_submit() {
        let (mut ops, calls) = FakeOps::new();
        let mut s = NativeSession::new(seq_from(7));
        s.attach(&mut ops, proof()).unwrap();
        assert_eq!(s.phase(), &SessionPhase::Ready);
        let acc = s.enqueue_send(&mut ops, "r1", cookie(1), b"BODY");
        assert_eq!(acc, SendAcceptance::Dispatched { cookie: cookie(1), seq: 7 });
        assert_eq!(s.counters.native_ops, 1);
        assert_eq!(calls.borrow().len(), 2, "探针 + 提交");
        assert_eq!(calls.borrow()[0], "probe");
        assert_eq!(calls.borrow()[1], "submit:4096:1:4");
        // inline 回调(提交已返回、响应先到)→ 终态可取走。
        let c = s.on_response(&cookie(1), ResponseOutcome::BusinessSuccess { native_id: "m1".into() });
        assert!(matches!(c, Completion::Terminated { .. }));
        let done = s.take_terminal();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].0, "r1");
    }

    /// PreCall 拒绝:零原生接触、终态失败、无悬挂条目。
    #[test]
    fn pre_call_refusal_leaves_no_hanging_entry() {
        let (mut ops, calls) = FakeOps::new();
        ops.fail = Some(OpsError::PreCall("links unwired"));
        let mut s = NativeSession::new(seq_from(1));
        s.attach(&mut ops, proof()).unwrap();
        let acc = s.enqueue_send(&mut ops, "r1", cookie(2), b"x");
        assert!(matches!(acc, SendAcceptance::RefusedPreCall { .. }));
        assert_eq!(s.counters.native_ops, 0, "PreCall 拒绝不计原生操作");
        assert!(s.ledger().pending().is_empty(), "账本无悬挂");
        // 调用记录 = 探针 + 1 次被 ops 拒绝的提交尝试(ops 层未触原生);
        // 会话层的“零原生接触”由 counters.native_ops==0 表达。
        assert_eq!(calls.borrow().len(), 2);
    }

    /// NotReady:未发起,不冒充失败或成功。
    #[test]
    fn not_ready_is_its_own_class() {
        let (mut ops, _) = FakeOps::new();
        ops.fail = Some(OpsError::NotReady("transport disconnected".into()));
        let mut s = NativeSession::new(seq_from(1));
        s.attach(&mut ops, proof()).unwrap();
        let acc = s.enqueue_send(&mut ops, "r1", cookie(3), b"x");
        assert!(matches!(acc, SendAcceptance::NotReady { .. }));
        assert_eq!(s.counters.not_ready, 1);
        assert!(s.ledger().pending().is_empty());
    }

    /// seq 持久化失败 → AllocationFailed,未发起,零原生调用。
    #[test]
    fn allocation_failure_blocks_dispatch() {
        let (mut ops, calls) = FakeOps::new();
        let mut s = NativeSession::new(SeqAllocator::new(1, Box::new(|_| {
            Err(std::io::Error::other("disk full"))
        })));
        s.attach(&mut ops, proof()).unwrap();
        let acc = s.enqueue_send(&mut ops, "r1", cookie(4), b"x");
        assert!(matches!(acc, SendAcceptance::AllocationFailed { .. }));
        assert_eq!(s.counters.native_ops, 0);
        assert_eq!(calls.borrow().len(), 1);
    }

    /// 代次失效:在途 → Unknown;旧代次回调只审计。
    #[test]
    fn invalidate_drains_unknown_and_stales_old_results() {
        let (mut ops, _) = FakeOps::new();
        let mut s = NativeSession::new(seq_from(1));
        s.attach(&mut ops, proof()).unwrap();
        let _ = s.enqueue_send(&mut ops, "in-flight", cookie(5), b"x");
        let drained = s.invalidate("session rebind");
        assert_eq!(drained, vec!["in-flight".to_string()]);
        assert_eq!(s.phase(), &SessionPhase::Invalidated);
        // 旧 cookie 迟到 → UnknownCookie 审计(条目已排空)。
        let c = s.on_response(&cookie(5), ResponseOutcome::BusinessSuccess { native_id: "m".into() });
        assert_eq!(c, Completion::UnknownCookie);
        // Invalidated 状态不接受新发送。
        assert_eq!(s.enqueue_send(&mut ops, "r2", cookie(6), b"x"), SendAcceptance::NotAttached);
    }

    /// 关闭:未终态 → Unknown;Clean/Quarantined 如实;关闭后零受理。
    #[test]
    fn close_reports_honestly() {
        let (mut ops, _) = FakeOps::new();
        let mut s = NativeSession::new(seq_from(1));
        s.attach(&mut ops, proof()).unwrap();
        let _ = s.enqueue_send(&mut ops, "a", cookie(7), b"x");
        let rpt = s.close("stop requested", false);
        assert!(!rpt.clean, "未确认原生排空 = Quarantined");
        assert_eq!(rpt.unknown_requests, vec!["a".to_string()]);
        assert_eq!(s.phase(), &SessionPhase::Closed { clean: false });
        assert_eq!(
            s.enqueue_send(&mut ops, "b", cookie(8), b"x"),
            SendAcceptance::NotAttached,
            "关闭后零受理"
        );
        // 已终态不被二次表达:再 close 只返回空。
        let rpt2 = s.close("again", true);
        assert!(rpt2.unknown_requests.is_empty());
        assert_eq!(s.phase(), &SessionPhase::Closed { clean: true });
    }
}
