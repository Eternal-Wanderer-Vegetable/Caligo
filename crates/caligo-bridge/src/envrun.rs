//! EXP-K2-01:自建 Node Environment 执行器(方案 d,见 js-context-acquisition.md §3)。
//!
//! 链路(全部经 QQNT.dll 导出,按 manifest 冻结版本绑定):
//!   CreatePlatform → uv_loop_new → CreateArrayBufferAllocator → NewIsolate
//!   → Isolate::Enter → NewContext → Context::Enter → CreateIsolateData
//!   → CreateEnvironment → Context::Global
//!   → thunk(我们的 napi_module)   [glue 创建 napi_env,回调记下 env]
//!   → thunk(major 的 napi_module)  [major 的服务面注册到我们的 exports 对象]
//!   → napi 枚举 exports 属性名 → 报告。
//!
//! 安全/纪律:
//! - 单线程(本执行器线程)持有全部 v8/node 状态;不触碰 QQ 的 Environment;
//! - 每个阶段完成后立即向报告文件追加一行 JSONL——中途崩溃也能留下尸检数据;
//! - thunk 地址、major 的 napi_module 指针均取自运行时链表数据,不硬编码地址;
//! - 空 libc++ vector 以 24 字节零块构造;IsolateSettings 以 4 KiB 零块近似缺省;
//! - 本执行器泄漏自有 isolate/context/env(不清理),以进程退出回收——如实记录。

use std::ffi::c_void;
use std::io::Write;

/// 执行结果码(caligo_env_start 的返回值)。
pub mod env_code {
    /// 执行线程已启动并完成(最终状态见报告文件)。
    pub const OK: u32 = 0;
    /// 报告路径无效。
    pub const ERR_NULL_PATH: u32 = 1;
    /// QQNT.dll 未加载。
    pub const ERR_NO_QQNT: u32 = 2;
    /// 执行线程创建失败。
    pub const ERR_THREAD: u32 = 3;
}

// --- 阶段报告(JSONL 追加) ---

