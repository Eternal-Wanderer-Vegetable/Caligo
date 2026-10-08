//! G2 接收:单观察者指针交换(设计:`qq-native-receive-design.md` §2b)。
//!
//! OnRecv(`1B41AE6`)在 push 路径上通知 `this+0x140` 的单观察者:
//! `obs->vtbl[1](obs, &msg_pair)`(msg_pair = {obj, ctrl} 借用,调用期内有效)。
//! 本模块:保存原观察者 O0 → 原子写入自有对象(slot1 = 复制 + 转发)→
//! 窗口结束恢复 O0。**零发送**;回调边界内不落盘、不传播 panic。
//!
//! 纪律:
//! - 转发保真:O0 的 slot1 在我们的处理器内被调用,QQ 行为不变;
//! - 证据:msg 对象头 0x40 字节 opaque hexdump(captured fixture,P5 字段
//!   验证回填用)——**不解析、不猜字段**;
//! - 环形缓冲:回调只做内存拷贝,落盘由观测线程在窗口结束后统一进行。

use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::Mutex;

/// getter 机器级原型(72DE38;同 g1)。
type GetterFn = unsafe extern "system" fn(out: *mut ServicePair) -> *mut ServicePair;

/// OnRecv 通知点:MSFService 对象 +0x140(单观察者指针)。
pub const FIELD_OBSERVER: usize = 0x140;
/// GETTER_RVA(同 g1)。
const GETTER_RVA: usize = 0x72DE38;
const ANCHOR_SVC_VTBL_RVA: usize = 0x3F6DED8;

pub const OK: u32 = 0;
pub const ERR_NO_OBSERVER: u32 = 21;
pub const ERR_SWAP_STATE: u32 = 22;
pub const ERR_NO_INNER: u32 = 23;

#[repr(C)]
pub struct G2Ctx {
    pub wrapper_base: usize,
    pub report_path: *const u16,
    pub observe_ms: u32,
}

/// 捕获环(回调侧 push,窗口结束 drain)。有界:满则丢弃并计数。
struct Ring {
    items: Mutex<Vec<(u64, u64)>>, // (msg_obj_ptr 前 8 字节, 次八字节)
    dropped: AtomicU64,
    pushed: AtomicU64,
    cap: usize,
}

static RING: std::sync::OnceLock<Ring> = std::sync::OnceLock::new();

fn ring() -> &'static Ring {
    RING.get_or_init(|| Ring {
        items: Mutex::new(Vec::new()),
        dropped: AtomicU64::new(0),
        pushed: AtomicU64::new(0),
        cap: 512,
    })
}

/// O0(原观察者)——交换期间由本模块独占写。
static O0: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// 我们的单一对象实例:vptr 槽 + 冗余(地址稳定性由 static 保证)。
static mut OUR_OBJECT: [*mut c_void; 4] = [core::ptr::null_mut(); 4];
static OUR_OBJECT_READY: AtomicU64 = AtomicU64::new(0);

/// 我们的 vtable:slot0..slot15;slot1 = 处理器,其余 = no-op。
static mut OUR_VTABLE: [usize; 16] = [0; 16];

unsafe extern "system" fn g2_noop(_this: *mut c_void) -> u64 {
    0
}

unsafe extern "system" fn g2_noop2(_a: *mut c_void, _b: *mut c_void) {}

/// slot1 处理器:捕获 msg 对象头 → 原样转发 O0。
unsafe extern "system" fn g2_slot1(_this: *mut c_void, msg_pair: *mut c_void) {
    // SAFETY: 边界内只做有界内存读 + ring 拷贝 + 转发;panic 隔离。
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !msg_pair.is_null() {
            let pair = *(msg_pair as *const [u64; 2]); // {obj, ctrl}
            let obj = pair[0] as usize;
            if obj != 0 {
                // 捕获头 0x40 字节(opaque;16 qword)。
                let mut head = [0u64; 8];
                for (i, h) in head.iter_mut().enumerate() {
                    // SAFETY: QQ 构造的 msg 对象;读失败保持 0。
                    *h = unsafe { core::ptr::read_volatile((obj + i * 8) as *const u64) };
                }
                let r = ring();
                let mut q = r.items.lock().unwrap();
                if q.len() < r.cap {
                    // 打包 8 qword → 4 条 (u64,u64) 记录(顺序保留)。
                    for c in 0..4 {
                        q.push((head[c * 2], head[c * 2 + 1]));
                    }
                    r.pushed.fetch_add(4, Ordering::Relaxed);
                } else {
                    r.dropped.fetch_add(1, Ordering::Relaxed);
                }
                drop(q);
            }
        }
        // 转发 O0(slot1 原语义)。
        let o0 = O0.load(Ordering::Acquire);
        if !o0.is_null() {
            let vptr = unsafe { core::ptr::read_volatile(o0 as *const usize) };
            let slot = unsafe { core::ptr::read_volatile((vptr + 8) as *const usize) };
            let f: unsafe extern "system" fn(*mut c_void, *mut c_void) =
                unsafe { core::mem::transmute(slot) };
            unsafe { f(o0, msg_pair) };
        }
    }));
}

