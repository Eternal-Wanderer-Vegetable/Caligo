//! K2-03:主 Environment 事件循环点载荷(设计见 local-evidence/k2-03/design.md,
//! 文档 §12;路线由 RequestInterrupt 载荷修订为 uv_async 载荷)。
//!
//! 链条(全部离线解码 + 页校验):env+0xB0 → IsolateData → +0x11E8 → uv_loop_t*。
//! - mode 0:干跑(解析/校验/uv_loop_alive,不 init);
//! - mode 1:uv_async_init + uv_async_send,回调只做原子写(轮转点证明);
//! - mode 2:uv_async 回调内上下文阶梯(isolate 一致性 → entered → incumbent)。
//!   实测(mode 2 第一轮):uv 轮转点上两个访问器皆空(JS 空闲时上下文未 entered),
//!   安全门按设计拒绝执行——mode 2 保留为证据轮。
//! - mode 3(当前主路线):**中断点捕获 + 循环点执行**。RequestInterrupt 只在
//!   v8 执行 JS 的间隙被处理(此时上下文必然 entered),其回调仅读取 entered
//!   Local 槽内的 tagged Context 指针(纯原子写);随后 uv_async 回调在
//!   HandleScope 内经 `HandleScope::CreateHandle`(ord 1066,已导出)把 tagged
//!   指针转为合法 Local<Context>,在其上 Script::Compile/Run 只读枚举脚本。
//!
//! 纪律(计划 §3.1):回调内零 IO、零堆分配、零锁——静态原子 + 预分配缓冲。
//! uv_async 句柄不 close(需与 loop 同步),随进程退出回收,如实记录。

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::envrun::append_stage;

/// 执行结果码(caligo_async_run 的返回值)。
pub mod async_code {
    /// 流程完成(各阶段成败以报告为准)。
    pub const OK: u32 = 0;
    /// ctx 或报告路径指针无效。
    pub const ERR_NULL_PATH: u32 = 1;
    /// 报告路径不是合法 UTF-16。
    pub const ERR_BAD_PATH: u32 = 2;
    /// env 候选页不可读 / 布局链断裂(拒绝继续)。
    pub const ERR_ENV_INVALID: u32 = 3;
    /// mode 非法(0/1/2/3 之外)。
    pub const ERR_BAD_MODE: u32 = 4;
    /// uv 阶段失败(alive=false / init 失败)。
    pub const ERR_UV: u32 = 5;
    /// mode 3 phase A:中断点未捕获到 entered 上下文(JS 间隙未出现/上下文空)。
    pub const ERR_NO_CAPTURE: u32 = 6;
}

/// mode 2 脚本选择(ctx.script):0 = 只读指纹(v2),1 = load 探针(v3,K2-04)。
pub const SCRIPT_FINGERPRINT: u32 = 0;
pub const SCRIPT_LOAD_PROBE: u32 = 1;
pub const SCRIPT_LOAD_HARVEST: u32 = 2;

/// mode 2 的枚举脚本:只读(不调用任何 QQ 函数)、自包含、异常全捕获。
/// 第二版:除顶层键外,另取 load 的类型/源码指纹、process.versions、
/// require 可用性与 globalThis 键表(截断),全部为纯读取。
pub const ENUM_SCRIPT: &str = "(function(){try{var m=process._linkedBinding('major');var o={ok:true,keys:Object.getOwnPropertyNames(m),loadType:typeof m.load};try{o.loadSrc=String(m.load).slice(0,300)}catch(e){o.loadSrcErr=String(e)}try{o.versions=process.versions}catch(e){}try{o.hasRequire=typeof require;o.hasProcess=typeof process}catch(e){}try{o.globalKeys=Object.getOwnPropertyNames(globalThis).slice(0,120)}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-04 load 探针(K2-04 设计记录):纯读描述符/name/length → **无参调用一次
/// load()** → 记录返回值形态或异常消息。不传参、不链式调用返回值成员、不赋值。
pub const LOAD_PROBE_SCRIPT: &str = "(function(){try{var m=process._linkedBinding('major');var o={ok:true};var d=Object.getOwnPropertyDescriptor(m,'load');o.desc=d&&{writable:d.writable,enumerable:d.enumerable,configurable:d.configurable,hasGet:!!d.get,hasSet:!!d.set};o.fnName=m.load.name;o.fnArity=m.load.length;try{var r=m.load();o.retType=typeof r;if(r&&(typeof r==='object'||typeof r==='function')){o.retKeys=Object.getOwnPropertyNames(r).slice(0,200);try{o.retCtor=r.constructor&&r.constructor.name}catch(e){}}else{o.retPrim=String(r).slice(0,100)}}catch(e){o.callErr=String(e)}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-04 v4 探针:错误收割(load 各位参喂错误类型,校验错误暴露签名)+
/// 全量 globalThis 键表。不触发任何模块执行。
pub const LOAD_HARVEST_SCRIPT: &str = "(function(){try{var m=process._linkedBinding('major');var o={ok:true};var probes=[['load(1)',function(){return m.load(1)}],['load(x)',function(){return m.load('x')}],['load(x,y)',function(){return m.load('x','y')}],['load(x,y,z)',function(){return m.load('x','y','z')}]];o.harvest=[];for(var i=0;i<probes.length;i++){var e={name:probes[i][0]};try{var r=probes[i][1]();e.retType=typeof r;if(r&&typeof r==='object'){e.retKeys=Object.getOwnPropertyNames(r).slice(0,80)}}catch(err){e.err=String(err).slice(0,300)}o.harvest.push(e)}try{var g=Object.getOwnPropertyNames(globalThis);o.globalCount=g.length;o.globalKeys=g}catch(e){o.globalErr=String(e)}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// 远程调用上下文(加载器写入,#[repr(C)]).
#[repr(C)]
pub struct AsyncCtx {
    /// 候选 node::Environment*(0 = 干跑:只验证解析与布局链,不 init async)。
    pub env: usize,
    /// 0=干跑 1=原子载荷 2=JS 枚举 3=中断捕获+执行(备用)。
    pub mode: u32,
    /// mode 2 的脚本选择:0=指纹 1=load 探针。
    pub script: u32,
    /// NUL 结尾 UTF-16 报告路径缓冲地址。
    pub report_path: usize,
    /// 等待回调触发的毫秒数。
    pub wait_ms: u32,
    pub _pad: u32,
}

