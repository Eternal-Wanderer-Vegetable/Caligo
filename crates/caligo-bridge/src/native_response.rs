//! 业务响应关联账本(N2;计划 §5-2/§7-N2)。
//!
//! 合同来源:计划 N2"建立 (host_generation, session_generation, request_id,
//! cookie) 关联;**发起之前预登记**,inline/异步/重复/乱序/迟到回调都进入
//! 同一结果账本";`response-consumer-scope.md`(cookie = 客户端自配 16B
//! msgUid,响应按 cookie 键控)。
//!
//! 边界(与既有部件的分工):
//! - `native_msf::MsfAdapter` 的在途账本以 request_id 键控(假宿主合同);
//!   真实响应按 cookie/msgUid 到达 —— 本模块提供 cookie 键控的预登记层,
//!   两者在 N3 接线时组合(request_id ↔ cookie 双向)。
//! - **原生返回/ACK 不等于业务成功**:`BusinessSuccess` 在类型上强制携带
//!   稳定 native 身份(缺身份 = 不得构成本变体;与
//!   `caligo_core::qq_protocol::receipt` 的 Unconfirmed 口径一致)。
//! - 未决四项(940588 门控/key 组合/msgUid 字段号/消费者 ABI)见
//!   response-consumer-scope.md;本模块不假设其答案,仅固定关联语义。

/// 响应关联键的 cookie:16B msgUid(95d4e6 契约;客户端自配)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cookie(pub [u8; 16]);

impl Cookie {
    /// 全零 cookie 拒绝:真实客户端 msgUid 非零;全零意味着调用方未生成。
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 16]
    }
}

/// 业务终局(四分类;新增类别须过计划版本门 —— BusinessAccepted 于
/// 2026-10-11 §四十 过门:field 实证 v4–v13 通道交付 ack-without-msgUid,
/// 类型合同"ACK 不能变成业务成功"(计划 §178)要求其与 BusinessSuccess
/// 严格分立;上层按"未关联"处理,不得伪造 id)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseOutcome {
    /// 业务成功。**必须**携带稳定 native 身份(msgUid/消息 ID);缺失时
    /// 上层只能表达 Unconfirmed/Unknown,不得构造本变体。
    BusinessSuccess { native_id: String },
    /// 已受理确认(消费通道 errCode = 0)但通道未提供 native 身份。
    /// 与 [`ResponseOutcome::BusinessSuccess`] 严格分立:不冒充成功,
    /// 上层按"未关联成功"处理(送达性已证,身份缺失)。
    BusinessAccepted,
    /// 业务明确拒绝(可解释 retCode/errStr;T07 负例的落点)。
    BusinessRejected { ret_code: i32, err: String },
    /// 传输层失败(未达业务层;与业务拒绝严格区分,不混用错误通道)。
    TransportFailed { reason: String },
}

/// 预登记错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// cookie 已被未终结请求占用(msgUid 冲突;真实客户端每消息新鲜值,
    /// 冲突即调用方缺陷)。
    DuplicateCookie,
    /// 同 request_id 未终结即重复发起(上层幂等语义自己管,本层不吞)。
    DuplicateRequest,
    /// 全零 cookie 拒绝。
    ZeroCookie,
}

/// `complete` 的分类结果(每个到达的回调都有明确去向)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// 服务 ACK 已确认,但 native identity 尚缺;保留登记继续等待后续证据。
    AcceptedUnconfirmed { request_id: String },
    /// 首次终结:请求到达终局,交给上层。
    Terminated {
        request_id: String,
        outcome: ResponseOutcome,
    },
    /// 已终态后的迟到/重复回调:只审计,不翻案(T13/T08 口径)。
    DuplicateIgnored { request_id: String },
    /// 旧代次到达:只审计(代次失效后结果不改变新代次状态)。
    StaleGeneration { request_id: String },
    /// 未经预登记的 cookie:不归属任何请求,审计计数(生产出现即诊断:
    /// 说明预登记先于发起的合同被破坏)。
    UnknownCookie,
}

