//! K3-E 通用 JS 执行器(生产 primitive):在 QQ 主 env 执行调用方提供的 JS,
//! 结果字符串写 JSONL 报告。门控与 inject 伞相同(本模块无独立门)。
//!
//! 机制与 asyncrun 相同(uv_async + Script::Compile/Run),但 JS 由调用方提供
//! ——桥保持哑,业务逻辑在 core 侧 JS 资产。
//!
//! # Safety
//!
//! ctx 必须指向本进程内由加载器写入的有效 [`ExecCtx`];js 指针须指向
//! js_len 字节的 UTF-8 缓冲(加载器已写入本进程内存)。

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::envrun::append_stage;
use crate::asyncrun::call_sret1;

/// 结果码(caligo_exec_run 的返回值)。
pub mod exec_code {
    pub const OK: u32 = 0;
    pub const ERR_NULL: u32 = 1;
    pub const ERR_NO_QQNT: u32 = 2;
    pub const ERR_NO_CONTEXT: u32 = 3;
}

/// #[repr(C)] 执行上下文(加载器写入)。
#[repr(C)]
pub struct ExecCtx {
    /// 候选 node::Environment*(0 = 干跑:只验证解析链)。
    pub env: usize,
    /// QQNT.dll 基址(加载器侧解析;env 新鲜度校验用)。
    pub qqnt_base: usize,
    /// UTF-8 JS 源码指针(加载器已写入本进程内存)。
    pub js: usize,
    /// JS 字节长度。
    pub js_len: usize,
    /// NUL 结尾 UTF-16 报告路径缓冲地址。
    pub report_path: usize,
    /// 执行轮询上限毫秒(JS 同步返回即完成)。
    pub wait_ms: u32,
    pub _pad: u32,
}

// 执行结果传递(回调写、远程线程读)。
static RESULT_LEN: AtomicU32 = AtomicU32::new(0);
static RESULT_SEQ: AtomicU32 = AtomicU32::new(0); // 0=未 2=有结果 6..9=错误码
static JS_PTR: AtomicUsize = AtomicUsize::new(0);
static JS_LEN: AtomicUsize = AtomicUsize::new(0);
static EXEC_ISOLATE: AtomicUsize = AtomicUsize::new(0);
static EXEC_CTXV: AtomicUsize = AtomicUsize::new(0);
static EXEC_ENV_ISOLATE: AtomicUsize = AtomicUsize::new(0);
static EXPORTS: AtomicUsize = AtomicUsize::new(0);

const RESULT_CAP: usize = 64 * 1024;
static mut RESULT_BUF: [u8; RESULT_CAP] = [0; RESULT_CAP];

const NAME_GET_CURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_ENTERED: &str =
    "?GetEnteredOrMicrotaskContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_INCUMBENT: &str =
    "?GetIncumbentContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_HS_CTOR: &str = "??0HandleScope@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_HS_DTOR: &str = "??1HandleScope@v8@@QEAA@XZ";
const NAME_NEWUTF8: &str =
    "?NewFromUtf8@String@v8@@SA?AV?$MaybeLocal@VString@v8@@@2@PEAVIsolate@2@PEBDW4NewStringType@2@H@Z";
const NAME_COMPILE: &str = "?Compile@Script@v8@@SA?AV?$MaybeLocal@VScript@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VString@v8@@@2@PEAVScriptOrigin@2@@Z";
const NAME_RUN: &str = "?Run@Script@v8@@QEAA?AV?$MaybeLocal@VValue@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VData@v8@@@2@@Z";
const NAME_UTF8_DTOR: &str = "??1Utf8Value@String@v8@@QEAA@XZ";
const NAME_UTF8_CTOR: &str =
    "??0Utf8Value@String@v8@@QEAA@PEAVIsolate@2@V?$Local@VValue@v8@@@2@@Z";