// --- 回调侧状态(仅原子;缓冲在回调前就绪) ---
static FIRED: AtomicU32 = AtomicU32::new(0);
static CB_TID: AtomicU32 = AtomicU32::new(0);
static CB_TICK: AtomicU64 = AtomicU64::new(0);
static CB_ISOLATE_MATCH: AtomicU32 = AtomicU32::new(0);
static CB_HAS_CTX: AtomicU32 = AtomicU32::new(0); // 0=无 1=entered 2=incumbent
static CB_JS_ERR: AtomicU32 = AtomicU32::new(0); // 0=ok 1=newstring/compile空 2=run空 3=链断裂 4=导出缺失
static RESULT_LEN: AtomicU32 = AtomicU32::new(0);
static RESULT_SEQ: AtomicU32 = AtomicU32::new(0); // 0=未触发 1=已触发无结果 2=有结果
static CB_MODE: AtomicU32 = AtomicU32::new(0);
static CB_SCRIPT: AtomicU32 = AtomicU32::new(0);
static CB_ENV: AtomicUsize = AtomicUsize::new(0);
static CB_ISOLATE: AtomicUsize = AtomicUsize::new(0);
/// mode 3:中断点捕获的 tagged Context 指针(0 = 未捕获)。
static CB_CTX_TAG: AtomicU64 = AtomicU64::new(0);
/// mode 3 phase A 完成标记(1=捕获完毕 2=回调已跑但未捕获到上下文)。
static CAP_SEQ: AtomicU32 = AtomicU32::new(0);

/// mode 2 结果缓冲(回调写、远程线程读;回调发布 seq=2 后读方可见)。
const RESULT_CAP: usize = 4096;
static mut RESULT_BUF: [u8; RESULT_CAP] = [0; RESULT_CAP];

// --- QQNT 导出(mangled 名;经 obs::export_addr 裸读解析) ---

const NAME_UV_ASYNC_INIT: &str = "uv_async_init";
const NAME_UV_ASYNC_SEND: &str = "uv_async_send";
const NAME_UV_HANDLE_SIZE: &str = "uv_handle_size";
const NAME_UV_LOOP_ALIVE: &str = "uv_loop_alive";
const NAME_ISOLATE_GETCURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_ISOLATE_ENTERED_CTX: &str =
    "?GetEnteredOrMicrotaskContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_ISOLATE_INCUMBENT_CTX: &str =
    "?GetIncumbentContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_HANDLE_SCOPE_CREATE: &str =
    "?CreateHandle@HandleScope@v8@@KAPEA_KPEAVIsolate@2@_K@Z";
const NAME_REQUEST_INTERRUPT: &str =
    "?RequestInterrupt@node@@YAXPEAVEnvironment@1@P6AXPEAX@Z1@Z";
const NAME_HANDLE_SCOPE_CTOR: &str = "??0HandleScope@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_HANDLE_SCOPE_DTOR: &str = "??1HandleScope@v8@@QEAA@XZ";
const NAME_STRING_NEW_FROM_UTF8: &str =
    "?NewFromUtf8@String@v8@@SA?AV?$MaybeLocal@VString@v8@@@2@PEAVIsolate@2@PEBDW4NewStringType@2@H@Z";
