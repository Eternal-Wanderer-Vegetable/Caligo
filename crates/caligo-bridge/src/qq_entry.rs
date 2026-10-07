//! K4-D7 候选 B 首入机器(计划 §6.1/§7-D7;bootstrap-contract §2)。
//!
//! **仅 research 构建存在**(`--features research`);普通构建整体不编译。
//! 版本绑定 9.9.33-52230(vtable RVA 见 [`EXPECTED_VTABLE_RVA`])。
//!
//! 序列(D2 契约 §2 候选 B):
//! 1. 解析符号(QQNT.dll 导出面;LAB 可注入假符号驱动同一机器);
//! 2. env 新鲜度门(`[env+0] == qqnt+主vtableRVA`,I-03 修复语义);
//! 3. uv_loop 链解析(env+0xB0 → IsolateData;+0x11E8 → loop,K2-03 布局);
//! 4. **首入**:`RequestInterrupt` 载荷 —— 回调在 QQ 的 JS/loop 线程执行;
//!    回调内先核 `Isolate::GetCurrent()`(**零 current 不放行**),通过后
//!    才在该线程 `uv_async_init` 常驻句柄并发布 ready(修复旧 asyncrun
//!    从远程线程 init 的 libuv 契约违例);
//! 5. 此后:工作线程仅 `uv_async_send` 唤醒(合法跨线程面);pump 回调在
//!    owner 轮转点执行有界 drain;关闭经 CLOSE_REQ → owner 线程
//!    `uv_close` → 关闭回调确认(不释放在用句柄)。
//!
//! 全程阶段化 JSONL 取证(每阶段可独立 PASS/FAIL,供 D7 报告逐行引用)。
//! 单进程单实例(计划 §6.1:每宿主代次初始化一次)。

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::exec::EXPECTED_VTABLE_RVA;

// —— 符号名(与 asyncrun 同源;QQNT.dll 导出面)——

const NAME_UV_ASYNC_INIT: &str = "uv_async_init";
const NAME_UV_ASYNC_SEND: &str = "uv_async_send";
const NAME_UV_HANDLE_SIZE: &str = "uv_handle_size";
const NAME_UV_CLOSE: &str = "uv_close";
const NAME_ISOLATE_GETCURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_REQUEST_INTERRUPT: &str =
    "?RequestInterrupt@node@@YAXPEAVEnvironment@1@P6AXPEAX@Z1@Z";

type FnUvAsyncInit = unsafe extern "C" fn(
    loop_: *mut core::ffi::c_void,
    async_: *mut core::ffi::c_void,
    cb: unsafe extern "C" fn(*mut core::ffi::c_void),
) -> i32;
type FnUvAsyncSend = unsafe extern "C" fn(async_: *mut core::ffi::c_void) -> i32;
type FnUvHandleSize = unsafe extern "C" fn(t: i32) -> usize;
type FnUvClose = unsafe extern "C" fn(
    handle: *mut core::ffi::c_void,
    cb: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
);
type FnIsolateGetCurrent = unsafe extern "C" fn() -> *mut core::ffi::c_void;
type FnRequestInterrupt = unsafe extern "C" fn(
    env: *mut core::ffi::c_void,
    cb: unsafe extern "system" fn(*mut core::ffi::c_void),
    ctx: *mut core::ffi::c_void,
);

/// 五个首入所需符号(地址形式;调用方按上述类型别名保证 ABI)。
#[derive(Debug, Clone, Copy)]
pub struct EntrySymbols {
    pub uv_async_init: usize,
    pub uv_async_send: usize,
    pub uv_handle_size: usize,
    pub uv_close: usize,
    pub isolate_get_current: usize,
    pub request_interrupt: usize,
}

impl EntrySymbols {
    /// 从 QQNT.dll 基址解析(真实路线;obs::export_addr 走导出表)。
    pub fn resolve(qqnt: usize, report: &str) -> Option<Self> {
        let get = |name: &str| -> Option<usize> {
            // SAFETY: obs::export_addr 只读导出表。
            let a = unsafe { crate::obs::export_addr(qqnt, name) };
            if a.is_none() {
                append_stage(report, "resolve", false, &format!("export missing: {name}"));
            }
            a
        };
        Some(Self {
            uv_async_init: get(NAME_UV_ASYNC_INIT)?,
            uv_async_send: get(NAME_UV_ASYNC_SEND)?,
            uv_handle_size: get(NAME_UV_HANDLE_SIZE)?,
            uv_close: get(NAME_UV_CLOSE)?,
            isolate_get_current: get(NAME_ISOLATE_GETCURRENT)?,
            request_interrupt: get(NAME_REQUEST_INTERRUPT)?,
        })
    }

