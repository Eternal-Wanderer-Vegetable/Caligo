//! K4-D4 常驻生命周期契约测试(计划 §7-D4 / §8.1:L07/L16/L17)。
//!
//! 自建宿主 = [`SharedMockHost`]:可控 owner 线程标识、env 有效性、
//! current 上下文、假会话服务语义(计数型)。全部用例零 QQ 依赖;
//! 进程内分配由计数分配器监控(L17 资源平衡)。
//!
//! 注意:本测试二进制安装了自己的 `#[global_allocator]`(计数分配器),
//! 仅用于本文件的资源增长断言。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{
    CloseReport, OwnedEvent, OwnedRequest, Resident, ResidentLimits, SubmitVerdict,
};

// ---- 计数分配器(L17:分配不随调度次数增长 / 关闭后回基线) ----

static LIVE: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

struct CountingAlloc;

unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: 前置转发到系统分配器。
        let p = unsafe { std::alloc::System.alloc(layout) };
        if !p.is_null() {
            LIVE.fetch_add(1, Ordering::Relaxed);
            TOTAL.fetch_add(1, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: 前置转发到系统分配器。
        unsafe { std::alloc::System.dealloc(ptr, layout) };
        LIVE.fetch_sub(1, Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn live_allocs() -> usize {
    LIVE.load(Ordering::Relaxed)
}

// ---- SharedMockHost:自建假宿主(测试夹具,无 QQ) ----

#[derive(Default)]
struct MockHostInner {
    env_valid: bool,
    ctx_ok: bool,
    current_thread: u64,
    native_calls: u64,
    failed_ops: u64,
    next_listener_token: u64,
    listener: Option<u64>,
    op_threads: Vec<u64>,
}

struct SharedMockHost {
    inner: Arc<std::sync::Mutex<MockHostInner>>,
    owner_thread: u64,
}

/// 测试侧控制面(与 resident 持有的 adapter 共享同一内部状态)。
#[derive(Clone)]
struct SharedControl {
    inner: Arc<std::sync::Mutex<MockHostInner>>,
}

impl SharedControl {
    fn set_env_valid(&self, ok: bool) {
        self.inner.lock().unwrap().env_valid = ok;
    }
    fn set_ctx_ok(&self, ok: bool) {
        self.inner.lock().unwrap().ctx_ok = ok;
    }
    /// 模拟外来线程执行 drain(错误线程)。
    fn masquerade_foreign_thread(&self) {
        self.inner.lock().unwrap().current_thread = 0x1BAD; // 非 owner
    }
    fn back_to_owner(&self) {
        self.inner.lock().unwrap().current_thread = OWNER;
    }
    fn native_calls(&self) -> u64 {
        self.inner.lock().unwrap().native_calls
    }
    fn listener_token(&self) -> Option<u64> {
        self.inner.lock().unwrap().listener
    }
    fn fail_next_native(&self) {
        self.inner.lock().unwrap().failed_ops += 1;
    }
    fn all_native_on_owner(&self) -> bool {
        self.inner.lock().unwrap().op_threads.iter().all(|t| *t == OWNER)
    }
}

const OWNER: u64 = 0xF00D;

impl SharedMockHost {
    fn new() -> (Self, SharedControl) {
        let inner = Arc::new(std::sync::Mutex::new(MockHostInner {
            env_valid: true,
            ctx_ok: true,
            current_thread: OWNER,
            next_listener_token: 5000,
            ..Default::default()
        }));
        (
            Self { inner: inner.clone(), owner_thread: OWNER },
            SharedControl { inner },
        )
    }
}

impl HostAdapter for SharedMockHost {
    fn owner_thread_id(&self) -> u64 {
        self.owner_thread
    }
    fn current_thread_id(&self) -> u64 {
        self.inner.lock().unwrap().current_thread
    }
    fn env_valid(&self) -> bool {
        self.inner.lock().unwrap().env_valid
    }
    fn current_context_ok(&self) -> bool {
        self.inner.lock().unwrap().ctx_ok
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        let mut inner = self.inner.lock().unwrap();
        // 防御纵深:实现内部自检(与 resident 层检查独立,两道防线)。
        if inner.current_thread != self.owner_thread {
            return Err(HostError::NotOwnerThread);
        }
        if !inner.env_valid {
            return Err(HostError::EnvInvalid);
        }
        if !inner.ctx_ok {
            return Err(HostError::NoCurrentContext);
        }
        inner.native_calls += 1;
        let tid = inner.current_thread;
        inner.op_threads.push(tid);
        if inner.failed_ops > 0 {
            inner.failed_ops -= 1;
            return Err(HostError::Native { code: 9 });
        }
        match op {
            HostOp::ListenerAdd => {
                inner.next_listener_token += 1;
                inner.listener = Some(inner.next_listener_token);
                Ok(HostOpResult::ListenerAdded {
                    token: inner.next_listener_token,
                })
            }
            HostOp::ListenerRemove { .. } => {
                inner.listener = None;
                Ok(HostOpResult::ListenerRemoved)
            }
            HostOp::Probe => Ok(HostOpResult::ProbeDone),
            HostOp::SendText { .. } => Ok(HostOpResult::Sent),
        }
    }
}

fn fixture() -> (Arc<Resident<SharedMockHost>>, SharedControl) {
    let (host, control) = SharedMockHost::new();
    let r = Arc::new(Resident::new(host, ResidentLimits::default()));
    (r, control)
}

// ---- 验收:bootstrap 一次;listener 注册在 owner;precheck 先于宿主 API ----

#[test]
fn bootstrap_once_and_listener_registered_on_owner() {
    let (r, ctl) = fixture();
    assert_eq!(ctl.native_calls(), 0, "bootstrap 前零宿主调用");
    let _tok = r.bootstrap().unwrap();
    assert_eq!(ctl.native_calls(), 1, "bootstrap = 恰一次 ListenerAdd");
    assert!(ctl.listener_token().is_some());
    // 二次 bootstrap:每宿主代次一次。
    assert!(r.bootstrap().is_err());
    assert_eq!(ctl.native_calls(), 1);
    assert!(ctl.all_native_on_owner());
}

// ---- L07:外来线程 / 零上下文 / env 失效 —— 一切宿主 API 之前拒绝 ----

#[test]
fn l07_foreign_thread_and_zero_context_rejected_before_any_host_api() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    r.submit(OwnedRequest::Probe { id: 1 });
    // 外来线程 drain:precheck 在任何 native 之前拒绝,队列原样保留。
    ctl.masquerade_foreign_thread();
    assert!(matches!(r.drain(), Err(HostError::NotOwnerThread)));
    assert_eq!(ctl.native_calls(), 1, "仅 bootstrap 一次调用,零业务 native");
    assert_eq!(r.pending_ingress(), 1, "被拒项不得静默丢弃");
    ctl.back_to_owner();

    // current 缺失(零上下文):同样在任何宿主 API 之前拒绝。
    r.submit(OwnedRequest::Probe { id: 2 });
    ctl.set_ctx_ok(false);
    assert!(matches!(r.drain(), Err(HostError::NoCurrentContext)));
    assert_eq!(ctl.native_calls(), 1);
    ctl.set_ctx_ok(true);

    // env 销毁:拒绝;恢复后可继续(队列未丢)。
    ctl.set_env_valid(false);
    assert!(matches!(r.drain(), Err(HostError::EnvInvalid)));
    ctl.set_env_valid(true);
    assert_eq!(r.drain().unwrap(), 2, "恢复后两条都执行");
    assert_eq!(ctl.native_calls(), 3);
    assert!(ctl.all_native_on_owner());
}

// ---- L16:关闭协议 + 迟回调拒绝 + 幂等 double close ----

#[test]
fn l16_close_protocol_and_late_callbacks() {
    let (r, ctl) = fixture();
    let tok = r.bootstrap().unwrap();
    r.submit(OwnedRequest::SendText { request_id: "a".into(), text_len: 2 });
    r.submit(OwnedRequest::SendText { request_id: "b".into(), text_len: 3 });
    assert_eq!(r.drain().unwrap(), 2);

    let report = r.close().unwrap();
    assert_eq!(
        report,
        CloseReport {
            cancelled_unsent: 0,
            marked_unknown: 0,
            listener_removed: Some(true),
            late_callbacks_rejected: 0,
            already_closed: false,
        }
    );
    assert!(ctl.listener_token().is_none(), "监听器对称移除并确认");
    assert_eq!(r.submit(OwnedRequest::Probe { id: 99 }), SubmitVerdict::Closed);

    // 迟到回调(已关闭):拒绝且零 native。
    let before = ctl.native_calls();
    assert!(r
        .deliver_callback(tok, OwnedEvent { native_id: "late".into(), text_len: 1 })
        .is_err());
    assert_eq!(ctl.native_calls(), before, "迟回调零宿主触碰");

    // double close:幂等。
    let again = r.close().unwrap();
    assert!(again.already_closed);
}

#[test]
fn close_cancels_queued_and_marks_failed_native_unknown() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    // native 失败(结果不确定)→ delivery_unknown,不宣称"未发送"。
    ctl.fail_next_native();
    r.submit(OwnedRequest::SendText { request_id: "inflight".into(), text_len: 1 });
    assert_eq!(r.drain().unwrap(), 1);
    assert_eq!(r.request_state("inflight"), Some("delivery_unknown"));
    // 排队未执行项随 close 取消。
    r.submit(OwnedRequest::SendText { request_id: "queued".into(), text_len: 1 });
    let report = r.close().unwrap();
    assert_eq!(report.cancelled_unsent, 1);
    assert_eq!(r.request_state("queued"), Some("cancelled_unsent"));
    assert_eq!(report.marked_unknown, 0, "inflight 已在 drain 时显式化");
}

#[test]
fn listener_removal_failure_quarantines_instead_of_fake_success() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    ctl.fail_next_native(); // close 内的 ListenerRemove 将失败
    assert!(r.close().is_err());
    assert!(
        r.quarantine_reason(),
        "无法证明清理 → Quarantined,不当作正常关闭"
    );
    // 隔离态不可洗白为正常关闭。
    assert!(r.close().is_err());
}