fn init_object() -> *mut c_void {
    // SAFETY: OUR_OBJECT/OUR_VTABLE 仅在启动阶段由本函数初始化一次
    // (单远程线程;READY 原子闸防重入),此后只读。
    unsafe {
        if OUR_OBJECT_READY.load(Ordering::Acquire) == 0 {
            OUR_VTABLE[0] = g2_noop as usize;
            OUR_VTABLE[1] = g2_slot1 as usize;
            for slot in OUR_VTABLE.iter_mut().skip(2) {
                *slot = g2_noop2 as usize;
            }
            OUR_OBJECT[0] = OUR_VTABLE.as_mut_ptr() as *mut c_void;
            OUR_OBJECT_READY.store(1, Ordering::Release);
        }
        OUR_OBJECT.as_mut_ptr() as *mut c_void
    }
}

/// 阶段日志(同 g1 独立实现)。
fn stage(path: *const u16, stage: &str, ok: bool, detail: &str) {
    // SAFETY: path 由 loader 写入,NUL 结尾 UTF-16。
    let text = unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        if len >= 32 * 1024 {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(path, len)))
        }
    };
    if let Some(p) = text {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        let line = format!(
            "{{\"ts\":{},\"tid\":{},\"stage\":\"{}\",\"ok\":{},\"detail\":\"{}\"}}\n",
            ts,
            tid,
            stage,
            ok,
            detail.replace('\\', "\\\\").replace('"', "'")
        );
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// G2 监听运行(有界同步;同 g1_native_run 形态):
/// 1. getter 租约 → anchor 核验;
/// 2. 读 O0 = *(obj+0x140)(null → 失败;记录 O0 vptr);
/// 3. 原子交换为我们的对象 → 观测窗口(capture 环就绪,回调转发 O0);
/// 4. 恢复 O0 → drain 环到 JSONL(captured fixture)。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`G2Ctx`]。
pub unsafe fn g2_listen_run(ctx: *const G2Ctx) -> u32 {
    // SAFETY: ctx 由 loader 写入。
    let (wrapper_base, report_path, observe_ms) =
        unsafe { ((*ctx).wrapper_base, (*ctx).report_path, (*ctx).observe_ms) };
    let observe_ms = observe_ms.min(120_000).max(500);

    // 租约。
    let getter: GetterFn = unsafe { core::mem::transmute(wrapper_base + GETTER_RVA) };
    let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
    // SAFETY: getter 契约(R2 §4)。
    let ret = unsafe { getter(&mut pair as *mut ServicePair) };
    if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
        stage(report_path, "g2_adopt", false, "getter failed");
        return ERR_NO_OBSERVER;
    }
    let vptr = unsafe { core::ptr::read_volatile(pair.obj as *const u64) };
    if (vptr as usize).wrapping_sub(wrapper_base) != ANCHOR_SVC_VTBL_RVA {
        stage(report_path, "g2_adopt", false, "anchor mismatch");
        return ERR_SWAP_STATE;
    }
    stage(report_path, "g2_adopt", true, &format!("obj={}", pair.obj as u64));

    // O0 读取 + 交换。
    let o0: *mut c_void =
        unsafe { core::ptr::read_volatile((pair.obj as usize + FIELD_OBSERVER) as *mut *mut c_void) };
    if o0.is_null() {
        stage(report_path, "o0", false, "*(obj+0x140) null (observer not registered yet)");
        return ERR_NO_OBSERVER;
    }
    let o0_vptr = unsafe { core::ptr::read_volatile(o0 as *const u64) };
    stage(
        report_path,
        "o0",
        true,
        &format!("addr={} vptr={} vptr_rva={:#x}", o0 as u64, hex64(o0_vptr), (o0_vptr as usize).wrapping_sub(wrapper_base)),
    );
    let ours = init_object();
    // 原子交换:x64 对齐指针的 xchg 为单指令;此处读-改-写经 AtomicPtr。
    let slot = (pair.obj as usize + FIELD_OBSERVER) as *mut AtomicPtr<c_void>;
    let prev = unsafe { (*slot).swap(ours, Ordering::AcqRel) };
    O0.store(o0, Ordering::Release);
    stage(report_path, "swap", prev == o0, &format!("prev={}", prev as u64));

    // 观测窗口。
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(observe_ms as u64);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // 恢复(仅当仍是我们的对象;O0 变化则如实记录不覆盖)。
    let cur: *mut c_void =
        unsafe { core::ptr::read_volatile((pair.obj as usize + FIELD_OBSERVER) as *mut *mut c_void) };
    if cur == ours {
        unsafe {
            core::ptr::write_volatile(
                (pair.obj as usize + FIELD_OBSERVER) as *mut *mut c_void,
                o0,
            );
        }
        stage(report_path, "restore", true, "O0 restored");
    } else {
        stage(report_path, "restore", false, &format!("slot changed during window: {:#x}", cur as u64));
    }

    // drain 环 → captured fixture。
    let r = ring();
    let q = r.items.lock().unwrap();
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(report_path_str(report_path)) {
        for (i, (a, b)) in q.iter().enumerate() {
            let _ = writeln!(f, "{{\"kind\":\"msg_head\",\"i\":{},\"q0\":{a},\"q1\":{b}}}", i / 4);
        }
        let _ = writeln!(
            f,
            "{{\"kind\":\"summary\",\"pushed\":{},\"dropped\":{}}}",
            r.pushed.load(Ordering::Relaxed),
            r.dropped.load(Ordering::Relaxed)
        );
    }
    drop(q);
    stage(report_path, "g2_done", true, &format!("captured={}", r.pushed.load(Ordering::Relaxed)));
    OK
}