    /// LAB 注入(假符号;机器逻辑与真实路线共用)。
    pub fn from_raw(
        uv_async_init: usize,
        uv_async_send: usize,
        uv_handle_size: usize,
        uv_close: usize,
        isolate_get_current: usize,
        request_interrupt: usize,
    ) -> Self {
        Self {
            uv_async_init,
            uv_async_send,
            uv_handle_size,
            uv_close,
            isolate_get_current,
            request_interrupt,
        }
    }

    fn to_fn<T: Copy>(a: usize) -> T {
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
        // SAFETY: 调用方保证地址与 T 的 ABI 匹配(真实:导出表;LAB:测试注入)。
        unsafe { std::ptr::read(core::ptr::addr_of!(a).cast::<T>()) }
    }
}

/// 首入配置。
#[derive(Debug, Clone)]
pub struct EntryConfig {
    pub qqnt_base: usize,
    pub env: usize,
    pub report_path: String,
}

/// 首入错误(与阶段日志对应)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryError {
    EnvNotFresh,
    LoopChainUnreadable,
    InterruptNoCurrentContext,
    UvInitFailed { r: i32 },
    HandleSizeInvalid { size: usize },
    Timeout,
    AlreadyBootstrapped,
}

// —— 共享状态(单实例;extern 回调经静态访问) ——

struct EntryShared {
    // 配置(bootstrap 成功前由发布方写入,ready 后只读)。
    qqnt_base: AtomicUsize,
    env: AtomicUsize,
    isolate: AtomicUsize,
    loop_ptr: AtomicUsize,
    // 符号地址。
    sym_uv_async_init: AtomicUsize,
    sym_uv_async_send: AtomicUsize,
    sym_uv_handle_size: AtomicUsize,
    sym_uv_close: AtomicUsize,
    sym_isolate_get_current: AtomicUsize,
    // 机器状态。
    tid: AtomicUsize,          // owner 线程(首入回调线程)
    handle: AtomicUsize,       // uv_async 句柄
    ready: AtomicUsize,        // 1 = 首入完成
    error_code: AtomicUsize,   // ERR_*;0 = 无
    error_aux: AtomicUsize,    // 附带数值(size/r)
    quarantine: AtomicUsize,   // pump 发现错误线程
    close_req: AtomicUsize,
    closed: AtomicUsize,
    drain_hook: AtomicUsize,   // fn() -> usize(owner 轮转点执行)
    pumps: AtomicU64,
    wake_sends: AtomicU64,
}

static ENTRY: EntryShared = EntryShared {
    qqnt_base: AtomicUsize::new(0),
    env: AtomicUsize::new(0),
    isolate: AtomicUsize::new(0),
    loop_ptr: AtomicUsize::new(0),
    sym_uv_async_init: AtomicUsize::new(0),
    sym_uv_async_send: AtomicUsize::new(0),
    sym_uv_handle_size: AtomicUsize::new(0),
    sym_uv_close: AtomicUsize::new(0),
    sym_isolate_get_current: AtomicUsize::new(0),
    tid: AtomicUsize::new(0),
    handle: AtomicUsize::new(0),
    ready: AtomicUsize::new(0),
    error_code: AtomicUsize::new(0),
    error_aux: AtomicUsize::new(0),
    quarantine: AtomicUsize::new(0),
    close_req: AtomicUsize::new(0),
    closed: AtomicUsize::new(0),
    drain_hook: AtomicUsize::new(0),
    pumps: AtomicU64::new(0),
    wake_sends: AtomicU64::new(0),
};

const ERR_NONE: usize = 0;
const ERR_ENV_NOT_FRESH: usize = 1;
const ERR_LOOP_CHAIN: usize = 2;
const ERR_NO_CURRENT: usize = 3;
const ERR_HANDLE_SIZE: usize = 4;
const ERR_UV_INIT: usize = 5;
const ERR_TIMEOUT: usize = 6;

fn set_error_code(code: usize, aux: usize) {
    ENTRY.error_code.store(code, Ordering::Release);
    ENTRY.error_aux.store(aux, Ordering::Release);
}

