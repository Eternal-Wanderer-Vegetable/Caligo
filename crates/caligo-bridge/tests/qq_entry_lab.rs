//! K4-D7 候选 B 首入机器 LAB 测试(research 构建)。
//!
//! 用**假 QQNT 符号**(Rust 函数,ABI 与真实导出一致)驱动同一台首入状态机:
//! 机器逻辑(阶段顺序、零 current 拒绝、owner 线程断言、关闭协议)在此验证;
//! 真实 QQNT 地址/ABI 属 D7 现场证据(B0-FIELD),不由本文件替代。
//!
//! 机器是进程单实例(§6.1),用例间经 `test_reset_state` + 全局锁隔离;
//! 运行:`cargo test -p caligo-bridge --features research`。
#![cfg(feature = "research")]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::qq_entry::{bootstrap, EntryConfig, EntryError, EntrySymbols, QqOwnerAdapter};
use caligo_bridge::resident::{Resident, ResidentLimits};

static TEST_LOCK: Mutex<()> = Mutex::new(());

/// 用例隔离:机器静态 + 假符号计数器一并清零。
fn lab_reset() {
    caligo_bridge::qq_entry::test_reset_state();
    FAKE_LOOP.store(0, Ordering::Release);
    FAKE_EXPECTED_ISOLATE.store(0, Ordering::Release);
    FAKE_CURRENT.store(0, Ordering::Release);
    FAKE_INIT_CALLS.store(0, Ordering::Relaxed);
    FAKE_SEND_CALLS.store(0, Ordering::Relaxed);
    FAKE_CLOSE_CALLS.store(0, Ordering::Relaxed);
    FAKE_RI_CALLS.store(0, Ordering::Relaxed);
    FAKE_INIT_CB.store(0, Ordering::Release);
    FAKE_CLOSE_CB.store(0, Ordering::Release);
    DRAIN_CALLS.store(0, Ordering::Relaxed);
}

// ---- 假 QQNT 内存布局(env / IsolateData / isolate) ----

struct FakeQq {
    // 保持分配存活(页校验要求真实可读内存)。
    _env_mem: Box<[u8; 0x200]>,
    _isolate_data: Box<[u8; 0x1200]>,
    _isolate_ptr: Box<u64>,
}

impl FakeQq {
    fn new() -> (Self, usize, usize, usize) {
        let mut env_mem = Box::new([0u8; 0x200]);
        let mut isolate_data = Box::new([0u8; 0x1200]);
        let isolate_inner = Box::into_raw(Box::new(0x1DEA_0001u64));
        let qqnt_base = 0x0000_5100_0000_0000usize; // 任意假基址
        let env = env_mem.as_mut_ptr() as usize;
        let idata = isolate_data.as_mut_ptr() as usize;
        let isolate = isolate_inner as usize;
        // 新鲜度门:env[0] = qqnt_base + 主 vtable RVA。
        let vfptr = (qqnt_base as u64).wrapping_add(caligo_bridge::exec::EXPECTED_VTABLE_RVA as u64);
        env_mem[0..8].copy_from_slice(&vfptr.to_le_bytes());
        // env+0xA0 = isolate;env+0xB0 = IsolateData*;IsolateData+0x11E8 = loop。
        env_mem[0xA0..0xA8].copy_from_slice(&(isolate as u64).to_le_bytes());
        env_mem[0xB0..0xB8].copy_from_slice(&(idata as u64).to_le_bytes());
        let loop_ptr = 0x5E17_0002usize;
        isolate_data[0x11E8..0x11F0].copy_from_slice(&(loop_ptr as u64).to_le_bytes());
        FAKE_LOOP.store(loop_ptr, Ordering::Release);
        FAKE_EXPECTED_ISOLATE.store(isolate, Ordering::Release);
        FAKE_CURRENT.store(isolate, Ordering::Release);
        (
            Self {
                _env_mem: env_mem,
                _isolate_data: isolate_data,
                _isolate_ptr: unsafe { Box::from_raw(isolate_inner) },
            },
            qqnt_base,
            env,
            loop_ptr,
        )
    }
}

// ---- 假符号(签名与真实导出一致;行为由静态记录) ----

static FAKE_LOOP: AtomicUsize = AtomicUsize::new(0);
static FAKE_EXPECTED_ISOLATE: AtomicUsize = AtomicUsize::new(0);
static FAKE_CURRENT: AtomicUsize = AtomicUsize::new(0);
static FAKE_INIT_CALLS: AtomicU64 = AtomicU64::new(0);
static FAKE_SEND_CALLS: AtomicU64 = AtomicU64::new(0);
static FAKE_CLOSE_CALLS: AtomicU64 = AtomicU64::new(0);
static FAKE_RI_CALLS: AtomicU64 = AtomicU64::new(0);
static FAKE_INIT_CB: AtomicUsize = AtomicUsize::new(0);
static FAKE_CLOSE_CB: AtomicUsize = AtomicUsize::new(0);
static DRAIN_CALLS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn fake_uv_handle_size(_t: i32) -> usize {
    64
}