fn hex64(v: u64) -> String {
    format!("{v:#x}")
}

fn report_path_str(path: *const u16) -> String {
    // SAFETY: loader 写入的 NUL 结尾 UTF-16。
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(path, len))
    }
}

/// getter 输出 pair(与 g1 同几何;避免跨模块私有依赖)。
#[repr(C)]
#[derive(Clone, Copy)]
struct ServicePair {
    obj: *mut c_void,
    ctrl: *mut c_void,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex as SyncMutex;

    /// 假 MSFService:+0x140 槽 + O0(vtbl slot1 记录转发调用)。
    struct FakeSetup {
        svc_obj: *mut c_void,
        o0_obj: *mut c_void,
        forwarded: Arc<SyncMutex<Vec<u64>>>, // O0 收到的 msg obj 指针
    }

    unsafe extern "system" fn fake_o0_slot1(_this: *mut c_void, msg_pair: *mut c_void) {
        // SAFETY: 测试构造的 pair。
        unsafe {
            let pair = *(msg_pair as *const [u64; 2]);
            // 全局转存(测试单线程)。
            FORWARDED.with(|f| f.borrow_mut().push(pair[0]));
        }
    }

    thread_local! {
        static FORWARDED: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    #[test]
    fn swap_forward_restore_symmetry() {
        // O0:vptr → vtable(slot1 = fake_o0_slot1)。
        let mut o0_vtable: [usize; 4] = [0; 4];
        o0_vtable[1] = fake_o0_slot1 as usize;
        let o0_storage = Box::leak(Box::new([o0_vtable.as_mut_ptr() as *mut c_void]));
        let o0_obj = o0_storage.as_mut_ptr() as *mut c_void;

        // 假 MSFService:+0x140 = O0。
        let mut svc = vec![0u64; 0x148 / 8];
        svc[FIELD_OBSERVER / 8] = o0_obj as u64;
        let svc_ptr = svc.as_mut_ptr() as *mut c_void;

        // 交换(不经 getter:直接用 svc 指针模拟 g2_listen_run 的中间段)。
        let o0: *mut c_void =
            unsafe { core::ptr::read_volatile((svc_ptr as usize + FIELD_OBSERVER) as *mut *mut c_void) };
        assert_eq!(o0, o0_obj);
        let ours = init_object();
        let slot = (svc_ptr as usize + FIELD_OBSERVER) as *mut AtomicPtr<c_void>;
        let prev = unsafe { (*slot).swap(ours, Ordering::AcqRel) };
        assert_eq!(prev, o0_obj, "交换返回原观察者");
        // 生产流程同款:swap 后登记 O0(处理器经它转发)。
        O0.store(o0_obj, Ordering::Release);

        // 模拟 OnRecv:调用 *(svc+0x140) 的 slot1,传 msg pair。
        let cur = unsafe { core::ptr::read_volatile(slot as *mut *mut c_void) };
        let vptr = unsafe { core::ptr::read_volatile(cur as *const usize) };
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void) =
            unsafe { core::mem::transmute(*(vptr as *const usize).byte_add(8)) };
        let msg_obj = [0xDEADBEEFu64, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8];
        let pair = [msg_obj.as_ptr() as u64, 0u64];
        // SAFETY: 测试构造。
        unsafe { f(cur, &pair as *const _ as *mut c_void) };

        // 转发达 O0。
        FORWARDED.with(|f| assert_eq!(*f.borrow(), vec![msg_obj.as_ptr() as u64]));
        // 环捕获了头部。
        let r = ring();
        assert!(r.pushed.load(Ordering::Relaxed) >= 4, "头 0x40 字节已入环");

        // 恢复。
        let cur = unsafe { core::ptr::read_volatile(slot as *mut *mut c_void) };
        if cur == ours {
            unsafe { core::ptr::write_volatile(slot as *mut *mut c_void, o0_obj) };
        }
        assert_eq!(
            unsafe { core::ptr::read_volatile(slot as *mut *mut c_void) },
            o0_obj,
            "恢复后 O0 回位"
        );
        O0.store(core::ptr::null_mut(), Ordering::Release);
    }
}