/// `pub(crate)`:intr(WU3)复用同一 JSONL 阶段报告格式。
pub(crate) fn append_stage(path: &str, stage: &str, ok: bool, detail: &str) {
    let line = format!(
        "{{\"stage\":\"{}\",\"ok\":{},\"detail\":\"{}\"}}\n",
        stage,
        ok,
        detail.replace('\\', "\\\\").replace('"', "'")
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

// --- QQNT 导出解析(按修饰名,版本绑定由 manifest 保证) ---

fn resolve(qqnt: *mut c_void, name: &str) -> Option<usize> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    let mut cname = name.as_bytes().to_vec();
    cname.push(0);
    // SAFETY: cname 为 NUL 结尾的修饰名;qqnt 为已加载的 QQNT.dll 模块句柄。
    unsafe { GetProcAddress(qqnt as _, cname.as_ptr()).map(|f| f as usize) }
}

macro_rules! proc {
    ($qqnt:expr, $name:expr, $stage:expr, $report:expr, $ty:ty) => {
        match resolve($qqnt, $name) {
            // SAFETY: 外层 unsafe 块内使用;地址来自 QQNT 导出表。
            Some(a) => std::mem::transmute::<usize, $ty>(a),
            None => {
                append_stage(
                    $report,
                    $stage,
                    false,
                    &format!("export missing: {}", $name),
                );
                return;
            }
        }
    };
}

// --- 主链路 ---

type FnCreatePlatform = unsafe extern "C" fn(i32, *mut c_void) -> *mut c_void;
type FnUvLoopNew = unsafe extern "C" fn() -> *mut c_void;
type FnCreateArrayBufferAllocator = unsafe extern "C" fn() -> *mut c_void;
type FnNewIsolate = unsafe extern "C" fn(
    allocator: *mut c_void,
    loop_: *mut c_void,
    platform: *mut c_void,
    snapshot: *mut c_void,
    settings: *const c_void,
    cppheap: *mut c_void,
) -> *mut c_void;
type FnMethodVoid = unsafe extern "C" fn(this: *mut c_void);
type FnNewContext =
    unsafe extern "C" fn(isolate: *mut c_void, object_template: *mut c_void) -> *mut c_void;
type FnCreateIsolateData = unsafe extern "C" fn(
    isolate: *mut c_void,
    loop_: *mut c_void,
    platform: *mut c_void,
    allocator: *mut c_void,
    snapshot: *mut c_void,
) -> *mut c_void;
type FnCreateEnvironment = unsafe extern "C" fn(
    isolate_data: *mut c_void,
    context: *mut c_void,
    args: *const c_void,
    exec_args: *const c_void,
    flags: u64,
    thread_id: u64,
    inspector: *mut c_void,
) -> *mut c_void;
type FnContextGlobal = unsafe extern "C" fn(this: *mut c_void) -> *mut c_void;
/// 共享 napi 注册 thunk:context_register_func(exports, module, context, priv)。
type FnThunk = unsafe extern "C" fn(
    exports: *mut c_void,
    module: *mut c_void,
    context: *mut c_void,
    priv_: *mut c_void,
);

type FnNapiOpenScope = unsafe extern "C" fn(env: *mut c_void, out: *mut *mut c_void) -> i32;
type FnNapiCloseScope = unsafe extern "C" fn(env: *mut c_void, scope: *mut c_void) -> i32;
type FnNapiGetPropertyNames =
    unsafe extern "C" fn(env: *mut c_void, obj: *mut c_void, out: *mut *mut c_void) -> i32;
type FnNapiGetArrayLength =
    unsafe extern "C" fn(env: *mut c_void, value: *mut c_void, out: *mut u32) -> i32;
type FnNapiGetElement = unsafe extern "C" fn(
    env: *mut c_void,
    obj: *mut c_void,
    index: u32,
    out: *mut *mut c_void,
) -> i32;
type FnNapiGetValueStringUtf8 = unsafe extern "C" fn(
    env: *mut c_void,
    value: *mut c_void,
    buf: *mut u8,
    size: usize,
    out_len: *mut usize,
) -> i32;

/// 在独立线程上执行整条链路。任何阶段失败都以阶段标记落盘并停止(不 panic)。
fn run_chain(report: String) {
    append_stage(&report, "start", true, "chain thread running");

    // SAFETY: 本线程执行自建环境链路;所有被调用方均为 QQNT 导出;
    // thunk 地址与 major 的 priv 取自运行时链表数据(不硬编码地址);
    // 分配的对象泄漏至进程退出(如实记录,probe 一次性)。
    unsafe {
        let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
        let qqnt = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr());
        if qqnt.is_null() {
            append_stage(&report, "qqnt", false, "QQNT.dll not loaded");
            return;
        }
        append_stage(&report, "qqnt", true, &format!("base={:p}", qqnt));

        // 1. 平台 / 事件循环 / 分配器
        let create_platform: FnCreatePlatform = proc!(
            qqnt,
            "?CreatePlatform@node@@YAPEAVMultiIsolatePlatform@1@HPEAVTracingController@v8@@@Z",
            "resolve_create_platform",
            &report,
            FnCreatePlatform
        );
        let platform = create_platform(2, std::ptr::null_mut());
        append_stage(
            &report,
            "create_platform",
            !platform.is_null(),
            &format!("platform={:p}", platform),
        );
        if platform.is_null() {
            return;
        }
        let uv_loop_new: FnUvLoopNew = proc!(
            qqnt,
            "uv_loop_new",
            "resolve_uv_loop_new",
            &report,
            FnUvLoopNew
        );
        let loop_ = uv_loop_new();
        append_stage(
            &report,
            "uv_loop_new",
            !loop_.is_null(),
            &format!("loop={:p}", loop_),
        );
        if loop_.is_null() {
            return;
        }
        let create_aba: FnCreateArrayBufferAllocator = proc!(
            qqnt,
            "?CreateArrayBufferAllocator@node@@YAPEAVArrayBufferAllocator@1@XZ",
            "resolve_allocator",
            &report,
            FnCreateArrayBufferAllocator
        );
        let allocator = create_aba();
        append_stage(
            &report,
            "allocator",
            !allocator.is_null(),
            &format!("allocator={:p}", allocator),
        );
        if allocator.is_null() {
            return;
        }

        // 2. Isolate(零化 IsolateSettings 近似缺省;CppHeap unique_ptr 为空)
        let new_isolate: FnNewIsolate = proc!(qqnt, "?NewIsolate@node@@YAPEAVIsolate@v8@@PEAVArrayBufferAllocator@1@PEAUuv_loop_s@@PEAVMultiIsolatePlatform@1@PEBVEmbedderSnapshotData@1@AEBUIsolateSettings@1@V?$unique_ptr@VCppHeap@v8@@U?$default_delete@VCppHeap@v8@@@__Cr@std@@@__Cr@std@@@Z", "resolve_new_isolate", &report, FnNewIsolate);
        let settings = vec![0u8; 4096];
        let isolate = new_isolate(
            allocator,
            loop_,
            platform,
            std::ptr::null_mut(),
            settings.as_ptr() as *const c_void,
            std::ptr::null_mut(),
        );
        append_stage(
            &report,
            "new_isolate",
            !isolate.is_null(),
            &format!("isolate={:p}", isolate),
        );
        if isolate.is_null() {
            return;
        }

        let isolate_enter: FnMethodVoid = proc!(
            qqnt,
            "?Enter@Isolate@v8@@QEAAXXZ",
            "resolve_isolate_enter",
            &report,
            FnMethodVoid
        );
        isolate_enter(isolate);
        append_stage(&report, "isolate_enter", true, "isolate entered");

        // 3. Context
        let new_context: FnNewContext = proc!(qqnt, "?NewContext@node@@YA?AV?$Local@VContext@v8@@@v8@@PEAVIsolate@3@V?$Local@VObjectTemplate@v8@@@3@@Z", "resolve_new_context", &report, FnNewContext);
        let context = new_context(isolate, std::ptr::null_mut());
        append_stage(
            &report,
            "new_context",
            !context.is_null(),
            &format!("context={:p}", context),
        );
        if context.is_null() {
            return;
        }
        let ctx_enter: FnMethodVoid = proc!(
            qqnt,
            "?Enter@Context@v8@@QEAAXXZ",
            "resolve_ctx_enter",
            &report,
            FnMethodVoid
        );
        ctx_enter(context);
        append_stage(&report, "context_enter", true, "context entered");

        // 4. IsolateData + Environment
        let create_id: FnCreateIsolateData = proc!(qqnt, "?CreateIsolateData@node@@YAPEAVIsolateData@1@PEAVIsolate@v8@@PEAUuv_loop_s@@PEAVMultiIsolatePlatform@1@PEAVArrayBufferAllocator@1@PEBVEmbedderSnapshotData@1@@Z", "resolve_create_isolate_data", &report, FnCreateIsolateData);
        let isolate_data = create_id(isolate, loop_, platform, allocator, std::ptr::null_mut());
        append_stage(
            &report,
            "create_isolate_data",
            !isolate_data.is_null(),
            &format!("isolate_data={:p}", isolate_data),
        );
        if isolate_data.is_null() {
            return;
        }

        let create_env: FnCreateEnvironment = proc!(qqnt, "?CreateEnvironment@node@@YAPEAVEnvironment@1@PEAVIsolateData@1@V?$Local@VContext@v8@@@v8@@AEBV?$vector@V?$basic_string@DU?$char_traits@D@__Cr@std@@V?$allocator@D@23@@__Cr@std@@V?$allocator@V?$basic_string@DU?$char_traits@D@__Cr@std@@V?$allocator@D@23@@__Cr@std@@@23@@__Cr@std@@2W4Flags@EnvironmentFlags@1@UThreadId@1@V?$unique_ptr@UInspectorParentHandle@node@@U?$default_delete@UInspectorParentHandle@node@@@__Cr@std@@@78@@Z", "resolve_create_environment", &report, FnCreateEnvironment);
        // 空 libc++ vector = 24 字节零块(__begin_/__end_/__end_cap_)。
        let empty_vec = [0u64; 3];
        let node_env = create_env(
            isolate_data,
            context,
            empty_vec.as_ptr() as *const c_void,
            empty_vec.as_ptr() as *const c_void,
            0,
            0,
            std::ptr::null_mut(),
        );
        append_stage(
            &report,
            "create_environment",
            !node_env.is_null(),
            &format!("node_env={:p}", node_env),
        );
        if node_env.is_null() {
            return;
        }

        // 5. exports 对象 = context 的 global
        let ctx_global: FnContextGlobal = proc!(
            qqnt,
            "?Global@Context@v8@@QEAA?AV?$Local@VObject@v8@@@2@XZ",
            "resolve_ctx_global",
            &report,
            FnContextGlobal
        );
        let global = ctx_global(context);
        append_stage(
            &report,
            "global",
            !global.is_null(),
            &format!("global={:p}", global),
        );
        if global.is_null() {
            return;
        }

        // 6. 从运行时链表取 major 节点(thunk 地址 + napi_module priv)
        let targets = crate::obs::linked_targets();
        let major = targets.iter().find(|(n, _)| n == "major").cloned();
        let Some((_, major_node_va)) = major else {
            append_stage(&report, "find_major", false, "major not in linked list");
            return;
        };
        let thunk = match crate::obs::read_node_field(
            major_node_va,
            crate::obs::node_module_offset::CONTEXT_REGISTER_FUNC,
        ) {
            Some(v) if v != 0 => v,
            _ => {
                append_stage(&report, "find_major", false, "major thunk unreadable");
                return;
            }
        };
        let major_priv =
            crate::obs::read_node_field(major_node_va, crate::obs::node_module_offset::PRIV)
                .unwrap_or(0);
        append_stage(
            &report,
            "find_major",
            true,
            &format!("thunk={thunk:#x} major_priv={major_priv:#x}"),
        );

        let thunk: FnThunk = std::mem::transmute::<usize, FnThunk>(thunk);

        // 7. 我们的绑定 → glue 创建 napi_env → entry_register 记录 env
        thunk(
            global,
            global,
            context,
            &crate::register::ENTRY_NAPI_MODULE as *const _ as *mut c_void,
        );
        let (_, fired, env_hint) = crate::register::entry_state();
        append_stage(
            &report,
            "own_binding",
            fired,
            &format!("napi_env={env_hint:#x}"),
        );

        // 8. major 的服务面注册到我们的 exports(global)上
        if major_priv != 0 {
            thunk(global, global, context, major_priv as *mut c_void);
            append_stage(
                &report,
                "major_binding",
                true,
                "major registered into our exports",
            );
        } else {
            append_stage(&report, "major_binding", false, "major priv null");
        }

        // 9. napi 枚举 exports 属性名
        let Some(env) = (if fired {
            Some(env_hint as *mut c_void)
        } else {
            None
        }) else {
            append_stage(&report, "enumerate", false, "no napi_env");
            return;
        };
        let open_scope: FnNapiOpenScope = proc!(
            qqnt,
            "napi_open_handle_scope",
            "resolve_open_scope",
            &report,
            FnNapiOpenScope
        );
        let close_scope: FnNapiCloseScope = proc!(
            qqnt,
            "napi_close_handle_scope",
            "resolve_close_scope",
            &report,
            FnNapiCloseScope
        );
        let get_names: FnNapiGetPropertyNames = proc!(
            qqnt,
            "napi_get_property_names",
            "resolve_get_names",
            &report,
            FnNapiGetPropertyNames
        );
        let get_len: FnNapiGetArrayLength = proc!(
            qqnt,
            "napi_get_array_length",
            "resolve_get_len",
            &report,
            FnNapiGetArrayLength
        );
        let get_elem: FnNapiGetElement = proc!(
            qqnt,
            "napi_get_element",
            "resolve_get_elem",
            &report,
            FnNapiGetElement
        );
        let get_str: FnNapiGetValueStringUtf8 = proc!(
            qqnt,
            "napi_get_value_string_utf8",
            "resolve_get_str",
            &report,
            FnNapiGetValueStringUtf8
        );

        let mut scope: *mut c_void = std::ptr::null_mut();
        if open_scope(env, &mut scope) != 0 {
            append_stage(&report, "open_scope", false, "open_handle_scope failed");
            return;
        }
        let mut names_arr: *mut c_void = std::ptr::null_mut();
        if get_names(env, global, &mut names_arr) != 0 || names_arr.is_null() {
            append_stage(&report, "get_names", false, "get_property_names failed");
            close_scope(env, scope);
            return;
        }
        let mut count: u32 = 0;
        let _ = get_len(env, names_arr, &mut count);
        append_stage(
            &report,
            "enumerate",
            true,
            &format!("global property count={count}"),
        );

        let mut collected: Vec<String> = Vec::new();
        for i in 0..count {
            let mut elem: *mut c_void = std::ptr::null_mut();
            if get_elem(env, names_arr, i, &mut elem) != 0 || elem.is_null() {
                continue;
            }
            let mut len: usize = 0;
            if get_str(env, elem, std::ptr::null_mut(), 0, &mut len) != 0 {
                continue;
            }
            let mut buf = vec![0u8; len + 1];
            if get_str(env, elem, buf.as_mut_ptr(), len + 1, &mut len) != 0 {
                continue;
            }
            buf.truncate(len);
            collected.push(String::from_utf8_lossy(&buf).into_owned());
        }
        close_scope(env, scope);
        append_stage(&report, "names", true, &collected.join(","));
        append_stage(&report, "done", true, "chain complete");
    }
}

/// 启动执行线程(立即返回;结果以报告文件呈现)。
pub fn env_start(report_path: &str) -> u32 {
    let report = report_path.to_string();
    match std::thread::Builder::new()
        .name("caligo-env-chain".into())
        .stack_size(4 * 1024 * 1024)
        .spawn(move || run_chain(report))
    {
        Ok(_) => env_code::OK,
        Err(_) => env_code::ERR_THREAD,
    }
}