const NAME_SCRIPT_COMPILE: &str = "?Compile@Script@v8@@SA?AV?$MaybeLocal@VScript@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VString@v8@@@2@PEAVScriptOrigin@2@@Z";
// Run 的单参导出是裸跳板(直跳双参本体且不准备 R9 → 残留 R9 会被当作
// Local<Data>);直接调用双参重载,data 传空 Local(0)。
const NAME_SCRIPT_RUN: &str =
    "?Run@Script@v8@@QEAA?AV?$MaybeLocal@VValue@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VData@v8@@@2@@Z";
const NAME_UTF8_CTOR: &str =
    "??0Utf8Value@String@v8@@QEAA@PEAVIsolate@2@V?$Local@VValue@v8@@@2@@Z";
const NAME_UTF8_DTOR: &str = "??1Utf8Value@String@v8@@QEAA@XZ";
const NAME_UTF8_DEREF: &str = "??DUtf8Value@String@v8@@QEAAPEADXZ";

type FnUvAsyncInit = unsafe extern "C" fn(
    loop_: *mut c_void,
    async_: *mut c_void,
    cb: unsafe extern "C" fn(*mut c_void),
) -> i32;
type FnUvAsyncSend = unsafe extern "C" fn(async_: *mut c_void) -> i32;
type FnUvHandleSize = unsafe extern "C" fn(t: i32) -> usize;
type FnUvLoopAlive = unsafe extern "C" fn(loop_: *mut c_void) -> i32;
type FnIsolateGetCurrent = unsafe extern "C" fn() -> *mut c_void;
/// v8::Local/MaybeLocal 非平凡返回的 MSVC/clang-cl ABI(全部门实测反汇编证据):
/// - **静态函数**:sret 在 RCX,参数自 RDX 起后移(NewFromUtf8 全参验证);
/// - **成员函数**:this 在 RCX,sret 在 RDX,其余参数自 R8 起
///   (GetEnteredOrMicrotaskContext 读 [rcx+0x100D0] + 写 [rdx];
///    Run 尾部 mov [rsi],rax,rsi=arg2)。第一版实弹两起事故均源于此序。
type FnIsolateCtx = unsafe extern "C" fn(isolate: *mut c_void, sret: *mut usize);
type FnHandleScopeCtor =
    unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void) -> *mut c_void;
type FnScopeDtor = unsafe extern "C" fn(this: *mut c_void);
type FnHandleScopeCreate =
    unsafe extern "C" fn(isolate: *mut c_void, tagged: u64) -> *mut u64;
type FnRequestInterrupt =
    unsafe extern "C" fn(env: *mut c_void, cb: unsafe extern "system" fn(*mut c_void), ctx: *mut c_void);
type FnStringNewFromUtf8 = unsafe extern "C" fn(
    sret: *mut usize,
    isolate: *mut c_void,
    data: *const u8,
    ty: i32,
    len: i32,
);
type FnScriptCompile =
    unsafe extern "C" fn(sret: *mut usize, ctx: usize, src: usize, origin: *mut c_void);
type FnScriptRun =
    unsafe extern "C" fn(this: usize, sret: *mut usize, ctx: usize, data: usize);
type FnUtf8Ctor =
    unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void, value: usize) -> *mut c_void;
type FnUtf8Deref = unsafe extern "C" fn(this: *mut c_void) -> *const u8;

/// Local/MaybeLocal 空判定:8 字节包装,空即值为 0。
fn local_empty(v: usize) -> bool {
    v == 0
}

/// 调用成员函数风格的 Local 返回导出(this 在 RCX、sret 在 RDX),取回句柄值;
/// 0 = 空。
unsafe fn call_sret1(
    f: unsafe extern "C" fn(*mut c_void, *mut usize),
    this: *mut c_void,
) -> usize {
    let mut ret = 0usize;
    // SAFETY: sret 槽在调用方栈上;f 来自裸读导出表。
    unsafe { f(this, core::ptr::addr_of_mut!(ret)) };
    ret
}

#[allow(dead_code)]
struct Exports {
    uv_async_init: FnUvAsyncInit,
    uv_async_send: FnUvAsyncSend,
    uv_handle_size: FnUvHandleSize,
    uv_loop_alive: FnUvLoopAlive,
    isolate_get_current: FnIsolateGetCurrent,
    isolate_entered_ctx: FnIsolateCtx,
    isolate_incumbent_ctx: FnIsolateCtx,
    handle_scope_create: FnHandleScopeCreate,
    handle_scope_ctor: FnHandleScopeCtor,
    handle_scope_dtor: FnScopeDtor,
    string_new_from_utf8: FnStringNewFromUtf8,
    script_compile: FnScriptCompile,
    script_run: FnScriptRun,
    utf8_ctor: FnUtf8Ctor,
    utf8_dtor: FnScopeDtor,
    utf8_deref: FnUtf8Deref,
    request_interrupt: Option<FnRequestInterrupt>,
}