/// 一条预登记(发起前建立)。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Registration {
    request_id: String,
    host_generation: u64,
    session_generation: u64,
    accepted_unconfirmed: bool,
    wire_seq: Option<u32>,
}

/// cookie 键控响应账本。
///
/// 用法合同(计划 §5-2):`register` 必须先于原生发起;`complete` 接受
/// 任意时序(inline 早于发起返回/异步/重复/乱序);`drain` 在关闭路径把
/// 未终态请求表达为显式 Unknown(不自动重发,重发策略在上层)。
#[derive(Debug, Default)]
pub struct ResponseLedger {
    by_cookie: Entries,
    current_host_generation: u64,
    current_session_generation: u64,
    /// 审计计数:重复回调/旧代次/未知 cookie(T08/T13 断言源)。
    pub audit: LedgerAudit,
}

// 简单关联结构(避免引入依赖;请求量有界,线性查找即可——上层每会话
// 在途请求数为个位数;若 N3 实测变大再换 map,不在本层提前优化)。
#[derive(Debug, Default)]
struct Entries(Vec<(Cookie, Registration, Option<ResponseOutcome>)>);

/// 审计计数(有界窗口外溢出由调用方日志承载;计数本身饱和)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LedgerAudit {
    pub duplicate_ignored: u64,
    pub stale_generation: u64,
    pub unknown_cookie: u64,
    pub drained_unknown: u64,
}

impl ResponseLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 代次推进(attach/重绑/会话失效)。旧代次在途项保留(迟到回调走
    /// StaleGeneration 审计),不静默清除。
    pub fn set_generations(&mut self, host_generation: u64, session_generation: u64) {
        self.current_host_generation = host_generation;
        self.current_session_generation = session_generation;
    }

    /// 发起前预登记(计划 §5-2:不能"先发送,后注册消费者")。
    pub fn register(&mut self, request_id: &str, cookie: Cookie) -> Result<(), RegisterError> {
        self.register_with_wire_seq(request_id, cookie, None)
    }

    /// 登记由 core 持久分配的 client_seq，供诊断与后续强身份关联使用。
    pub fn register_with_wire_seq(
        &mut self,
        request_id: &str,
        cookie: Cookie,
        wire_seq: Option<u32>,
    ) -> Result<(), RegisterError> {
        if cookie.is_zero() {
            return Err(RegisterError::ZeroCookie);
        }
        let entries = &mut self.by_cookie.0;
        if entries.iter().any(|(c, _, _)| *c == cookie) {
            return Err(RegisterError::DuplicateCookie);
        }
        if entries.iter().any(|(_, r, _)| r.request_id == request_id) {
            return Err(RegisterError::DuplicateRequest);
        }
        entries.push((
            cookie,
            Registration {
                request_id: request_id.to_string(),
                host_generation: self.current_host_generation,
                session_generation: self.current_session_generation,
                accepted_unconfirmed: false,
                wire_seq,
            },
            None,
        ));
        Ok(())
    }

    /// 响应到达(任意时序)。只终结一次;其余全部显式分类。
    pub fn complete(&mut self, cookie: &Cookie, outcome: ResponseOutcome) -> Completion {
        let Some(idx) = self.by_cookie.0.iter().position(|(c, _, _)| c == cookie) else {
            self.audit.unknown_cookie = self.audit.unknown_cookie.saturating_add(1);
            return Completion::UnknownCookie;
        };
        let (_, reg, slot) = &mut self.by_cookie.0[idx];
        // 代次核对:登记代次 ≠ 当前代次 → 旧代次回调,只审计。
        if reg.host_generation != self.current_host_generation
            || reg.session_generation != self.current_session_generation
        {
            self.audit.stale_generation = self.audit.stale_generation.saturating_add(1);
            return Completion::StaleGeneration {
                request_id: reg.request_id.clone(),
            };
        }
        if slot.is_some() {
            self.audit.duplicate_ignored = self.audit.duplicate_ignored.saturating_add(1);
            return Completion::DuplicateIgnored {
                request_id: reg.request_id.clone(),
            };
        }
        if matches!(&outcome, ResponseOutcome::BusinessAccepted) {
            if reg.accepted_unconfirmed {
                self.audit.duplicate_ignored = self.audit.duplicate_ignored.saturating_add(1);
                return Completion::DuplicateIgnored {
                    request_id: reg.request_id.clone(),
                };
            }
            reg.accepted_unconfirmed = true;
            return Completion::AcceptedUnconfirmed {
                request_id: reg.request_id.clone(),
            };
        }
        *slot = Some(outcome.clone());
        Completion::Terminated {
            request_id: reg.request_id.clone(),
            outcome,
        }
    }

    /// 当前未终态请求(诊断/超时扫描用;调用方只读)。
    pub fn pending(&self) -> Vec<(Cookie, String)> {
        self.by_cookie
            .0
            .iter()
            .filter(|(_, _, o)| o.is_none())
            .map(|(c, r, _)| (*c, r.request_id.clone()))
            .collect()
    }

    pub fn wire_seq_for_request(&self, request_id: &str) -> Option<u32> {
        self.by_cookie
            .0
            .iter()
            .find(|(_, registration, _)| registration.request_id == request_id)
            .and_then(|(_, registration, _)| registration.wire_seq)
    }

    /// 终态取走(上层经此把结果交给 core;取走后条目移除,迟到回调转为
    /// UnknownCookie 审计 —— 终态只交付一次)。
    pub fn take_terminal(&mut self) -> Vec<(String, ResponseOutcome)> {
        let mut out = Vec::new();
        self.by_cookie.0.retain_mut(|(c, r, o)| {
            if let Some(res) = o.take() {
                let _ = c;
                out.push((r.request_id.clone(), res));
                return false; // 移除已交付条目
            }
            true
        });
        out
    }

    /// 关闭排空:未终态 → 显式 Unknown(DeliveryUnknown 口径),条目移除;
    /// 已终态条目保留(等 take_terminal 交付)。**不产生任何可被误读为
    /// "可重发"的信号**;重发决策只属于上层且默认不重发。
    pub fn drain(&mut self, reason: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (_, r, o) in self.by_cookie.0.iter_mut() {
            if o.is_none() {
                out.push((r.request_id.clone(), reason.to_string()));
            }
        }
        let drained: Vec<String> = out.iter().map(|(id, _)| id.clone()).collect();
        self.by_cookie.0.retain(|(_, r, o)| {
            if o.is_none() && drained.contains(&r.request_id) {
                self.audit.drained_unknown += 1;
                return false;
            }
            true
        });
        out
    }
}

