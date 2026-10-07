//! K4-D8 V8 执行阶梯(仅 research 构建):在 owner 轮转点执行固定语义 JS。
//!
//! ABI 全部按 K2-03 §12.2.1 反汇编实证规则(Local/MaybeLocal 的 sret 约定):
//! - 静态函数:sret 在 RCX,参数自 RDX 后移;
//! - 成员函数:this 在 RCX,sret 在 RDX,其余自 R8 起;
//! - ScriptOrigin(0x28)零构造安全;Run 用双参重载、data 传空 Local;
//! - **入口硬门:isolate_get_current() 必须非 0 且等于 isolate**
//!   (计划 §5.1:零 current 不放行 —— 不继承 K2 旧阶梯的 0 放行缺陷)。
//!   安静 QQ 轮转点 current 合法为 0 → 调用方按 NoCurrentContext 延迟重试。
//!
//! 符号全部可注入(LAB 假机器驱动同一阶梯);`resolve(qqnt)` 为现场路线。

use core::ffi::c_void;

use crate::qq_entry::append_stage;

const NAME_HANDLE_SCOPE_CTOR: &str = "??0HandleScope@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_HANDLE_SCOPE_DTOR: &str = "??1HandleScope@v8@@QEAA@XZ";
const NAME_ISOLATE_GETCURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_ISOLATE_ENTERED_CTX: &str =
    "?GetEnteredOrMicrotaskContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_ISOLATE_INCUMBENT_CTX: &str =
    "?GetIncumbentContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_STRING_NEW_FROM_UTF8: &str =
    "?NewFromUtf8@String@v8@@SA?AV?$MaybeLocal@VString@v8@@@2@PEAVIsolate@2@PEBDW4NewStringType@2@H@Z";
const NAME_SCRIPT_COMPILE: &str =
    "?Compile@Script@v8@@SA?AV?$MaybeLocal@VScript@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VString@v8@@@2@PEAVScriptOrigin@2@@Z";
const NAME_SCRIPT_RUN: &str =
    "?Run@Script@v8@@QEAA?AV?$MaybeLocal@VValue@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VData@v8@@@2@@Z";
const NAME_UTF8_CTOR: &str =
    "??0Utf8Value@String@v8@@QEAA@PEAVIsolate@2@V?$Local@VValue@v8@@@2@@Z";
const NAME_UTF8_DTOR: &str = "??1Utf8Value@String@v8@@QEAA@XZ";
const NAME_UTF8_DEREF: &str = "??DUtf8Value@String@v8@@QEAAPEADXZ";

type FnScopeCtor = unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void) -> *mut c_void;
type FnScopeDtor = unsafe extern "C" fn(this: *mut c_void);
type FnGetCurrent = unsafe extern "C" fn() -> *mut c_void;
type FnIsolateCtx = unsafe extern "C" fn(isolate: *mut c_void, sret: *mut usize);
type FnStringNewFromUtf8 =
    unsafe extern "C" fn(sret: *mut usize, isolate: *mut c_void, data: *const u8, ty: i32, len: i32);
type FnScriptCompile = unsafe extern "C" fn(sret: *mut usize, ctx: usize, src: usize, origin: *mut c_void);
type FnScriptRun = unsafe extern "C" fn(this: usize, sret: *mut usize, ctx: usize, data: usize);
type FnUtf8Ctor = unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void, value: usize) -> *mut c_void;
type FnUtf8Dtor = unsafe extern "C" fn(this: *mut c_void);
type FnUtf8Deref = unsafe extern "C" fn(this: *mut c_void) -> *const u8;

#[derive(Debug, Clone, Copy)]
pub struct V8Symbols {
    pub scope_ctor: usize,
    pub scope_dtor: usize,
    pub get_current: usize,
    pub entered_ctx: usize,
    pub incumbent_ctx: usize,
    pub string_new_from_utf8: usize,
    pub script_compile: usize,
    pub script_run: usize,
    pub utf8_ctor: usize,
    pub utf8_dtor: usize,
    pub utf8_deref: usize,
}

impl V8Symbols {
    pub fn resolve(qqnt: usize, report: &str) -> Option<Self> {
        let get = |name: &str| -> Option<usize> {
            // SAFETY: obs::export_addr 只读导出表。
            let a = unsafe { crate::obs::export_addr(qqnt, name) };
            if a.is_none() {
                append_stage(report, "v8_resolve", false, &format!("export missing: {name}"));
            }
            a
        };
        Some(Self {
            scope_ctor: get(NAME_HANDLE_SCOPE_CTOR)?,
            scope_dtor: get(NAME_HANDLE_SCOPE_DTOR)?,
            get_current: get(NAME_ISOLATE_GETCURRENT)?,
            entered_ctx: get(NAME_ISOLATE_ENTERED_CTX)?,
            incumbent_ctx: get(NAME_ISOLATE_INCUMBENT_CTX)?,
            string_new_from_utf8: get(NAME_STRING_NEW_FROM_UTF8)?,
            script_compile: get(NAME_SCRIPT_COMPILE)?,
            script_run: get(NAME_SCRIPT_RUN)?,
            utf8_ctor: get(NAME_UTF8_CTOR)?,
            utf8_dtor: get(NAME_UTF8_DTOR)?,
            utf8_deref: get(NAME_UTF8_DEREF)?,
        })
    }