/// 地址 → 函数指针(签名由调用方按导出面 mangled 名保证)。
fn to_fn<T: Copy>(a: usize) -> T {
    // SAFETY: 地址来自裸读导出表;调用方以类型参数固定签名。
    // 经 usize 位面拷贝(泛型 T 上 transmute 无法静态证尺寸相等)。
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
    unsafe { std::ptr::read(core::ptr::addr_of!(a).cast::<T>()) }
}

unsafe fn resolve_exports(qqnt: usize, report: &str) -> Option<Exports> {
    macro_rules! get {
        ($name:expr) => {
            match crate::obs::export_addr(qqnt, $name) {
                Some(a) => a,
                None => {
                    append_stage(report, "resolve", false, &format!("export missing: {}", $name));
                    return None;
                }
            }
        };
    }
    Some(Exports {
        uv_async_init: to_fn(get!(NAME_UV_ASYNC_INIT)),
        uv_async_send: to_fn(get!(NAME_UV_ASYNC_SEND)),
        uv_handle_size: to_fn(get!(NAME_UV_HANDLE_SIZE)),
        uv_loop_alive: to_fn(get!(NAME_UV_LOOP_ALIVE)),
        isolate_get_current: to_fn(get!(NAME_ISOLATE_GETCURRENT)),
        isolate_entered_ctx: to_fn(get!(NAME_ISOLATE_ENTERED_CTX)),
        isolate_incumbent_ctx: to_fn(get!(NAME_ISOLATE_INCUMBENT_CTX)),
        handle_scope_create: to_fn(get!(NAME_HANDLE_SCOPE_CREATE)),
        handle_scope_ctor: to_fn(get!(NAME_HANDLE_SCOPE_CTOR)),
        handle_scope_dtor: to_fn(get!(NAME_HANDLE_SCOPE_DTOR)),
        string_new_from_utf8: to_fn(get!(NAME_STRING_NEW_FROM_UTF8)),
        script_compile: to_fn(get!(NAME_SCRIPT_COMPILE)),
        script_run: to_fn(get!(NAME_SCRIPT_RUN)),
        utf8_ctor: to_fn(get!(NAME_UTF8_CTOR)),
        utf8_dtor: to_fn(get!(NAME_UTF8_DTOR)),
        utf8_deref: to_fn(get!(NAME_UTF8_DEREF)),
        // mode 3 需要;缺失不阻塞 mode 0-2。
        request_interrupt: match crate::obs::export_addr(qqnt, NAME_REQUEST_INTERRUPT) {
            Some(a) => Some(to_fn(a)),
            None => {
                append_stage(report, "resolve", true, "RequestInterrupt missing (mode 3 unavailable)");
                None
            }
        },
    })
}

// --- 页校验读(与 intr 相同语义) ---

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

fn page_readable_span(addr: usize, len: usize) -> bool {
    let start = addr & !0xFFF;
    let end = (addr + len - 1) & !0xFFF;
    let mut page = start;
    while page <= end {
        if !page_readable(page) {
            return false;
        }
        page += 0x1000;
    }
    true
}

unsafe fn read_usize(p: usize) -> Option<usize> {
    if !page_readable_span(p, 8) {
        return None;
    }
    // SAFETY: 页已校验;窗口期卸载风险由报告如实呈现。
    unsafe { Some((p as *const usize).read_unaligned()) }
}

/// 回调可见的导出束(远程线程在 uv_async_init 之前发布)。
static EXPORTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe fn strlen_bounded(p: *const u8, cap: usize) -> usize {
    let mut n = 0usize;
    // SAFETY: Utf8Value 缓冲 NUL 结尾;上限防失控。
    unsafe {
        while n < cap && *p.add(n) != 0 {
            n += 1;
        }
    }
    n
}