const NAME_UTF8_DEREF: &str = "??DUtf8Value@String@v8@@QEAAPEADXZ";
const NAME_UV_HANDLE_SIZE: &str = "uv_handle_size";
const NAME_UV_ASYNC_INIT: &str = "uv_async_init";
const NAME_UV_ASYNC_SEND: &str = "uv_async_send";

type FnIsolateGetCurrent = unsafe extern "C" fn() -> *mut c_void;
/// v8 成员函数返回 Local:this=RCX,sret=RDX(asyncrun 同款)。
type FnIsolateCtx = unsafe extern "C" fn(isolate: *mut c_void, sret: *mut usize);
type FnHandleScopeCtor =
    unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void) -> *mut c_void;
type FnScopeDtor = unsafe extern "C" fn(this: *mut c_void);
type FnStringNewFromUtf8 = unsafe extern "C" fn(
    sret: *mut usize,
    isolate: *mut c_void,
    data: *const u8,
    ty: i32,
    len: i32,
);
type FnScriptCompile =
    unsafe extern "C" fn(sret: *mut usize, ctx: usize, src: usize, origin: *mut c_void);
type FnScriptRun = unsafe extern "C" fn(this: usize, sret: *mut usize, ctx: usize, data: usize);
type FnUtf8Ctor =
    unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void, value: usize) -> *mut c_void;
type FnUtf8Deref = unsafe extern "C" fn(this: *mut c_void) -> *const u8;
type FnUvHandleSize = unsafe extern "C" fn(t: i32) -> usize;
type FnUvAsyncInit = unsafe extern "C" fn(
    loop_: *mut c_void,
    async_: *mut c_void,
    cb: unsafe extern "C" fn(*mut c_void),
) -> i32;
type FnUvAsyncSend = unsafe extern "C" fn(async_: *mut c_void) -> i32;

#[allow(dead_code)]
struct ExecExports {
    isolate_get_current: FnIsolateGetCurrent,
    entered_ctx: FnIsolateCtx,
    incumbent_ctx: FnIsolateCtx,
    hs_ctor: FnHandleScopeCtor,
    hs_dtor: FnScopeDtor,
    new_from_utf8: FnStringNewFromUtf8,
    compile: FnScriptCompile,
    run: FnScriptRun,
    utf8_ctor: FnUtf8Ctor,
    utf8_dtor: FnScopeDtor,
    utf8_deref: FnUtf8Deref,
    uv_handle_size: FnUvHandleSize,
    uv_async_init: FnUvAsyncInit,
    uv_async_send: FnUvAsyncSend,
}

/// 地址 → 函数指针(签名按导出面 mangled 名保证)。
fn to_fn<T: Copy>(a: usize) -> T {
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
    // SAFETY: usize 位面拷贝(transmute 泛型无法静态证尺寸)。
    unsafe { std::ptr::read(core::ptr::addr_of!(a).cast::<T>()) }
}

fn local_empty(v: usize) -> bool {
    v == 0
}

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

fn page_span_ok(addr: usize, len: usize) -> bool {
    let start = addr & !0xFFF;
    let end = (addr + len - 1) & !0xFFF;
    let mut p = start;
    while p <= end {
        if !page_readable(p) {
            return false;
        }
        p += 0x1000;
    }
    true
}

unsafe fn read_usize(p: usize) -> Option<usize> {
    if !page_span_ok(p, 8) {
        return None;
    }
    // SAFETY: 页已校验;窗口期风险由报告呈现。
    unsafe { Some((p as *const usize).read_unaligned()) }
}