unsafe extern "C" fn fake_isolate_get_current() -> *mut core::ffi::c_void {
    FAKE_CURRENT.load(Ordering::Acquire) as *mut core::ffi::c_void
}

unsafe extern "C" fn fake_uv_async_init(
    loop_: *mut core::ffi::c_void,
    _handle: *mut core::ffi::c_void,
    cb: unsafe extern "C" fn(*mut core::ffi::c_void),
) -> i32 {
    FAKE_INIT_CALLS.fetch_add(1, Ordering::Relaxed);
    FAKE_INIT_CB.store(cb as usize, Ordering::Release);
    if loop_ as usize != FAKE_LOOP.load(Ordering::Acquire) {
        return -1; // loop 不符:机器必须报 UvInitFailed
    }
    0
}

unsafe extern "C" fn fake_uv_async_send(_h: *mut core::ffi::c_void) -> i32 {
    FAKE_SEND_CALLS.fetch_add(1, Ordering::Relaxed);
    0
}

unsafe extern "C" fn fake_uv_close(
    _h: *mut core::ffi::c_void,
    cb: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
) {
    FAKE_CLOSE_CALLS.fetch_add(1, Ordering::Relaxed);
    if let Some(cb) = cb {
        FAKE_CLOSE_CB.store(cb as usize, Ordering::Release);
    }
}

unsafe extern "system" fn fake_request_interrupt(
    _env: *mut core::ffi::c_void,
    cb: unsafe extern "system" fn(*mut core::ffi::c_void),
    ctx: *mut core::ffi::c_void,
) {
    FAKE_RI_CALLS.fetch_add(1, Ordering::Relaxed);
    // 假宿主:同步在当前线程执行(真实为 JS 线程异步;机器以状态机等待)。
    cb(ctx);
}

fn fake_symbols() -> EntrySymbols {
    // 先收敛为函数指针再转 usize(函数项是 ZST,不可直接转整型)。
    type FnUvAsyncInit = unsafe extern "C" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        unsafe extern "C" fn(*mut core::ffi::c_void),
    ) -> i32;
    type FnUvAsyncSend = unsafe extern "C" fn(*mut core::ffi::c_void) -> i32;
    type FnUvHandleSize = unsafe extern "C" fn(i32) -> usize;
    type FnUvClose = unsafe extern "C" fn(
        *mut core::ffi::c_void,
        Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
    );
    type FnIsolateGetCurrent = unsafe extern "C" fn() -> *mut core::ffi::c_void;
    type FnRequestInterrupt = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        unsafe extern "system" fn(*mut core::ffi::c_void),
        *mut core::ffi::c_void,
    );
    let a1: FnUvAsyncInit = fake_uv_async_init;
    let a2: FnUvAsyncSend = fake_uv_async_send;
    let a3: FnUvHandleSize = fake_uv_handle_size;
    let a4: FnUvClose = fake_uv_close;
    let a5: FnIsolateGetCurrent = fake_isolate_get_current;
    let a6: FnRequestInterrupt = fake_request_interrupt;
    EntrySymbols::from_raw(a1 as usize, a2 as usize, a3 as usize, a4 as usize, a5 as usize, a6 as usize)
}