/// uv_async 回调:在 QQ 的 JS 线程、libuv 轮转点执行。
/// 全路径零 IO/零堆分配/零锁(导出束与缓冲均预先就绪)。
unsafe extern "C" fn async_cb(_handle: *mut c_void) {
    // 原子记录(全部模式)。
    // SAFETY: GetCurrentThreadId/GetTickCount64 无副作用。
    unsafe {
        CB_TID.store(
            windows_sys::Win32::System::Threading::GetCurrentThreadId(),
            Ordering::Release,
        );
        CB_TICK.store(
            windows_sys::Win32::System::SystemInformation::GetTickCount64(),
            Ordering::Release,
        );
    }
    FIRED.store(1, Ordering::Release);
    let mode = CB_MODE.load(Ordering::Acquire);
    if mode < 2 {
        RESULT_SEQ.store(1, Ordering::Release);
        return;
    }

    // mode 2/3:JS 枚举。任何前置不满足 → 原子标记后返回,不执行 JS。
    let ex_ptr = EXPORTS.load(Ordering::Acquire);
    if ex_ptr == 0 {
        CB_JS_ERR.store(4, Ordering::Release);
        RESULT_SEQ.store(1, Ordering::Release);
        return;
    }
    let env = CB_ENV.load(Ordering::Acquire);
    let Some(env_isolate) = (unsafe { read_usize(env + 0xA0) }) else {
        CB_JS_ERR.store(3, Ordering::Release);
        RESULT_SEQ.store(1, Ordering::Release);
        return;
    };
    // SAFETY: 导出束由远程线程在 init 前发布。
    let ex: &Exports = unsafe { &*(ex_ptr as *const Exports) };

    // HandleScope:v8 HandleScope 对象实际约 0x20 字节(isolate + prev Data),
    // 取 0x40 槽位防布局漂移踩栈。
    let mut scope = [0usize; 8];
    // SAFETY: ctor/dtor 成对;isolate 来自 env 布局链(页校验)。
    unsafe {
        (ex.handle_scope_ctor)(scope.as_mut_ptr().cast(), env_isolate as *mut c_void);
    }

    let finish = |js_err: u32| {
        CB_JS_ERR.store(js_err, Ordering::Release);
        RESULT_SEQ.store(if js_err == 0 { 2 } else { 1 }, Ordering::Release);
    };
    let bail = |js_err: u32| {
        CB_JS_ERR.store(js_err, Ordering::Release);
        RESULT_SEQ.store(1, Ordering::Release);
    };

    let ctx: usize;
    let isolate = env_isolate;
    if mode == 2 {
        // 上下文阶梯:GetCurrent 一致性 → entered → incumbent;皆空则安全返回。
        // SAFETY: 无参静态导出。
        let current = unsafe { (ex.isolate_get_current)() } as usize;
        let isolate_ok = current == 0 || current == env_isolate;
        CB_ISOLATE_MATCH.store(u32::from(isolate_ok), Ordering::Release);
        if !isolate_ok {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(3);
            return;
        }
        // SAFETY: 纯读访问器(sret 约定)。
        let entered = unsafe { call_sret1(ex.isolate_entered_ctx, isolate as *mut c_void) };
        let mut found = 0usize;
        let mut src_kind = 0u32;
        if !local_empty(entered) {
            found = entered;
            src_kind = 1;
        } else {
            let incumbent = unsafe { call_sret1(ex.isolate_incumbent_ctx, isolate as *mut c_void) };
            if !local_empty(incumbent) {
                found = incumbent;
                src_kind = 2;
            }
        }
        CB_HAS_CTX.store(src_kind, Ordering::Release);
        if found == 0 {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(0);
            return;
        }
        ctx = found;
    } else {
        // mode 3:中断点捕获的 tagged 指针 → CreateHandle 转为合法 Local。
        let tag = CB_CTX_TAG.load(Ordering::Acquire);
        if tag == 0 {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(5); // 5 = 未捕获到上下文
            return;
        }
        // SAFETY: 当前 HandleScope 内分配槽位;tagged 值来自中断点实读。
        let slot = unsafe { (ex.handle_scope_create)(isolate as *mut c_void, tag) };
        if slot.is_null() {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(3);
            return;
        }
        ctx = slot as usize;
    }
    // SAFETY: 助手只做 Compile/Run/Utf8,均在当前有效 HandleScope 内。
    let script_sel = CB_SCRIPT.load(Ordering::Acquire);
    let r = unsafe { exec_enum_script(ex, isolate, ctx, script_sel) };
    // SAFETY: 成对析构。
    unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
    finish(r);
}

/// mode 3 phase A 回调:RequestInterrupt 的处理点(v8 执行 JS 的间隙)。
/// 只读 entered/incumbent Local 槽内的 tagged Context 指针并原子发布;
/// 不调用任何有副作用的 V8 API(计划 §3.1)。
unsafe extern "system" fn intr_capture_cb(_ctx: *mut c_void) {
    let ex_ptr = EXPORTS.load(Ordering::Acquire);
    if ex_ptr == 0 {
        CAP_SEQ.store(2, Ordering::Release);
        return;
    }
    // SAFETY: 导出束由远程线程在触发中断前发布。
    let ex: &Exports = unsafe { &*(ex_ptr as *const Exports) };
    let isolate = CB_ISOLATE.load(Ordering::Acquire);
    if isolate == 0 {
        CAP_SEQ.store(2, Ordering::Release);
        return;
    }
    // SAFETY: 纯读访问器(sret 约定)。
    let entered = unsafe { call_sret1(ex.isolate_entered_ctx, isolate as *mut c_void) };
    let mut tag = 0u64;
    if !local_empty(entered) {
        // Local 值 = 句柄槽地址;*槽 = tagged 对象指针(对象生命周期覆盖 env)。
        tag = unsafe { read_usize(entered) }.unwrap_or(0) as u64;
    } else {
        let incumbent = unsafe { call_sret1(ex.isolate_incumbent_ctx, isolate as *mut c_void) };
        if !local_empty(incumbent) {
            tag = unsafe { read_usize(incumbent) }.unwrap_or(0) as u64;
        }
    }
    if tag != 0 {
        CB_CTX_TAG.store(tag, Ordering::Release);
    }
    CAP_SEQ.store(1, Ordering::Release);
}