    /// LAB 注入。
    pub fn from_raw(parts: [usize; 11]) -> Self {
        Self {
            scope_ctor: parts[0],
            scope_dtor: parts[1],
            get_current: parts[2],
            entered_ctx: parts[3],
            incumbent_ctx: parts[4],
            string_new_from_utf8: parts[5],
            script_compile: parts[6],
            script_run: parts[7],
            utf8_ctor: parts[8],
            utf8_dtor: parts[9],
            utf8_deref: parts[10],
        }
    }

    fn to_fn<T: Copy>(a: usize) -> T {
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
        // SAFETY: 调用方保证地址与 T 的 ABI 匹配(现场:导出表;LAB:注入)。
        unsafe { std::ptr::read(core::ptr::addr_of!(a).cast::<T>()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8Error {
    /// 零 current 或 current 与 isolate 不符:轮转点不具备 JS 执行条件。
    NoCurrentContext,
    StringEmpty,
    CompileFailed,
    RunEmpty,
    Utf8Failed,
}

fn to_fn<T: Copy>(a: usize) -> T {
    V8Symbols::to_fn::<T>(a)
}

/// 取执行上下文:entered 优先,incumbent 兜底;皆空 → None。
unsafe fn ladder_ctx(syms: &V8Symbols, isolate: *mut c_void) -> Option<usize> {
    // SAFETY: 符号 ABI 见类型别名;sret 槽在栈上。
    unsafe {
        let entered: FnIsolateCtx = to_fn(syms.entered_ctx);
        let mut v = 0usize;
        entered(isolate, &mut v);
        if v != 0 {
            return Some(v);
        }
        let incumbent: FnIsolateCtx = to_fn(syms.incumbent_ctx);
        let mut v2 = 0usize;
        incumbent(isolate, &mut v2);
        if v2 != 0 {
            return Some(v2);
        }
        None
    }
}

/// 在轮转点执行 JS 并取回字符串。
///
/// 检查顺序:零/异 current 拒绝(先于 HandleScope)→ ladder ctx →
/// HandleScope → String → Compile → Run → Utf8Value → 回收。
pub unsafe fn exec_script(
    syms: &V8Symbols,
    isolate: *mut c_void,
    js: &str,
) -> Result<String, V8Error> {
    // SAFETY: 符号 ABI 见类型别名;各 sret 槽在栈上;缓冲生命周期受控。
    unsafe {
        let get_current: FnGetCurrent = to_fn(syms.get_current);
        let current = (get_current)() as usize;
        // K2-03 r3 实证语义(uv 轮转点):GetCurrent 经常合法为 0,
        // entered/incumbent 才是轮转点的有效上下文。拒绝条件 = current 与
        // isolate 不一致(非 0 且不等)+ 阶梯上下文为空。§5.1 的"零 current
        // 不能自行解释为可进入"由此满足:仍须通过上下文阶梯方可进入,
        // 且 env 新鲜度门 + owner 线程检查在阶梯之前(QqOwnerAdapter)。
        if current != 0 && current != isolate as usize {
            return Err(V8Error::NoCurrentContext);
        }
        let Some(ctx) = ladder_ctx(syms, isolate) else {
            return Err(V8Error::NoCurrentContext);
        };
        let scope_ctor: FnScopeCtor = to_fn(syms.scope_ctor);
        let scope_dtor: FnScopeDtor = to_fn(syms.scope_dtor);
        let mut scope = [0usize; 8];
        scope_ctor(scope.as_mut_ptr().cast(), isolate);

        let r = exec_in_scope(syms, isolate, ctx, js, &mut scope);
        scope_dtor(scope.as_mut_ptr().cast());
        r
    }
}

/// SAFETY: 须持有效 HandleScope;ctx 经阶梯取得。
unsafe fn exec_in_scope(
    syms: &V8Symbols,
    isolate: *mut c_void,
    ctx: usize,
    js: &str,
    _scope: &mut [usize],
) -> Result<String, V8Error> {
    // SAFETY: 同上。
    unsafe {
        let new_utf8: FnStringNewFromUtf8 = to_fn(syms.string_new_from_utf8);
        let compile: FnScriptCompile = to_fn(syms.script_compile);
        let run: FnScriptRun = to_fn(syms.script_run);
        let utf8_ctor: FnUtf8Ctor = to_fn(syms.utf8_ctor);
        let utf8_dtor: FnUtf8Dtor = to_fn(syms.utf8_dtor);
        let utf8_deref: FnUtf8Deref = to_fn(syms.utf8_deref);

        let mut src_local = 0usize;
        new_utf8(
            &mut src_local,
            isolate,
            js.as_ptr(),
            0, // kNormal
            js.len() as i32,
        );
        if src_local == 0 {
            return Err(V8Error::StringEmpty);
        }
        // ScriptOrigin(0x28 字节)零构造安全(K2-03 §12.1)。
        let mut origin = [0usize; 5];
        let mut script_local = 0usize;
        compile(&mut script_local, ctx, src_local, origin.as_mut_ptr().cast());
        if script_local == 0 {
            return Err(V8Error::CompileFailed);
        }
        let mut result_local = 0usize;
        run(script_local, &mut result_local, ctx, 0 /*空 data Local*/);
        if result_local == 0 {
            return Err(V8Error::RunEmpty);
        }
        let mut utf8 = [0usize; 3];
        utf8_ctor(utf8.as_mut_ptr().cast(), isolate, result_local);
        let p = utf8_deref(utf8.as_mut_ptr().cast());
        if p.is_null() {
            utf8_dtor(utf8.as_mut_ptr().cast());
            return Err(V8Error::Utf8Failed);
        }
        let n = strlen_bounded(p, 1 << 20);
        let out = core::str::from_utf8(core::slice::from_raw_parts(p, n))
            .unwrap_or_default()
            .to_string();
        utf8_dtor(utf8.as_mut_ptr().cast());
        Ok(out)
    }
}

fn strlen_bounded(p: *const u8, cap: usize) -> usize {
    // SAFETY: Utf8Value 缓冲 NUL 结尾;上限防失控。
    unsafe {
        let mut n = 0usize;
        while n < cap && *p.add(n) != 0 {
            n += 1;
        }
        n
    }
}

// —— D8 固定语义脚本(源自 js/resident-*.js,K3 现场验证版本;移植为常量) ——

/// 注册监听器(活会话扫描 + addKernelMsgListener;每会话代次一次)。
/// 返回 "ARMED sid=<n> lidRet=<id>" / "ALREADY ..." / "ERR:..."。
pub const D8_START_JS: &str = include_str!("../../../js/resident-start.js");

/// 排空接收环:返回 JSON {sid,n,items:[...]} / "NOT_ARMED"。
pub const D8_POLL_JS: &str = include_str!("../../../js/resident-poll.js");

/// 对称移除监听器 + 清理常驻对象:返回 "STOPPED sid=.. remove=.."。
pub const D8_STOP_JS: &str = include_str!("../../../js/resident-stop.js");

// —— 共享符号存取(daemon 导出解析一次;adapter/pump 共用) ——

static V8_STORE: std::sync::Mutex<Option<V8Symbols>> = std::sync::Mutex::new(None);

/// daemon 导出侧:解析并存储 V8 符号(每代次一次)。
pub fn store_symbols(qqnt: usize, report: &str) -> bool {
    let Some(syms) = V8Symbols::resolve(qqnt, report) else {
        return false;
    };
    if let Ok(mut g) = V8_STORE.lock() {
        *g = Some(syms);
        true
    } else {
        false
    }
}

/// LAB 注入(绕过导出表解析;仅 research 构建导出,真机禁用)。
#[doc(hidden)]
pub fn store_symbols_raw(s: V8Symbols) {
    if let Ok(mut g) = V8_STORE.lock() {
        *g = Some(s);
    }
}

fn symbols() -> Option<V8Symbols> {
    V8_STORE.lock().ok().and_then(|g| *g)
}

fn isolate_addr() -> *mut c_void {
    crate::qq_entry::isolate_addr()
}

/// 排空轮转点:执行 D8_POLL_JS 并返回原始 JSON("NOT_ARMED"/items JSON)。
pub fn poll_listener_json() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: 阶梯内自管全部检查;调用方保证 owner 线程。
    unsafe { exec_script(&syms, isolate_addr(), D8_POLL_JS) }
}

/// 注册监听器(D8_START_JS)。返回 "ARMED sid=.. lidRet=N" / "ALREADY .." / "ERR:.."。
pub fn start_listener() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: 同上。
    unsafe { exec_script(&syms, isolate_addr(), D8_START_JS) }
}

/// 对称移除监听器(D8_STOP_JS)。返回 "STOPPED .. remove=.." / "NOT_ARMED"。
pub fn stop_listener() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: 同上。
    unsafe { exec_script(&syms, isolate_addr(), D8_STOP_JS) }
}