unsafe fn resolve_exports(qqnt: usize, report: &str) -> Option<ExecExports> {
    macro_rules! get {
        ($name:expr) => {
            match crate::obs::export_addr(qqnt, $name) {
                Some(a) => a,
                None => {
                    append_stage(report, "resolve", false, &format!("missing {}", $name));
                    return None;
                }
            }
        };
    }
    Some(ExecExports {
        isolate_get_current: to_fn(get!(NAME_GET_CURRENT)),
        entered_ctx: to_fn(get!(NAME_ENTERED)),
        incumbent_ctx: to_fn(get!(NAME_INCUMBENT)),
        hs_ctor: to_fn(get!(NAME_HS_CTOR)),
        hs_dtor: to_fn(get!(NAME_HS_DTOR)),
        new_from_utf8: to_fn(get!(NAME_NEWUTF8)),
        compile: to_fn(get!(NAME_COMPILE)),
        run: to_fn(get!(NAME_RUN)),
        utf8_ctor: to_fn(get!(NAME_UTF8_CTOR)),
        utf8_dtor: to_fn(get!(NAME_UTF8_DTOR)),
        utf8_deref: to_fn(get!(NAME_UTF8_DEREF)),
        uv_handle_size: to_fn(get!(NAME_UV_HANDLE_SIZE)),
        uv_async_init: to_fn(get!(NAME_UV_ASYNC_INIT)),
        uv_async_send: to_fn(get!(NAME_UV_ASYNC_SEND)),
    })
}

/// uv_async 回调:主 env 轮转点上执行上下文阶梯 + JS_PTR/JS_LEN 源码。
/// 阶梯必须在主循环线程(此处)执行——远程线程直调 v8 方法会死锁(49688 事故)。
unsafe extern "C" fn exec_cb(_handle: *mut c_void) {
    // SAFETY: 全路径页校验 + 错误码化,无异常外泄。
    unsafe {
        let js_ptr = JS_PTR.load(Ordering::Acquire);
        let js_len = JS_LEN.load(Ordering::Acquire);
        let ex_ptr = EXPORTS.load(Ordering::Acquire);
        let env_isolate = EXEC_ENV_ISOLATE.load(Ordering::Acquire);
        if js_ptr == 0 || js_len == 0 || !page_span_ok(js_ptr, js_len) || ex_ptr == 0 || env_isolate == 0 {
            RESULT_SEQ.store(9, Ordering::Release);
            return;
        }
        let ex: &ExecExports = &*(ex_ptr as *const ExecExports);
        // 上下文阶梯(本线程 = 主循环线程,与 async_cb 同位;asyncrun 已证安全)。
        // SAFETY: GetCurrent 无参静态;entered/incumbent 为纯读访问器。
        let current = (ex.isolate_get_current)() as usize;
        let isolate = if current != 0 { current } else { env_isolate };
        let entered = call_sret1(ex.entered_ctx, isolate as *mut c_void);
        let mut ctxv = 0usize;
        if !local_empty(entered) {
            ctxv = entered;
        } else {
            let incumbent = call_sret1(ex.incumbent_ctx, isolate as *mut c_void);
            if !local_empty(incumbent) {
                ctxv = incumbent;
            }
        }
        if ctxv == 0 {
            RESULT_SEQ.store(5, Ordering::Release);
            return;
        }
        EXEC_ISOLATE.store(isolate, Ordering::Release);
        EXEC_CTXV.store(ctxv, Ordering::Release);
        let mut scope = [0usize; 8];
        (ex.hs_ctor)(scope.as_mut_ptr().cast(), isolate as *mut c_void);
        let mut src = 0usize;
        (ex.new_from_utf8)(
            core::ptr::addr_of_mut!(src),
            isolate as *mut c_void,
            js_ptr as *const u8,
            0,
            js_len as i32,
        );
        if local_empty(src) {
            (ex.hs_dtor)(scope.as_mut_ptr().cast());
            RESULT_SEQ.store(8, Ordering::Release);
            return;
        }
        let mut origin: [u8; 0x28] = [0; 0x28];
        origin[0x14..0x18].copy_from_slice(&(-1i32).to_le_bytes());
        let mut script = 0usize;
        (ex.compile)(core::ptr::addr_of_mut!(script), ctxv, src, origin.as_mut_ptr().cast());
        if local_empty(script) {
            (ex.hs_dtor)(scope.as_mut_ptr().cast());
            RESULT_SEQ.store(7, Ordering::Release);
            return;
        }
        let mut result = 0usize;
        (ex.run)(script, core::ptr::addr_of_mut!(result), ctxv, 0);
        if local_empty(result) {
            (ex.hs_dtor)(scope.as_mut_ptr().cast());
            RESULT_SEQ.store(6, Ordering::Release);
            return;
        }
        let mut utf8 = [0usize; 3];
        (ex.utf8_ctor)(utf8.as_mut_ptr().cast(), isolate as *mut c_void, result);
        let text = (ex.utf8_deref)(utf8.as_mut_ptr().cast());
        if !text.is_null() {
            let mut n = 0usize;
            while n < RESULT_CAP - 1 && *text.add(n) != 0 {
                n += 1;
            }
            let dst = core::ptr::addr_of_mut!(RESULT_BUF).cast::<u8>();
            core::ptr::copy_nonoverlapping(text, dst, n);
            *dst.add(n) = 0;
            RESULT_LEN.store(n as u32, Ordering::Release);
        }
        (ex.utf8_dtor)(utf8.as_mut_ptr().cast());
        (ex.hs_dtor)(scope.as_mut_ptr().cast());
        RESULT_SEQ.store(2, Ordering::Release);
    }
}