/// 在给定 isolate/context 上执行选定脚本并落结果缓冲。
/// 返回 js_err 码(0=成功)。须在有效 HandleScope 内调用。
unsafe fn exec_enum_script(ex: &Exports, isolate: usize, ctx: usize, script: u32) -> u32 {
    let src_str = match script {
        SCRIPT_LOAD_PROBE => LOAD_PROBE_SCRIPT,
        SCRIPT_LOAD_HARVEST => LOAD_HARVEST_SCRIPT,
        _ => ENUM_SCRIPT,
    };
    // String::NewFromUtf8(kNormal=0;sret 约定)。
    let script_bytes = src_str.as_bytes();
    let mut src = 0usize;
    // SAFETY: 只读静态缓冲 + 有效 isolate;sret 槽在栈上。
    unsafe {
        (ex.string_new_from_utf8)(
            core::ptr::addr_of_mut!(src),
            isolate as *mut c_void,
            script_bytes.as_ptr(),
            0,
            script_bytes.len() as i32,
        )
    };
    if local_empty(src) {
        return 1;
    }
    // ScriptOrigin(0x28 零构造;script_id=-1;host_defined 空 → ctor 尾调用短路)。
    let mut origin: [u8; 0x28] = [0; 0x28];
    origin[0x14..0x18].copy_from_slice(&(-1i32).to_le_bytes());
    // SAFETY: 参数均为刚取得的有效句柄(sret 约定)。
    let mut script = 0usize;
    unsafe {
        (ex.script_compile)(core::ptr::addr_of_mut!(script), ctx, src, origin.as_mut_ptr().cast())
    };
    if local_empty(script) {
        return 1;
    }
    // SAFETY: 方法调用(this, sret, ctx, data=空 Local 约定);script 为 Compile 产物。
    let mut result = 0usize;
    unsafe { (ex.script_run)(script, core::ptr::addr_of_mut!(result), ctx, 0) };
    if local_empty(result) {
        return 2;
    }
    // Utf8Value 取回(结构 0x18 槽位足够:ptr + length)。
    let mut utf8 = [0usize; 3];
    // SAFETY: ctor/dtor 成对。
    unsafe {
        (ex.utf8_ctor)(utf8.as_mut_ptr().cast(), isolate as *mut c_void, result);
    }
    let text = unsafe { (ex.utf8_deref)(utf8.as_mut_ptr().cast()) };
    if !text.is_null() {
        let len = unsafe { strlen_bounded(text, RESULT_CAP - 1) };
        // SAFETY: 目标为静态缓冲;长度已限。
        unsafe {
            let dst = core::ptr::addr_of_mut!(RESULT_BUF).cast::<u8>();
            core::ptr::copy_nonoverlapping(text, dst, len);
            *dst.add(len) = 0;
            RESULT_LEN.store(len as u32, Ordering::Release);
        }
    }
    // SAFETY: 成对析构。
    unsafe {
        (ex.utf8_dtor)(utf8.as_mut_ptr().cast());
    }
    0
}