fn report_path(tag: &str) -> String {
    let p = std::env::temp_dir().join(format!("caligo-qqentry-{tag}-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

/// 模拟 QQ loop 的一个轮转点(真实由宿主事件循环调用;LAB 由测试直调)。
fn pump_once() {
    let cb = FAKE_INIT_CB.load(Ordering::Acquire);
    assert!(cb != 0, "pump 未注册");
    // SAFETY: 指针来自 fake_uv_async_init 记录的回调。
    unsafe {
        let f: unsafe extern "C" fn(*mut core::ffi::c_void) =
            std::mem::transmute::<usize, unsafe extern "C" fn(*mut core::ffi::c_void)>(cb);
        f(std::ptr::null_mut());
    }
}

fn close_once() {
    let cb = FAKE_CLOSE_CB.load(Ordering::Acquire);
    assert!(cb != 0, "close 回调未注册");
    // SAFETY: 指针来自 fake_uv_close 记录的回调。
    unsafe {
        let f: unsafe extern "C" fn(*mut core::ffi::c_void) =
            std::mem::transmute::<usize, unsafe extern "C" fn(*mut core::ffi::c_void)>(cb);
        f(std::ptr::null_mut());
    }
}

// ---- 用例 ----

#[test]
fn staged_bootstrap_wake_and_owner_pump() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("full") };
    let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
    assert_eq!(FAKE_INIT_CALLS.load(Ordering::Relaxed), 1, "uv_async_init 恰一次(常驻)");
    assert_eq!(FAKE_RI_CALLS.load(Ordering::Relaxed), 1);
    assert!(!handle.quarantine());

    // owner 轮转点:drain 钩子被调用。
    handle.set_drain_hook(|| {
        DRAIN_CALLS.fetch_add(1, Ordering::Relaxed);
        1
    });
    assert!(handle.wake());
    assert_eq!(FAKE_SEND_CALLS.load(Ordering::Relaxed), 1);
    pump_once();
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), 1);
    assert!(handle.pumps() >= 1);
}

#[test]
fn close_completes_via_pump_and_close_callback() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("close") };
    // bootstrap 在"owner 线程"上执行(假宿主同步首入 → ENTRY.tid = 该线程);
    // 之后轮转点也必须由同一线程驱动(pump 的 owner 断言是机器契约)。
    let (tx_handle, rx_handle) = std::sync::mpsc::channel();
    let (tx_drive, rx_drive) = std::sync::mpsc::channel::<()>();
    let owner = std::thread::spawn(move || {
        let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
        tx_handle.send(handle).unwrap();
        // 等主线程发起 close 后,由本线程(= owner)驱动轮转点与关闭回调。
        rx_drive.recv().unwrap();
        pump_once(); // 处理 CLOSE_REQ → uv_close
        close_once(); // 关闭回调 → closed
    });
    let handle = std::sync::Arc::new(rx_handle.recv().unwrap());
    let closer = {
        let h = handle.clone();
        std::thread::spawn(move || h.close(3000))
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    tx_drive.send(()).unwrap();
    let r = closer.join().unwrap();
    owner.join().unwrap();
    r.expect("close protocol completes");
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 1);
}

#[test]
fn zero_current_context_aborts_before_any_host_api() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    FAKE_CURRENT.store(0, Ordering::Release); // 零 current:不得放行
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("zerocur") };
    let err = bootstrap(&cfg, &fake_symbols()).unwrap_err();
    assert_eq!(err, EntryError::InterruptNoCurrentContext);
    assert_eq!(
        FAKE_INIT_CALLS.load(Ordering::Relaxed),
        0,
        "零 current 下不得 init 任何宿主资源"
    );
    FAKE_CURRENT.store(FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire), Ordering::Release);
}

#[test]
fn stale_env_fails_freshness_gate_before_interrupt() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    // 篡改 vfptr:陈旧 env(I-03 语义)。
    // SAFETY: env 是测试持有的真实内存。
    unsafe {
        *(env as *mut u64) ^= 0xFF;
    }
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("stale") };
    let err = bootstrap(&cfg, &fake_symbols()).unwrap_err();
    assert_eq!(err, EntryError::EnvNotFresh);
    assert_eq!(FAKE_RI_CALLS.load(Ordering::Relaxed), 0, "新鲜度门先于 RequestInterrupt");
}

#[test]
fn adapter_rejects_unproven_ops_and_resident_deferred_mode() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("adapter") };
    let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
    let mut adapter = QqOwnerAdapter::new(handle);
    assert_eq!(
        adapter.owner_thread_id(),
        adapter.current_thread_id(),
        "假宿主同步首入:同线程"
    );
    // Probe:通过(新鲜度 + current)。
    assert!(adapter.native_op(HostOp::Probe).is_ok());
    // D8/D9 面:显式拒绝 / 延迟,不冒充能力。
    assert_eq!(
        adapter.native_op(HostOp::ListenerAdd).unwrap(),
        HostOpResult::ListenerDeferred
    );
    assert!(matches!(
        adapter.native_op(HostOp::ListenerRemove { token: 1 }),
        Err(HostError::NoProvenRoute)
    ));
    assert!(matches!(
        adapter.native_op(HostOp::SendText { text_len: 1 }),
        Err(HostError::NoProvenRoute)
    ));

    // resident 以延迟监听模式 bootstrap/close(关闭不做对称移除)。
    let resident = Resident::new(adapter, ResidentLimits::default());
    resident.bootstrap().unwrap();
    let report = resident.close().unwrap();
    assert_eq!(report.listener_removed, None, "延迟模式:无监听器可移除");
}