fn read_wide(path_ptr: usize) -> Option<String> {
    if path_ptr == 0 {
        return None;
    }
    let p = path_ptr as *const u16;
    // SAFETY: loader 约定 NUL 结尾;上限 32 KiB。
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 {
            if *p.add(len) == 0 {
                break;
            }
            len += 1;
        }
        if len >= 32 * 1024 {
            return None;
        }
        String::from_utf16(std::slice::from_raw_parts(p, len)).ok()
    }
}

/// 远程线程主体:解析 → env 校验 → 上下文阶梯 → uv_async 执行 JS → 报告。
///
/// # Safety
///
/// ctx 由加载器写入本进程;js 缓冲须有效且长度正确。
pub unsafe fn exec_run(ctx: &ExecCtx) -> u32 {
    // SAFETY: 调用方保证 ctx/js 有效。
    unsafe {
        RESULT_LEN.store(0, Ordering::Release);
        RESULT_SEQ.store(0, Ordering::Release);
        JS_PTR.store(0, Ordering::Release);
        JS_LEN.store(0, Ordering::Release);
        EXEC_ISOLATE.store(0, Ordering::Release);
        EXEC_CTXV.store(0, Ordering::Release);
        EXPORTS.store(0, Ordering::Release);

        let path_ptr = ctx.report_path;
        if path_ptr == 0 || ctx.js == 0 || ctx.js_len == 0 {
            return exec_code::ERR_NULL;
        }
        let report = match read_wide(path_ptr) {
            Some(s) => s,
            None => return exec_code::ERR_NULL,
        };
        if !page_span_ok(ctx.js, ctx.js_len) {
            append_stage(&report, "js", false, "js buffer unreadable");
            return exec_code::ERR_NULL;
        }
        append_stage(&report, "start", true, &format!("js_len={} wait_ms={}", ctx.js_len, ctx.wait_ms));

        let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
        let h = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr());
        if h.is_null() {
            append_stage(&report, "qqnt", false, "not loaded");
            return exec_code::ERR_NO_QQNT;
        }
        let Some(ex) = resolve_exports(h as usize, &report) else {
            return exec_code::ERR_NO_QQNT;
        };
        let ex = Box::leak(Box::new(ex));
        EXPORTS.store(ex as *const ExecExports as usize, Ordering::Release);

        if ctx.env == 0 {
            append_stage(&report, "dry_run", true, "env=0; exports resolved");
            append_stage(&report, "done", true, "dry run complete");
            return exec_code::OK;
        }
        if !page_span_ok(ctx.env, 0xB60) {
            append_stage(&report, "validate_env", false, "env unreadable");
            return exec_code::ERR_NO_QQNT;
        }
        // 13056 事故修复:env 可能因 QQ 内部环境回收/登出而悬垂——页映射不代表
        // 有效。vfptr 必须与 qqnt_base+主 vtable RVA 严格一致,失配即拒绝。
        if !env_is_fresh_pub(ctx.qqnt_base, ctx.env) {
            append_stage(
                &report,
                "validate_env",
                false,
                "env STALE (vfptr mismatch) — rescan required (envscan)",
            );
            return exec_code::ERR_NO_QQNT;
        }
        let Some(_env_isolate_check) = read_usize(ctx.env + 0xA0) else {
            append_stage(&report, "validate_env", false, "env+0xA0 unreadable");
            return exec_code::ERR_NO_QQNT;
        };
        // 阶梯已在 exec_cb(主循环线程)执行——49688 死锁修复。

        let size = (ex.uv_handle_size)(1); // UV_ASYNC
        if size == 0 || size > 0x1000 {
            append_stage(&report, "alloc_async", false, &format!("size={size}"));
            return exec_code::ERR_NO_QQNT;
        }
        let Ok(layout) = std::alloc::Layout::from_size_align(size, 16) else {
            return exec_code::ERR_NO_QQNT;
        };
        let handle = std::alloc::alloc(layout);
        if handle.is_null() {
            return exec_code::ERR_NO_QQNT;
        }
        core::ptr::write_bytes(handle, 0, size);
        let Some(id) = read_usize(ctx.env + 0xB0) else {
            append_stage(&report, "loop", false, "env+0xB0 unreadable");
            return exec_code::ERR_NO_QQNT;
        };
        let Some(loop_) = read_usize(id + 0x11E8) else {
            append_stage(&report, "loop", false, "loop unreadable");
            return exec_code::ERR_NO_QQNT;
        };
        let init_r = (ex.uv_async_init)(loop_ as *mut c_void, handle as *mut c_void, exec_cb);
        append_stage(&report, "uv_async_init", init_r == 0, &format!("r={init_r} loop={loop_:#x}"));
        if init_r != 0 {
            return exec_code::ERR_NO_QQNT;
        }
        let started = std::time::Instant::now();
        let send_r = (ex.uv_async_send)(handle as *mut c_void);
        append_stage(&report, "uv_async_send", send_r == 0, &format!("r={send_r}"));

        let deadline = started + std::time::Duration::from_millis(ctx.wait_ms.max(1) as u64);
        while RESULT_SEQ.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let seq = RESULT_SEQ.load(Ordering::Acquire);
        let len = RESULT_LEN.load(Ordering::Acquire) as usize;
        let ok = seq == 2;
        // SAFETY: 回调已发布;长度受上限。
        let text = {
            core::slice::from_raw_parts(core::ptr::addr_of!(RESULT_BUF).cast::<u8>(), len)
        };
        append_stage(&report, "result", ok, &format!("seq={seq} {}", String::from_utf8_lossy(text)));
        append_stage(&report, "done", true, "exec complete");
        exec_code::OK
    }
}

/// 环境新鲜度校验(13056 事故修复):[env+0] 必须等于 qqnt_base+主 vtable RVA。
/// env 指针可能因 QQ 内部环境回收/账号登出而悬垂——页映射不代表有效。
pub const EXPECTED_VTABLE_RVA: u32 = 0x0A80_4990;

pub(crate) unsafe fn env_is_fresh_pub(qqnt_base: usize, env: usize) -> bool {
    // SAFETY: 双指针读均经页校验。
    unsafe {
        let Some(vfptr) = read_usize(env) else { return false; };
        let expected = (qqnt_base as u64).wrapping_add(EXPECTED_VTABLE_RVA as u64);
        vfptr as u64 == expected
    }
}