fn read_wide(path_ptr: usize) -> Option<String> {
    if path_ptr == 0 {
        return None;
    }
    let p = path_ptr as *const u16;
    // SAFETY: loader 约定 NUL 结尾;扫描上限 32 KiB。
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

/// 远程线程主体。同步执行:解析 → 校验 → (mode≥1)init+send → 轮询 → 报告。
///
/// # Safety
///
/// ctx 必须指向本进程内由加载器写入的有效 [`AsyncCtx`];env 须来自
/// K2-02 扫描结果且实例健康。
pub unsafe fn async_run(ctx: &AsyncCtx) -> u32 {
    // 复位回调状态。
    FIRED.store(0, Ordering::Release);
    CB_TID.store(0, Ordering::Release);
    CB_TICK.store(0, Ordering::Release);
    CB_ISOLATE_MATCH.store(0, Ordering::Release);
    CB_HAS_CTX.store(0, Ordering::Release);
    CB_JS_ERR.store(0, Ordering::Release);
    RESULT_LEN.store(0, Ordering::Release);
    RESULT_SEQ.store(0, Ordering::Release);
    CB_MODE.store(0, Ordering::Release);
    CB_ENV.store(0, Ordering::Release);
    CB_SCRIPT.store(0, Ordering::Release);
    CB_ISOLATE.store(0, Ordering::Release);
    CB_CTX_TAG.store(0, Ordering::Release);
    CAP_SEQ.store(0, Ordering::Release);
    EXPORTS.store(0, Ordering::Release);

    let path_ptr = ctx.report_path;
    if path_ptr == 0 {
        return async_code::ERR_NULL_PATH;
    }
    let report = match read_wide(path_ptr) {
        Some(s) => s,
        None => return async_code::ERR_BAD_PATH,
    };
    if ctx.mode > 3 {
        append_stage(&report, "mode", false, &format!("bad mode {}", ctx.mode));
        return async_code::ERR_BAD_MODE;
    }
    append_stage(
        &report,
        "start",
        true,
        &format!("mode={} env={:#x} wait_ms={}", ctx.mode, ctx.env, ctx.wait_ms),
    );

    // QQNT 模块句柄:GetModuleHandleW 的 F1-R4 竞态是与宿主并发加载模块;
    // 本实验与 intr 同型(实例稳定运行、单次调用、窗口如实记录)。
    let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
    // SAFETY: 只读查询;竞态论证见上注。
    let h = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr()) };
    if h.is_null() {
        append_stage(&report, "qqnt", false, "QQNT.dll not loaded");
        return async_code::ERR_ENV_INVALID;
    }
    let Some(ex_boxed) = (unsafe { resolve_exports(h as usize, &report) }) else {
        return async_code::ERR_ENV_INVALID;
    };
    let ex = Box::leak(Box::new(ex_boxed));
    EXPORTS.store(ex as *const Exports as usize, Ordering::Release);

    // env 布局链:env+0xB0 → IsolateData → +0x11E8 → loop。
    if ctx.env == 0 {
        append_stage(&report, "dry_run", true, "env=0; exports resolved; plumbing only");
        append_stage(&report, "done", true, "dry run complete");
        return async_code::OK;
    }
    if !page_readable_span(ctx.env, 0xB60) {
        append_stage(&report, "validate_env", false, "env pages unreadable (need 0xB60 span)");
        return async_code::ERR_ENV_INVALID;
    }
    let Some(isolate_data) = (unsafe { read_usize(ctx.env + 0xB0) }) else {
        append_stage(&report, "validate_env", false, "env+0xB0 unreadable");
        return async_code::ERR_ENV_INVALID;
    };
    let Some(loop_) = (unsafe { read_usize(isolate_data + 0x11E8) }) else {
        append_stage(&report, "validate_env", false, "isolate_data+0x11E8 unreadable");
        return async_code::ERR_ENV_INVALID;
    };
    append_stage(
        &report,
        "validate_env",
        true,
        &format!("isolate_data={isolate_data:#x} loop={loop_:#x}"),
    );

    let alive = (ex.uv_loop_alive)(loop_ as *mut c_void);
    append_stage(&report, "loop_alive", alive != 0, &format!("alive={alive}"));
    if alive == 0 && ctx.mode >= 1 {
        append_stage(&report, "abort", true, "loop not alive; refusing async_init");
        return async_code::ERR_UV;
    }
    if ctx.mode == 0 {
        append_stage(&report, "done", true, "dry run complete");
        return async_code::OK;
    }

    // mode 3 phase A:中断点捕获上下文(JS 间隙必然 entered)。
    if ctx.mode == 3 {
        let Some(ri) = ex.request_interrupt else {
            append_stage(&report, "mode3", false, "RequestInterrupt export missing");
            return async_code::ERR_BAD_MODE;
        };
        let Some(env_isolate) = (unsafe { read_usize(ctx.env + 0xA0) }) else {
            append_stage(&report, "mode3", false, "env+0xA0 unreadable");
            return async_code::ERR_ENV_INVALID;
        };
        CAP_SEQ.store(0, Ordering::Release);
        CB_CTX_TAG.store(0, Ordering::Release);
        CB_ISOLATE.store(env_isolate, Ordering::Release);
        append_stage(&report, "mode3_capture", true, &format!("isolate={env_isolate:#x}"));
        // SAFETY: env 已页校验;回调只做纯读 + 原子写。
        unsafe { ri(ctx.env as *mut c_void, intr_capture_cb, std::ptr::null_mut()) };
        let cap_deadline = std::time::Instant::now() + std::time::Duration::from_millis((ctx.wait_ms.max(2) as u64) / 2);
        while CAP_SEQ.load(Ordering::Acquire) == 0 && std::time::Instant::now() < cap_deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let cap = CB_CTX_TAG.load(Ordering::Acquire);
        append_stage(
            &report,
            "mode3_capture",
            cap != 0,
            &format!("cap_seq={} ctx_tag={cap:#x}", CAP_SEQ.load(Ordering::Acquire)),
        );
        if cap == 0 {
            return async_code::ERR_NO_CAPTURE;
        }
    }

    // UV_ASYNC = 1(libuv uv_handle_type)。
    let size = (ex.uv_handle_size)(1);
    if size == 0 || size > 0x1000 {
        append_stage(&report, "alloc_async", false, &format!("uv_handle_size(UV_ASYNC)={size}"));
        return async_code::ERR_UV;
    }
    let layout = match std::alloc::Layout::from_size_align(size, 16) {
        Ok(l) => l,
        Err(_) => return async_code::ERR_UV,
    };
    let async_handle = unsafe { std::alloc::alloc(layout) };
    if async_handle.is_null() {
        append_stage(&report, "alloc_async", false, "alloc failed");
        return async_code::ERR_UV;
    }
    // SAFETY: 清零分配缓冲,防脏字段。
    unsafe {
        core::ptr::write_bytes(async_handle, 0, size);
    }

    CB_MODE.store(ctx.mode, Ordering::Release);
    CB_ENV.store(ctx.env, Ordering::Release);
    CB_SCRIPT.store(ctx.script, Ordering::Release);

    let init_r = unsafe {
        (ex.uv_async_init)(loop_ as *mut c_void, async_handle as *mut c_void, async_cb)
    };
    append_stage(&report, "uv_async_init", init_r == 0, &format!("r={init_r} loop={loop_:#x}"));
    if init_r != 0 {
        return async_code::ERR_UV;
    }
    let send_r = unsafe { (ex.uv_async_send)(async_handle as *mut c_void) };
    let started = std::time::Instant::now();
    append_stage(&report, "uv_async_send", send_r == 0, &format!("r={send_r}"));

    // 轮询(10ms 步进)。
    let deadline = started + std::time::Duration::from_millis(ctx.wait_ms.max(1) as u64);
    while RESULT_SEQ.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let fired = FIRED.load(Ordering::Acquire);
    let tid = CB_TID.load(Ordering::Acquire);
    let latency = started.elapsed().as_millis() as u64;
    append_stage(
        &report,
        "callback_fired",
        fired != 0,
        &format!("fired={fired} cb_thread_id={tid} latency_ms={latency}"),
    );
    if ctx.mode >= 2 {
        append_stage(
            &report,
            "context_ladder",
            true,
            &format!(
                "isolate_match={} has_ctx={} js_err={}",
                CB_ISOLATE_MATCH.load(Ordering::Acquire),
                CB_HAS_CTX.load(Ordering::Acquire),
                CB_JS_ERR.load(Ordering::Acquire),
            ),
        );
        if RESULT_SEQ.load(Ordering::Acquire) == 2 {
            let len = RESULT_LEN.load(Ordering::Acquire) as usize;
            // SAFETY: 回调已以 seq=2 发布;长度受缓冲上限约束。
            let text = unsafe {
                let src = core::ptr::addr_of!(RESULT_BUF).cast::<u8>();
                core::slice::from_raw_parts(src, len)
            };
            append_stage(&report, "result", true, &String::from_utf8_lossy(text));
        } else {
            append_stage(&report, "result", false, "no JS output (see context_ladder)");
        }
    }
    // 句柄不 close(需与 loop 同步;随进程退出回收——如实记录)。
    std::thread::sleep(std::time::Duration::from_millis(200));
    let still_ok = page_readable_span(ctx.env, 0x40);
    append_stage(&report, "post_check", still_ok, &format!("env still readable={still_ok}"));
    append_stage(&report, "done", true, "async round complete");
    async_code::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_script_is_self_contained() {
        // 只读枚举脚本:不赋值全局、不加定时器、有 try/catch、返回字符串。
        assert!(ENUM_SCRIPT.starts_with("(function(){"));
        assert!(ENUM_SCRIPT.contains("try"));
        assert!(ENUM_SCRIPT.contains("_linkedBinding('major')"));
        assert!(ENUM_SCRIPT.ends_with("}})()"));
        // load 探针:无参调用一次,异常捕获,不链式调用。
        assert!(LOAD_PROBE_SCRIPT.contains("m.load()"));
        assert!(LOAD_PROBE_SCRIPT.contains("callErr"));
        assert!(LOAD_PROBE_SCRIPT.contains("fnArity"));
        // 错误收割:只喂错误类型参数,不喂有效路径。
        assert!(LOAD_HARVEST_SCRIPT.contains("load(1)"));
        assert!(LOAD_HARVEST_SCRIPT.contains("globalCount"));
        assert!(!LOAD_HARVEST_SCRIPT.contains("app_launcher"));
    }

    #[test]
    fn ctx_layout_is_fixed() {
        // repr(C) 布局核对:env/mode/script/report/wait —— 与加载器写入序列一致。
        assert_eq!(std::mem::size_of::<AsyncCtx>(), std::mem::size_of::<usize>() * 3 + 8);
        let c = AsyncCtx {
            env: 0x11,
            mode: 2,
            script: 1,
            report_path: 0x22,
            wait_ms: 33,
            _pad: 0,
        };
        let base = &c as *const AsyncCtx as *const u8;
        // SAFETY: 读自身结构体字段。
        unsafe {
            assert_eq!(*(base as *const usize), 0x11);
            assert_eq!(*base.add(std::mem::size_of::<usize>()).cast::<u32>(), 2);
            assert_eq!(
                *base.add(std::mem::size_of::<usize>() + 4).cast::<u32>(),
                1
            );
            assert_eq!(
                *base.add(std::mem::size_of::<usize>() * 2).cast::<usize>(),
                0x22
            );
            assert_eq!(
                *base.add(std::mem::size_of::<usize>() * 3).cast::<u32>(),
                33
            );
        }
    }
}