// ---- L17:10,000 次非发送调度 + 100 轮关闭/重建,资源回基线 ----

#[test]
fn l17_ten_thousand_probes_no_growth_and_counters_balance() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    assert_eq!(r.counters().native_ops_total, 1, "bootstrap 的 ListenerAdd 计入 native 总数");
    // 预热(排除首次建表的一次性分配)。
    for i in 0..500u64 {
        r.submit(OwnedRequest::Probe { id: i });
        assert_eq!(r.drain().unwrap(), 1);
    }
    let warm_live = live_allocs();
    let warm_total = TOTAL.load(Ordering::Relaxed);

    for i in 500..10_500u64 {
        r.submit(OwnedRequest::Probe { id: i });
        assert_eq!(r.drain().unwrap(), 1);
    }
    let total_delta = TOTAL.load(Ordering::Relaxed) - warm_total;
    // 稳态:每调度 ≤ 2 次分配(排队元组),且 live 不随轮数增长。
    assert!(
        total_delta <= 2 * 10_000 + 1_000,
        "稳态分配异常增长: {total_delta}"
    );
    assert!(
        live_allocs() <= warm_live + 64,
        "live 分配随调度增长: {} -> {}",
        warm_live,
        live_allocs()
    );
    let c = r.counters();
    assert_eq!(c.dispatched_total, 10_500);
    assert_eq!(c.native_ops_total, 10_501);
    assert_eq!(r.pending_ingress(), 0);
    let report = r.close().unwrap();
    assert_eq!(report.listener_removed, Some(true));
    assert_eq!(ctl.listener_token(), None);
}