/// 单调 seq 分配器(T09;计划 N2"持久化分配先于发起且记录保留")。
///
/// 合同:
/// - `allocate` 先持久化再交付;持久化失败 → 拒绝分配(调用方不得发起,
///   消除"未持久化就发起"的窗口)。
/// - 重启恢复从持久值继续(只前进);崩溃窗口造成的空洞保留 —— 已开始
///   发送的 seq 绝不二次分配(不自动重发的数值侧对应物)。
/// - 作用域:一个实例 = 一个计数域(当前证据:服务器不校验会话 seq,
///   唯一失败模式是自发撞车;r19 归档)。作用域扩展须过实际合同核对。
#[derive(Debug)]
pub struct SeqAllocator<P: FnMut(u64) -> Result<(), std::io::Error>> {
    next: u64,
    persist: P,
}

impl<P: FnMut(u64) -> Result<(), std::io::Error>> SeqAllocator<P> {
    /// `next` = 下一个要分配的值(恢复方传入持久值 +1;本构造函数不回读)。
    pub fn new(next: u64, persist: P) -> Self {
        Self { next, persist }
    }

    /// 分配:先持久化新值,成功才交付当前值并前进。
    pub fn allocate(&mut self) -> Result<u64, std::io::Error> {
        let v = self.next;
        (self.persist)(v)?;
        self.next = v.wrapping_add(1);
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn cookie(n: u8) -> Cookie {
        let mut c = [0u8; 16];
        c[0] = n;
        Cookie(c)
    }

    /// T06:inline 回调(发起返回前完成)可关联且只终结一次。
    #[test]
    fn inline_completion_terminates_once() {
        let mut l = ResponseLedger::new();
        l.set_generations(1, 1);
        l.register("req-1", cookie(1)).unwrap();
        // inline:submit 返回前响应已到。
        let c1 = l.complete(
            &cookie(1),
            ResponseOutcome::BusinessSuccess {
                native_id: "nat-1".into(),
            },
        );
        assert_eq!(
            c1,
            Completion::Terminated {
                request_id: "req-1".into(),
                outcome: ResponseOutcome::BusinessSuccess {
                    native_id: "nat-1".into()
                }
            }
        );
        // 迟到重复:只审计,不翻案。
        let c2 = l.complete(
            &cookie(1),
            ResponseOutcome::BusinessRejected {
                ret_code: 1,
                err: "late".into(),
            },
        );
        assert_eq!(
            c2,
            Completion::DuplicateIgnored {
                request_id: "req-1".into()
            }
        );
        assert_eq!(l.audit.duplicate_ignored, 1);
    }

    /// T07:三分类互不混用;成功必须携带稳定身份(类型强制,编译期)。
    #[test]
    fn three_way_outcomes_distinct() {
        let mut l = ResponseLedger::new();
        l.set_generations(2, 3);
        for (i, cid) in [1u8, 2, 3].iter().enumerate() {
            l.register(&format!("req-{i}"), cookie(*cid)).unwrap();
        }
        assert!(matches!(
            l.complete(
                &cookie(1),
                ResponseOutcome::BusinessSuccess {
                    native_id: "m1".into()
                }
            ),
            Completion::Terminated { .. }
        ));
        assert!(matches!(
            l.complete(
                &cookie(2),
                ResponseOutcome::BusinessRejected {
                    ret_code: -1,
                    err: "e".into()
                }
            ),
            Completion::Terminated { .. }
        ));
        assert!(matches!(
            l.complete(
                &cookie(3),
                ResponseOutcome::TransportFailed { reason: "t".into() }
            ),
            Completion::Terminated { .. }
        ));
        let done = l.take_terminal();
        assert_eq!(done.len(), 3);
        assert!(matches!(done[0].1, ResponseOutcome::BusinessSuccess { .. }));
        // 交付后迟到 → UnknownCookie 审计(条目已移除)。
        assert_eq!(
            l.complete(
                &cookie(1),
                ResponseOutcome::BusinessSuccess {
                    native_id: "x".into()
                }
            ),
            Completion::UnknownCookie
        );
    }

    #[test]
    fn business_acceptance_stays_pending_until_strong_identity_arrives() {
        let mut ledger = ResponseLedger::new();
        ledger.set_generations(1, 7);
        ledger.register("R-identity", cookie(14)).unwrap();
        assert_eq!(
            ledger.complete(&cookie(14), ResponseOutcome::BusinessAccepted),
            Completion::AcceptedUnconfirmed {
                request_id: "R-identity".into()
            },
        );
        assert_eq!(ledger.pending(), vec![(cookie(14), "R-identity".into())]);
        assert_eq!(
            ledger.complete(&cookie(14), ResponseOutcome::BusinessAccepted),
            Completion::DuplicateIgnored {
                request_id: "R-identity".into()
            },
        );
        assert_eq!(
            ledger.complete(
                &cookie(14),
                ResponseOutcome::BusinessSuccess {
                    native_id: "N-14".into()
                }
            ),
            Completion::Terminated {
                request_id: "R-identity".into(),
                outcome: ResponseOutcome::BusinessSuccess {
                    native_id: "N-14".into()
                },
            },
        );
        assert_eq!(
            ledger.take_terminal(),
            vec![(
                "R-identity".into(),
                ResponseOutcome::BusinessSuccess {
                    native_id: "N-14".into()
                },
            )]
        );
        assert!(ledger.pending().is_empty());
    }

    /// T08:旧代次回调只审计;登记绑定发起时代次。
    #[test]
    fn stale_generation_audited() {
        let mut l = ResponseLedger::new();
        l.set_generations(1, 5);
        l.register("old", cookie(9)).unwrap();
        l.set_generations(1, 6); // 会话代次推进
        assert_eq!(
            l.complete(
                &cookie(9),
                ResponseOutcome::BusinessSuccess {
                    native_id: "m".into()
                }
            ),
            Completion::StaleGeneration {
                request_id: "old".into()
            }
        );
        assert_eq!(l.audit.stale_generation, 1);
        // 旧代次结果不得翻案:pending 仍含 old?—— 否:登记绑定旧代次,
        // drain 时以 Unknown 收口(其真实结局不可知)。
        assert_eq!(l.pending().len(), 1);
    }

    /// T08:未知 cookie(未预登记)→ 审计,不归属任何请求。
    #[test]
    fn unknown_cookie_audited() {
        let mut l = ResponseLedger::new();
        l.set_generations(1, 1);
        assert_eq!(
            l.complete(
                &cookie(42),
                ResponseOutcome::TransportFailed { reason: "r".into() }
            ),
            Completion::UnknownCookie
        );
        assert_eq!(l.audit.unknown_cookie, 1);
    }

    /// T13:drain 把未终态表达为 Unknown;不产生可重发信号。
    #[test]
    fn drain_marks_unknown_and_never_resends() {
        let mut l = ResponseLedger::new();
        l.set_generations(1, 1);
        l.register("a", cookie(1)).unwrap();
        l.register("b", cookie(2)).unwrap();
        l.complete(
            &cookie(1),
            ResponseOutcome::BusinessRejected {
                ret_code: 5,
                err: "x".into(),
            },
        );
        let drained = l.drain("bridge closing");
        assert_eq!(
            drained,
            vec![("b".to_string(), "bridge closing".to_string())]
        );
        assert_eq!(l.audit.drained_unknown, 1);
        assert!(l.pending().is_empty());
        // 已终态(a)不被 drain 二次表达。
    }

    /// 预登记冲突:重复 cookie / 重复 request / 全零 cookie 拒绝。
    #[test]
    fn registration_conflicts_rejected() {
        let mut l = ResponseLedger::new();
        l.set_generations(1, 1);
        l.register("r1", cookie(1)).unwrap();
        assert_eq!(
            l.register("r2", cookie(1)),
            Err(RegisterError::DuplicateCookie)
        );
        assert_eq!(
            l.register("r1", cookie(2)),
            Err(RegisterError::DuplicateRequest)
        );
        assert_eq!(
            l.register("r3", Cookie([0u8; 16])),
            Err(RegisterError::ZeroCookie)
        );
    }

    /// T09:持久化先于交付;持久化失败不前进、不交付(调用方不得发起)。
    #[test]
    fn seq_allocator_persists_before_handout() {
        let log: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(Vec::new()));
        let l2 = log.clone();
        let mut a = SeqAllocator::new(100, move |v| {
            l2.borrow_mut().push(v);
            Ok(())
        });
        assert_eq!(a.allocate().unwrap(), 100);
        assert_eq!(a.allocate().unwrap(), 101);
        assert_eq!(*log.borrow(), vec![100, 101], "先持久化后交付");
    }

    #[test]
    fn seq_allocator_persist_failure_blocks_dispatch() {
        let mut a: SeqAllocator<Box<dyn FnMut(u64) -> Result<(), std::io::Error>>> =
            SeqAllocator::new(7, Box::new(|_v| Err(std::io::Error::other("disk full"))));
        assert!(a.allocate().is_err(), "持久化失败必须拒绝分配");
        // 计数不前进:修复后仍从 7 开始(未持久化的值绝不发出去过)。
        let mut ok = SeqAllocator::new(7, |_v| Ok(()));
        assert_eq!(ok.allocate().unwrap(), 7);
    }

    /// T09:重启恢复只前进;空洞保留(崩溃窗口的已发起 seq 不复用)。
    #[test]
    fn seq_recovery_moves_forward_only() {
        // 崩溃前持久到 105(实际发起 ≤104);恢复方以 105 起步。
        let mut a = SeqAllocator::new(105, |_v| Ok(()));
        assert_eq!(a.allocate().unwrap(), 105);
        assert_eq!(a.allocate().unwrap(), 106);
    }
}