fn set_error(e: EntryError) {
    match e {
        EntryError::EnvNotFresh => set_error_code(ERR_ENV_NOT_FRESH, 0),
        EntryError::LoopChainUnreadable => set_error_code(ERR_LOOP_CHAIN, 0),
        EntryError::InterruptNoCurrentContext => set_error_code(ERR_NO_CURRENT, 0),
        EntryError::HandleSizeInvalid { size } => set_error_code(ERR_HANDLE_SIZE, size),
        EntryError::UvInitFailed { r } => set_error_code(ERR_UV_INIT, r as usize),
        EntryError::Timeout => set_error_code(ERR_TIMEOUT, 0),
        EntryError::AlreadyBootstrapped => set_error_code(ERR_TIMEOUT, 0),
    }
}

fn take_error() -> Option<EntryError> {
    let code = ENTRY.error_code.swap(ERR_NONE, Ordering::Acquire);
    let aux = ENTRY.error_aux.load(Ordering::Acquire);
    match code {
        ERR_NONE => None,
        ERR_ENV_NOT_FRESH => Some(EntryError::EnvNotFresh),
        ERR_LOOP_CHAIN => Some(EntryError::LoopChainUnreadable),
        ERR_NO_CURRENT => Some(EntryError::InterruptNoCurrentContext),
        ERR_HANDLE_SIZE => Some(EntryError::HandleSizeInvalid { size: aux }),
        ERR_UV_INIT => Some(EntryError::UvInitFailed { r: aux as i32 }),
        _ => Some(EntryError::Timeout),
    }
}

// —— 页校验读(与 asyncrun/intr 同语义)——

fn page_readable(addr: usize) -> bool {
    // SAFETY: VirtualQuery 无锁查询。
    unsafe {
        let mut mbi: windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION =
            std::mem::zeroed();
        let n = windows_sys::Win32::System::Memory::VirtualQuery(
            addr as *const core::ffi::c_void,
            &mut mbi,
            std::mem::size_of::<windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION>(),
        );
        if n == 0 {
            return false;
        }
        const MEM_COMMIT: u32 = 0x1000;
        const PAGE_NOACCESS: u32 = 0x01;
        const PAGE_GUARD: u32 = 0x100;
        mbi.State == MEM_COMMIT && (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)) == 0
    }
}

fn read_usize_checked(p: usize) -> Option<usize> {
    if !page_readable(p) || !page_readable(p + 7) {
        return None;
    }
    // SAFETY: 页已校验。
    unsafe { Some((p as *const usize).read_unaligned()) }
}

// —— 阶段日志 ——