#[test]
fn l17_hundred_close_rebuild_cycles_return_to_baseline() {
    let live_before = live_allocs();
    for cycle in 0..100u64 {
        let (r, ctl) = fixture();
        r.bootstrap().unwrap();
        r.submit(OwnedRequest::Probe { id: cycle });
        assert_eq!(r.drain().unwrap(), 1);
        let report = r.close().unwrap();
        assert_eq!(report.listener_removed, Some(true));
        assert!(ctl.listener_token().is_none());
        let c = r.counters();
        assert_eq!(c.listener_adds, 1);
        assert_eq!(c.listener_removes, 1);
    }
    // 100 轮后 live 分配回基线(允许少量常量残差)。
    let after = live_allocs();
    assert!(
        after <= live_before + 128,
        "100 轮后资源未回基线: before={live_before} after={after}"
    );
}

// ---- 跨线程:任意线程提交、owner 执行(TSFN 语义) ----

#[test]
fn submit_from_worker_threads_executes_only_on_owner() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    let mut handles = Vec::new();
    // 总提交量 = 队列上限(32):生产在 worker,消费在主线程的 drain 之外。
    for w in 0..4u64 {
        let rr = r.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..8u64 {
                assert_eq!(
                    rr.submit(OwnedRequest::Probe { id: w * 8 + i }),
                    SubmitVerdict::Accepted
                );
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let mut drained = 0;
    loop {
        let n = r.drain().unwrap();
        drained += n;
        if n == 0 {
            break;
        }
    }
    assert_eq!(drained, 32);
    assert!(ctl.all_native_on_owner(), "全部 native 调用都在 owner 线程");
    r.close().unwrap();
}

// ---- 有界队列:满即明确拒绝;drain 分批,有剩余可继续调度 ----

#[test]
fn bounded_queues_reject_explicitly_and_drain_batches() {
    let (host, _ctl) = SharedMockHost::new();
    // 队列上限(40)> 单轮批次(16):验证分批与"不依赖新消息唤醒"。
    let r = Arc::new(Resident::new(
        host,
        ResidentLimits { ingress_max: 40, events_max: 256, drain_batch: 16 },
    ));
    r.bootstrap().unwrap();
    for i in 0..40u64 {
        assert_eq!(r.submit(OwnedRequest::Probe { id: i }), SubmitVerdict::Accepted);
    }
    assert_eq!(
        r.submit(OwnedRequest::Probe { id: 9999 }),
        SubmitVerdict::QueueFull,
        "队列满必须显式拒绝"
    );
    assert_eq!(r.drain().unwrap(), 16);
    assert_eq!(r.pending_ingress(), 24);
    assert_eq!(r.drain().unwrap(), 16, "有剩余时不依赖新消息唤醒");
    assert_eq!(r.drain().unwrap(), 8);
    assert_eq!(r.drain().unwrap(), 0);
}

// ---- 重复提交同 ID send:查询语义,不重复派发 ----

#[test]
fn duplicate_send_request_id_is_not_redispatched() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    let req = OwnedRequest::SendText { request_id: "dup".into(), text_len: 5 };
    assert_eq!(r.submit(req.clone()), SubmitVerdict::Accepted);
    assert_eq!(
        r.submit(req),
        SubmitVerdict::Accepted,
        "重复 ID 接受但不重复入队"
    );
    assert_eq!(r.pending_ingress(), 1);
    assert_eq!(r.drain().unwrap(), 1);
    let calls = ctl.native_calls();
    assert_eq!(r.request_state("dup"), Some("done"));
    r.drain().unwrap();
    assert_eq!(ctl.native_calls(), calls, "第二次 drain 不会再次执行 dup");
}

// ---- env 销毁后:业务调度拒绝且零 native(仅 bootstrap 的 1 次存在) ----

#[test]
fn after_env_destroyed_business_dispatch_touches_nothing() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    ctl.set_env_valid(false);
    r.submit(OwnedRequest::Probe { id: 1 });
    assert!(matches!(r.drain(), Err(HostError::EnvInvalid)));
    assert_eq!(ctl.native_calls(), 1, "仅 bootstrap;业务零触碰");
}