pub fn append_stage(report: &str, stage: &str, ok: bool, detail: &str) {
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
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(report)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

// —— 首入回调与 pump(libuv 轮转点)——

/// 首入载荷:RequestInterrupt 的处理点(QQ 的 JS/loop 线程)。
/// 检查顺序固定:线程记录 → **current isolate 核对(零不放行)** →
/// uv_handle_size → uv_async_init。任一失败即置 error 并中止,**不触碰宿主**。
unsafe extern "system" fn first_entry_cb(_ctx: *mut core::ffi::c_void) {
    // SAFETY: 各符号地址由 bootstrap 校验后发布;调用约定见类型别名。
    unsafe {
        let tid = windows_sys::Win32::System::Threading::GetCurrentThreadId();
        ENTRY.tid.store(tid as usize, Ordering::Release);
        let get_current: FnIsolateGetCurrent =
            EntrySymbols::to_fn(ENTRY.sym_isolate_get_current.load(Ordering::Acquire));
        let current = (get_current)() as usize;
        let expected = ENTRY.isolate.load(Ordering::Acquire);
        if current == 0 || current != expected {
            set_error(EntryError::InterruptNoCurrentContext);
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        let handle_size: FnUvHandleSize =
            EntrySymbols::to_fn(ENTRY.sym_uv_handle_size.load(Ordering::Acquire));
        let size = (handle_size)(1 /* UV_ASYNC */);
        if size == 0 || size > 0x1000 {
            set_error(EntryError::HandleSizeInvalid { size });
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        let layout = std::alloc::Layout::from_size_align(size, 16).unwrap();
        let handle = std::alloc::alloc(layout);
        if handle.is_null() {
            set_error(EntryError::HandleSizeInvalid { size });
            return;
        }
        core::ptr::write_bytes(handle, 0, size);
        let init: FnUvAsyncInit = EntrySymbols::to_fn(ENTRY.sym_uv_async_init.load(Ordering::Acquire));
        let r = (init)(
            ENTRY.loop_ptr.load(Ordering::Acquire) as *mut core::ffi::c_void,
            handle as *mut core::ffi::c_void,
            pump_cb,
        );
        if r != 0 {
            std::alloc::dealloc(handle, layout);
            set_error(EntryError::UvInitFailed { r });
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        ENTRY.handle.store(handle as usize, Ordering::Release);
        ENTRY.ready.store(1, Ordering::Release);
    }
}

/// 常驻 pump:owner 轮转点执行。线程不符 → quarantine(保留证据,不继续)。
unsafe extern "C" fn pump_cb(_handle: *mut core::ffi::c_void) {
    // SAFETY: 静态原子量;drain_hook 由本 crate 注册。
    unsafe {
        let tid = windows_sys::Win32::System::Threading::GetCurrentThreadId() as usize;
        if tid != ENTRY.tid.load(Ordering::Acquire) {
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        ENTRY.pumps.fetch_add(1, Ordering::Relaxed);
        if ENTRY.close_req.load(Ordering::Acquire) == 1 {
            let close: FnUvClose = EntrySymbols::to_fn(ENTRY.sym_uv_close.load(Ordering::Acquire));
            (close)(
                ENTRY.handle.load(Ordering::Acquire) as *mut core::ffi::c_void,
                Some(close_cb),
            );
            return;
        }
        let hook = ENTRY.drain_hook.load(Ordering::Acquire);
        if hook != 0 {
            let f: fn() -> usize = std::mem::transmute::<usize, fn() -> usize>(hook);
            let _ = f();
        }
    }
}

unsafe extern "C" fn close_cb(_handle: *mut core::ffi::c_void) {
    ENTRY.closed.store(1, Ordering::Release);
}

/// 首入结果句柄。
#[derive(Debug)]
pub struct EntryHandle;

impl EntryHandle {
    /// 工作线程唤醒(合法跨线程面:uv_async_send)。
    pub fn wake(&self) -> bool {
        if ENTRY.ready.load(Ordering::Acquire) != 1 {
            return false;
        }
        ENTRY.wake_sends.fetch_add(1, Ordering::Relaxed);
        // SAFETY: 地址来自导出解析/注入;uv_async_send 是 libuv 唯一
        // 明确允许跨线程调用的接口。
        unsafe {
            let send: FnUvAsyncSend = EntrySymbols::to_fn(ENTRY.sym_uv_async_send.load(Ordering::Acquire));
            (send)(ENTRY.handle.load(Ordering::Acquire) as *mut core::ffi::c_void) == 0
        }
    }

    /// 注册 owner 轮转点 drain 钩子(`fn() -> usize`,返回处理数)。
    pub fn set_drain_hook(&self, f: fn() -> usize) {
        ENTRY.drain_hook.store(f as usize, Ordering::Release);
    }

    /// 关闭协议:置 CLOSE_REQ → 唤醒 → owner pump 执行 uv_close →
    /// 等关闭回调。超时不证明关闭完成:返回 Timeout,**不释放句柄内存**
    /// (计划 §5.3;留证据,随进程回收)。
    pub fn close(&self, wait_ms: u32) -> Result<(), EntryError> {
        if ENTRY.ready.load(Ordering::Acquire) != 1 {
            return Err(EntryError::AlreadyBootstrapped);
        }
        ENTRY.close_req.store(1, Ordering::Release);
        self.wake();
        let deadline = Instant::now() + Duration::from_millis(wait_ms as u64);
        while ENTRY.closed.load(Ordering::Acquire) != 1 {
            if Instant::now() >= deadline {
                return Err(EntryError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    pub fn owner_tid(&self) -> usize {
        ENTRY.tid.load(Ordering::Acquire)
    }

    pub fn pumps(&self) -> u64 {
        ENTRY.pumps.load(Ordering::Relaxed)
    }

    pub fn quarantine(&self) -> bool {
        ENTRY.quarantine.load(Ordering::Acquire) == 1
    }
}

/// 执行首入(计划 §6.1:每宿主代次一次;重复调用拒绝)。
pub fn bootstrap(cfg: &EntryConfig, symbols: &EntrySymbols) -> Result<EntryHandle, EntryError> {
    if ENTRY.ready.load(Ordering::Acquire) == 1 {
        return Err(EntryError::AlreadyBootstrapped);
    }
    let report = &cfg.report_path;
    // 阶段 1:env 新鲜度门。
    let Some(vfptr) = read_usize_checked(cfg.env) else {
        append_stage(report, "env_fresh", false, "env page unreadable");
        return Err(EntryError::EnvNotFresh);
    };
    let expected = (cfg.qqnt_base as u64).wrapping_add(EXPECTED_VTABLE_RVA as u64);
    if vfptr as u64 != expected {
        append_stage(
            report,
            "env_fresh",
            false,
            &format!("vfptr {:#x} != expected {:#x}", vfptr as u64, expected),
        );
        return Err(EntryError::EnvNotFresh);
    }
    append_stage(report, "env_fresh", true, "env vtable matches frozen RVA");
    // 阶段 2:uv_loop 链(env+0xB0 → IsolateData;+0x11E8 → loop)。
    let Some(isolate_data) = read_usize_checked(cfg.env + 0xB0) else {
        append_stage(report, "loop_chain", false, "env+0xB0 unreadable");
        return Err(EntryError::LoopChainUnreadable);
    };
    let Some(isolate) = read_usize_checked(cfg.env + 0xA0) else {
        append_stage(report, "loop_chain", false, "env+0xA0 unreadable");
        return Err(EntryError::LoopChainUnreadable);
    };
    let Some(loop_ptr) = read_usize_checked(isolate_data + 0x11E8) else {
        append_stage(report, "loop_chain", false, "IsolateData+0x11E8 unreadable");
        return Err(EntryError::LoopChainUnreadable);
    };
    if isolate == 0 || loop_ptr == 0 {
        append_stage(report, "loop_chain", false, "isolate/loop null");
        return Err(EntryError::LoopChainUnreadable);
    }
    append_stage(
        report,
        "loop_chain",
        true,
        &format!("isolate={isolate:#x} loop={loop_ptr:#x}"),
    );
    // 发布(ready 之前;回调以 Acquire 读取)。
    ENTRY.qqnt_base.store(cfg.qqnt_base, Ordering::Release);
    ENTRY.env.store(cfg.env, Ordering::Release);
    ENTRY.isolate.store(isolate, Ordering::Release);
    ENTRY.loop_ptr.store(loop_ptr, Ordering::Release);
    ENTRY
        .sym_uv_async_init
        .store(symbols.uv_async_init, Ordering::Release);
    ENTRY
        .sym_uv_async_send
        .store(symbols.uv_async_send, Ordering::Release);
    ENTRY
        .sym_uv_handle_size
        .store(symbols.uv_handle_size, Ordering::Release);
    ENTRY.sym_uv_close.store(symbols.uv_close, Ordering::Release);
    ENTRY
        .sym_isolate_get_current
        .store(symbols.isolate_get_current, Ordering::Release);
    // 阶段 3:首入(RequestInterrupt;回调同步或异步执行均按状态机等待)。
    append_stage(report, "interrupt_posted", true, "posting first-entry payload");
    // SAFETY: env 经新鲜度门;符号 ABI 见类型别名(K2/K3 反汇编实证)。
    unsafe {
        let ri: FnRequestInterrupt = EntrySymbols::to_fn(symbols.request_interrupt);
        ri(cfg.env as *mut core::ffi::c_void, first_entry_cb, std::ptr::null_mut());
    }
    // 等待首入完成(空闲 QQ 的 JS 间隙可拖延数分钟 —— K2-03 §12.3 记录;
    // 调用方按 D7 观察窗给足超时,超时不是失败证据,记录后由执行者决策)。
    let deadline = Instant::now() + Duration::from_secs(600);
    while ENTRY.ready.load(Ordering::Acquire) != 1 {
        if let Some(e) = take_error() {
            append_stage(report, "first_entry", false, &format!("{e:?}"));
            return Err(e);
        }
        if Instant::now() >= deadline {
            append_stage(report, "first_entry", false, "timeout(600s;空闲 QQ 首入延迟候选)");
            return Err(EntryError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    append_stage(
        report,
        "first_entry",
        true,
        &format!("owner_tid={}", ENTRY.tid.load(Ordering::Acquire)),
    );
    Ok(EntryHandle)
}

/// LAB 专用:重置首入静态状态(仅 research 构建导出;生产链路永不调用 ——
/// 真机上单宿主代次单实例,重置意味着允许二次首入,违反 §6.1)。
#[doc(hidden)]
pub fn test_reset_state() {
    ENTRY.qqnt_base.store(0, Ordering::Release);
    ENTRY.env.store(0, Ordering::Release);
    ENTRY.isolate.store(0, Ordering::Release);
    ENTRY.loop_ptr.store(0, Ordering::Release);
    ENTRY.sym_uv_async_init.store(0, Ordering::Release);
    ENTRY.sym_uv_async_send.store(0, Ordering::Release);
    ENTRY.sym_uv_handle_size.store(0, Ordering::Release);
    ENTRY.sym_uv_close.store(0, Ordering::Release);
    ENTRY.sym_isolate_get_current.store(0, Ordering::Release);
    ENTRY.tid.store(0, Ordering::Release);
    ENTRY.handle.store(0, Ordering::Release);
    ENTRY.ready.store(0, Ordering::Release);
    ENTRY.error_code.store(0, Ordering::Release);
    ENTRY.error_aux.store(0, Ordering::Release);
    ENTRY.quarantine.store(0, Ordering::Release);
    ENTRY.close_req.store(0, Ordering::Release);
    ENTRY.closed.store(0, Ordering::Release);
    ENTRY.drain_hook.store(0, Ordering::Release);
    ENTRY.pumps.store(0, Ordering::Relaxed);
    ENTRY.wake_sends.store(0, Ordering::Relaxed);
}

// —— HostAdapter 适配(QqOwnerAdapter;resident 的生产缝) ——

/// 候选 B 宿主适配器:D7 面向 Health/身份/停止;消息监听(D8)与
/// 发送(D9)显式拒绝,不用占位成果冒充能力。
pub struct QqOwnerAdapter {
    handle: EntryHandle,
}

impl QqOwnerAdapter {
    /// 首入成功后构造。`f` 由 bootstrap 侧提供。
    pub fn new(handle: EntryHandle) -> Self {
        Self { handle }
    }

    pub fn entry(&self) -> &EntryHandle {
        &self.handle
    }
}

impl crate::host_adapter::HostAdapter for QqOwnerAdapter {
    fn owner_thread_id(&self) -> u64 {
        self.handle.owner_tid() as u64
    }
    /// 当前执行线程(owner 语义由调用方线程保证;真实线程标识逐次读取)。
    fn current_thread_id(&self) -> u64 {
        // SAFETY: 无副作用。
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
    }
    fn env_valid(&self) -> bool {
        // 每次进入宿主 API 前重读新鲜度门(不缓存 —— K3 教训)。
        match read_usize_checked(self.handle_env()) {
            Some(v) => v as u64
                == (ENTRY.qqnt_base.load(Ordering::Acquire) as u64)
                    .wrapping_add(EXPECTED_VTABLE_RVA as u64),
            None => false,
        }
    }
    fn current_context_ok(&self) -> bool {
        // SAFETY: 符号地址经 bootstrap 校验。
        unsafe {
            let get_current: FnIsolateGetCurrent =
                EntrySymbols::to_fn(ENTRY.sym_isolate_get_current.load(Ordering::Acquire));
            let current = (get_current)() as usize;
            current != 0 && current == ENTRY.isolate.load(Ordering::Acquire)
        }
    }
    fn native_op(&mut self, op: crate::host_adapter::HostOp) -> Result<crate::host_adapter::HostOpResult, crate::host_adapter::HostError> {
        use crate::host_adapter::{HostError, HostOp, HostOpResult};
        match op {
            HostOp::Probe => {
                if !self.env_valid() {
                    return Err(HostError::EnvInvalid);
                }
                Ok(HostOpResult::ProbeDone)
            }
            // D7 无消息监听(D8 接线):显式延迟,不冒充已注册。
            HostOp::ListenerAdd => Ok(HostOpResult::ListenerDeferred),
            // D9 未接线:显式拒绝。
            HostOp::ListenerRemove { .. } => Err(HostError::NoProvenRoute),
            HostOp::SendText { .. } => Err(HostError::NoProvenRoute),
        }
    }
}

impl QqOwnerAdapter {
    fn handle_env(&self) -> usize {
        ENTRY.env.load(Ordering::Acquire)
    }
}
