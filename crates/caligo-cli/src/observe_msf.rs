//! P1 受控观测(计划 §7-P1 失败分支):外部只读读取 QQ 进程内的
//! MSF 单例 / transport(+0x60) / 每线程 TLS 提交目标。
//!
//! 安全边界(与 envscan 同模型):
//! - OpenProcess 仅申请 `QUERY_INFORMATION | VM_READ`;OpenThread 仅
//!   `THREAD_QUERY_INFORMATION`;全部数据经 ReadProcessMemory 读取;
//! - **零写入、零远程线程、零注入、零 QQ 函数调用** —— 不消耗目标实例
//!   的首次 bootstrap 机会,不影响 QQ 运行,失败可安全重试;
//! - 锚点 RVA 来自固定样本(SHA `63112ab9…`)的 R2/R3 静态证据;
//!   目标模块基址运行时解析,任何越界/不可读一律如实记 `unreadable`。
//!
//! TLS 布局自校准:在本进程 TlsAlloc+TlsSetValue(魔数),再经 RPM 读自身
//! TEB 验证"索引 → 值地址"的实际规则(内联槽 0xE10 / 扩展数组指针偏移),
//! 校准后的规则应用到目标线程 TEB —— 不硬编码未证实的布局假设。
//!
//! 一次运行 = 一次实验;每个未知(transport vtable 实际目标 / TLS
//! dispatcher 身份)各占独立输出段。


/// 固定样本 SHA256(锚点仅对该样本有效;基址运行时解析)。
pub const WRAPPER_SHA256: &str =
    "63112ab9161e127f5f7e17998a7196e143808923fb54cbbf7b4e21426187a5f0";

// —— 锚点 RVA(相对 wrapper.node 基址;R2/R3 静态证据)——
const MSF_SERVICE_OBJ: u64 = 0x674D2C0;
const MSF_SERVICE_CTRL: u64 = 0x674D2C8;
const MSF_CORE_OBJ: u64 = 0x674D3E8;
const MSF_CORE_CTRL: u64 = 0x674D3F0;
const CORE_EXECUTOR_OBJ: u64 = 0x750088;
const EXECUTOR_TLS_INDEX: u64 = 0x6753454;
const ANCHOR_VTBL_SVC: u64 = 0x3F6DED8;
const ANCHOR_VTBL_CORE: u64 = 0x3F70C18;
const ANCHOR_VTBL_EXEC: u64 = 0x3FAD868;
const GLOBAL_DISPATCHER_OBJ: u64 = 0x67510A8; // b816e8 静态存储 +8(发送链提交目标)

const SLOT_COUNT: usize = 16; // 覆盖 vtable+0x38(slot7,发送路径调用点)

const CAL_MAGIC: u64 = 0x0B5C_A11A_5EED_0001;

pub fn cmd_observe_msf(args: &[String]) -> Result<(), String> {
    let mut pid: u32 = 0;
    let mut out: Option<String> = None;
    let mut self_test = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pid" => {
                i += 1;
                pid = args.get(i).and_then(|s| s.parse().ok()).ok_or("--pid 需要 PID")?;
            }
            "--out" => {
                i += 1;
                out = Some(args.get(i).cloned().ok_or("--out 需要路径")?);
            }
            "--self-test" => self_test = true,
            other => return Err(format!("未知参数 {other}(支持 --pid/--out/--self-test)")),
        }
        i += 1;
    }
    let mut planted: Option<(u32, *mut core::ffi::c_void, *mut [u64])> = None; // (tls_idx, pair_ptr, fake_obj)
    if self_test {
        pid = std::process::id();
        println!("[self-test] 观测对象 = 本进程 pid={pid}(校验机械;无 wrapper 属预期)");
        // 植入假 pair 走完整命中路径:pair={fake_obj,0},fake_obj={vptr,0},
        // vptr = ntdll 内真实函数地址(落在已知模块 → 命中判定应通过)。
        use windows_sys::Win32::System::Threading::{TlsAlloc, TlsSetValue};
        // SAFETY: 自进程校验;用后释放。
        unsafe {
            let ntdll = crate::winutil::to_wide("ntdll.dll");
            let h = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(ntdll.as_ptr());
            if h.is_null() {
                return Err("self-test: ntdll 未找到".into());
            }
            let name = std::ffi::CString::new("NtQueryInformationThread").unwrap();
            let func = windows_sys::Win32::System::LibraryLoader::GetProcAddress(h, name.as_ptr() as *const u8);
            let Some(func) = func else { return Err("self-test: 函数地址未找到".into()) };
            let func_addr = func as usize as u64;
            let fake_obj: Box<[u64]> = vec![func_addr, 0].into_boxed_slice();
            let fake_obj_ptr = Box::into_raw(fake_obj);
            let fake_obj_addr = fake_obj_ptr as *const u64 as usize as u64;
            let pair: Box<[u64]> = vec![fake_obj_addr, 0].into_boxed_slice();
            let pair_ptr = Box::into_raw(pair) as *mut core::ffi::c_void;
            let idx = TlsAlloc();
            if idx == u32::MAX {
                return Err("self-test: TlsAlloc 失败".into());
            }
            if TlsSetValue(idx, pair_ptr) == 0 {
                return Err("self-test: TlsSetValue 失败".into());
            }
            planted = Some((idx, pair_ptr, fake_obj_ptr));
            println!("[self-test] 已植入假 pair:fake_obj={fake_obj_ptr:?} vptr={func_addr:#x}(ntdll) tls_idx={idx}");
        }
    }
    if pid == 0 {
        return Err("缺少 --pid(或 --self-test)".into());
    }
    let report = observe(pid, planted.as_ref().map(|(idx, _, _)| *idx as u64), self_test)?;
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    match &out {
        Some(path) => {
            std::fs::write(path, &text).map_err(|e| format!("写 {path}: {e}"))?;
            println!("[observe-msf] 证据已写入 {path}");
        }
        None => println!("{text}"),
    }
    print_summary(&report);
    if let Some((idx, pair_ptr, fake_obj)) = planted {
        use windows_sys::Win32::System::Threading::{TlsFree, TlsSetValue};
        // SAFETY: 自进程清理;植入口为本线程设置。
        unsafe {
            TlsSetValue(idx, std::ptr::null_mut());
            TlsFree(idx);
            drop(Box::from_raw(pair_ptr as *mut [u64; 2]));
            drop(Box::from_raw(fake_obj));
        }
    }
    Ok(())
}

fn print_summary(r: &ObserveReport) {
    println!(
        "[observe-msf] pid={} wrapper={}",
        r.pid,
        r.wrapper
            .as_ref()
            .map(|w| format!("base={:#x} size={}", w.base, w.size))
            .unwrap_or_else(|| "未找到".into())
    );
    for s in &r.singletons {
        match &s.obj {
            Some(o) => {
                println!(
                    "[observe-msf] {} @+{:#x}: obj={:#x} vptr=+{:#x}(+{}) strong={:?} weak={:?}",
                    s.name,
                    s.storage_rva,
                    o.obj,
                    o.vptr_rva.unwrap_or(u64::MAX),
                    if o.vptr_matches_anchor { "锚点命中" } else { "锚点不匹配!" },
                    o.ctrl_strong,
                    o.ctrl_weak
                );
                match &o.transport {
                    Some(t) => println!(
                        "[observe-msf]   transport(+0x60)={:#x} vptr={:#x} ({});slot7(+0x38)={}",
                        t.ptr,
                        t.vptr,
                        t.vptr_module.as_deref().unwrap_or("未知模块"),
                        t.slots.get(7).cloned().unwrap_or_default()
                    ),
                    None => println!("[observe-msf]   transport(+0x60) = null(未安装)"),
                }
            }
            None => println!("[observe-msf] {} @+{:#x}: 未构造(getter 尚未被调用)", s.name, s.storage_rva),
        }
    }
    let installed: Vec<&ThreadReport> = r.threads.iter().filter(|t| t.dispatcher_obj.is_some()).collect();
    if let Some(g) = &r.global_dispatcher {
        println!(
            "[observe-msf] 全局 dispatcher @+{:#x}: obj={:#x} vptr_rva={:?}({}) owner_tid={:?} pair={:?} slot0={}",
            GLOBAL_DISPATCHER_OBJ,
            g.obj,
            g.vptr_rva,
            g.vptr_module.as_deref().unwrap_or("?"),
            g.field_58_tid,
            g.field_8_pair,
            g.slots.first().cloned().unwrap_or_default()
        );
    }
    println!(
        "[observe-msf] 执行器单例 @+{:#x}: vptr={:?}(锚点{});TLS index={:?}({});线程 {} 个,dispatcher 已安装 {} 个{}",
        r.executor_static.storage_rva,
        r.executor_static.vptr_rva,
        if r.executor_static.vptr_matches_anchor { "命中" } else { "未命中/未构造" },
        r.tls.index_raw,
        if r.tls.index_valid { "有效" } else { "无效/未分配" },
        r.threads.len(),
        installed.len(),
        installed
            .first()
            .map(|t| format!(
                "(示例 tid={} vptr={} ({}),slot0={})",
                t.tid,
                t.vptr.map(|v| format!("{v:#x}")).unwrap_or_default(),
                t.vptr_module.as_deref().unwrap_or("?"),
                t.slots.first().cloned().unwrap_or_default()
            ))
            .unwrap_or_default()
    );
    for n in &r.notes {
        println!("[observe-msf] 注记: {n}");
    }
}

// —— 报告结构 ——

#[derive(serde::Serialize)]
struct ObserveReport {
    kind: &'static str,
    pid: u32,
    observed_at_unix_ms: u64,
    target_process_created_unix_ms: Option<u64>,
    wrapper: Option<WrapperInfo>,
    wrapper_sha256_expected: &'static str,
    singletons: Vec<SingletonReport>,
    executor_static: ExecutorStaticReport,
    global_dispatcher: Option<GlobalDispatcherReport>,
    tls: TlsReport,
    threads: Vec<ThreadReport>,
    notes: Vec<String>,
}

#[derive(serde::Serialize)]
struct WrapperInfo {
    name: String,
    base: u64,
    size: u32,
}

#[derive(serde::Serialize)]
struct SingletonReport {
    name: &'static str,
    storage_rva: u64,
    ctrl_rva: u64,
    obj: Option<SingletonObj>,
}

#[derive(serde::Serialize)]
struct SingletonObj {
    obj: u64,
    ctrl: Option<u64>,
    ctrl_strong: Option<i32>,
    ctrl_weak: Option<i32>,
    vptr: u64,
    vptr_rva: Option<u64>,
    vptr_matches_anchor: bool,
    slots: Vec<String>,
    field_50: u64,
    field_140: u64,
    field_170_head: u64,
    field_178_sentinel: u64,
    field_60: Option<u64>,
    field_70: u64,
    field_1f0_hex: String,
    field_270: u64,
    transport: Option<TransportReport>,
}

#[derive(serde::Serialize)]
struct TransportReport {
    ptr: u64,
    vptr: u64,
    vptr_module: Option<String>,
    slots: Vec<String>,
    head_hex: String,
}

#[derive(serde::Serialize)]
struct ExecutorStaticReport {
    storage_rva: u64,
    vptr: Option<u64>,
    vptr_rva: Option<u64>,
    vptr_matches_anchor: bool,
}

#[derive(serde::Serialize)]
struct GlobalDispatcherReport {
    obj: u64,
    vptr: u64,
    vptr_rva: Option<u64>,
    vptr_module: Option<String>,
    slots: Vec<String>,
    /// dispatcher+0x58 的 owner TID(31F88EA 跨线程判定字段)。
    field_58_tid: Option<i32>,
    /// dispatcher+8 的内联 pair(队列对象)。
    field_8_pair: Option<u64>,
}

#[derive(serde::Serialize)]
struct TlsReport {
    index_storage_rva: u64,
    index_raw: Option<i32>,
    index_valid: bool,
    calibration: Vec<String>,
}

#[derive(serde::Serialize)]
struct ThreadReport {
    tid: u32,
    teb: Option<u64>,
    slot_rule: Option<String>,
    pair: Option<u64>,
    dispatcher_obj: Option<u64>,
    vptr: Option<u64>,
    vptr_rva: Option<u64>,
    vptr_module: Option<String>,
    slots: Vec<String>,
    ctrl: Option<u64>,
    ctrl_strong: Option<i32>,
    ctrl_weak: Option<i32>,
}

// —— 观测器(单一 RPM 路径) ——

struct Observer {
    handle: windows_sys::Win32::Foundation::HANDLE,
    base: u64,
    modules: Vec<crate::winutil::ModuleInfo>,
}

impl Observer {
    /// SAFETY: 只读。
    fn q(&self, addr: u64) -> Option<u64> {
        unsafe { rpm::<u64>(self.handle, addr) }
    }
    fn i32(&self, addr: u64) -> Option<i32> {
        unsafe { rpm::<i32>(self.handle, addr) }
    }
    fn hex(&self, addr: u64, len: usize) -> String {
        use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        let mut buf = vec![0u8; len];
        let mut got = 0usize;
        // SAFETY: 只读转储。
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const core::ffi::c_void,
                buf.as_mut_ptr().cast(),
                len,
                &mut got,
            )
        };
        if ok == 0 {
            return "unreadable".into();
        }
        buf.truncate(got);
        buf.iter().map(|b| format!("{b:02X}")).collect()
    }
    fn module_of(&self, addr: u64) -> Option<&str> {
        self.modules
            .iter()
            .find(|m| (addr as usize) >= m.base && (addr as usize) < m.base + m.size as usize)
            .map(|m| m.name.as_str())
    }
    fn vtable(&self, vptr: u64, n: usize) -> Vec<String> {
        (0..n)
            .map(|k| self.q(vptr + 8 * k as u64).map(|v| format!("{v:#x}")).unwrap_or_else(|| "unreadable".into()))
            .collect()
    }
}

fn observe(pid: u32, planted_idx: Option<u64>, self_test: bool) -> Result<ObserveReport, String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    let now = now_unix_ms();
    let mut notes = Vec::new();

    // SAFETY: 只读句柄;函数返回前关闭。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        return Err(format!(
            "OpenProcess({pid}) 失败 —— 目标须与本工具同用户会话;若 QQ 以管理员运行,工具也需提权"
        ));
    }
    let created = target_created_unix_ms(handle);

    let modules = crate::winutil::modules_in(pid)?;
    let wrapper = modules
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("wrapper.node"))
        .map(|m| WrapperInfo { name: m.name.clone(), base: m.base as u64, size: m.size });
    let base = wrapper.as_ref().map(|w| w.base).unwrap_or(0);
    let obs = Observer { handle, base, modules };

    let mut singletons = Vec::new();
    if base != 0 {
        singletons.push(read_singleton(&obs, "MSFService", MSF_SERVICE_OBJ, MSF_SERVICE_CTRL, ANCHOR_VTBL_SVC));
        singletons.push(read_singleton(&obs, "MSFCoreService", MSF_CORE_OBJ, MSF_CORE_CTRL, ANCHOR_VTBL_CORE));
    } else {
        notes.push("wrapper.node 不在目标模块表中(目标不是加载了 wrapper 的 QQ 进程?)".into());
    }

    // 发送链全局 dispatcher(b7ce8a 经 D32138 提交的目标)。
    let global_dispatcher = if base != 0 {
        match obs.q(base + GLOBAL_DISPATCHER_OBJ).filter(|v| *v != 0) {
            Some(obj) => {
                let vptr = obs.q(obj).unwrap_or(0);
                Some(GlobalDispatcherReport {
                    obj,
                    vptr,
                    vptr_rva: vptr.checked_sub(base).filter(|r| *r < 0x800_0000),
                    vptr_module: obs.module_of(vptr).map(|m| m.to_string()),
                    slots: obs.vtable(vptr, 8),
                    field_58_tid: obs.i32(obj + 0x58),
                    field_8_pair: obs.q(obj + 8),
                })
            }
            None => None,
        }
    } else {
        None
    };

    let executor_static = if base != 0 {
        match obs.q(base + CORE_EXECUTOR_OBJ) {
            Some(vptr) => {
                let rva = vptr.checked_sub(base).filter(|r| *r < 0x800_0000);
                ExecutorStaticReport {
                    storage_rva: CORE_EXECUTOR_OBJ,
                    vptr: Some(vptr),
                    vptr_rva: rva,
                    vptr_matches_anchor: rva == Some(ANCHOR_VTBL_EXEC),
                }
            }
            None => ExecutorStaticReport {
                storage_rva: CORE_EXECUTOR_OBJ,
                vptr: None,
                vptr_rva: None,
                vptr_matches_anchor: false,
            },
        }
    } else {
        ExecutorStaticReport { storage_rva: CORE_EXECUTOR_OBJ, vptr: None, vptr_rva: None, vptr_matches_anchor: false }
    };

    let (layout, calibration) = calibrate_tls_layout(handle);
    let index_raw = if base != 0 { obs.i32(base + EXECUTOR_TLS_INDEX) } else { None };
    let index_valid = matches!(index_raw, Some(i) if i >= 0);
    let tls = TlsReport {
        index_storage_rva: EXECUTOR_TLS_INDEX,
        index_raw,
        index_valid,
        calibration,
    };

    let mut threads = Vec::new();
    let mut eff_index: Option<u64> = index_raw.filter(|i| *i >= 0).map(|i| i as u64);
    if self_test {
        // self-test:用植入槽的真实 index 驱动线程枚举(无 wrapper 也走全机械)。
        eff_index = planted_idx.or(eff_index);
    }
    if let Some(idx) = eff_index {
        threads = enumerate_thread_tls(&obs, pid, idx, layout);
        notes.push(format!(
            "TLS index={idx} 全线程读取完成;命中判定 = 值非空且 [值] 的 vptr 落在已知模块"
        ));
    }

    if self_test {
        // 命中路径必须验证:本线程应带着植入 pair 命中(vptr 落在 ntdll)。
        let my_tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        match threads.iter().find(|t| t.tid == my_tid) {
            Some(t) if t.dispatcher_obj.is_some() => {
                println!("[self-test] 命中路径 OK:tid={my_tid} rule={:?} pair={:?}", t.slot_rule, t.pair);
            }
            Some(t) => println!(
                "[self-test] 线程已枚举但未命中:tid={my_tid} teb={:?} rule={:?}(植入槽未被命中,见 JSON 细节)",
                t.teb, t.slot_rule
            ),
            None => println!("[self-test] 线程枚举未覆盖本线程 tid={my_tid} —— 机械缺陷!"),
        }
    }

    unsafe { CloseHandle(handle) };

    Ok(ObserveReport {
        kind: "caligo-observe-msf-v1",
        pid,
        observed_at_unix_ms: now,
        target_process_created_unix_ms: created,
        wrapper,
        wrapper_sha256_expected: WRAPPER_SHA256,
        singletons,
        executor_static,
        global_dispatcher,
        tls,
        threads,
        notes,
    })
}

fn read_singleton(
    obs: &Observer,
    name: &'static str,
    storage_rva: u64,
    ctrl_rva: u64,
    anchor_vtbl: u64,
) -> SingletonReport {
    let Some(obj) = obs.q(obs.base + storage_rva).filter(|v| *v != 0) else {
        return SingletonReport { name, storage_rva, ctrl_rva, obj: None };
    };
    let ctrl = obs.q(obs.base + ctrl_rva).filter(|v| *v != 0);
    let (ctrl_strong, ctrl_weak) = match ctrl {
        Some(c) => (obs.i32(c + 8), obs.i32(c + 0x10)),
        None => (None, None),
    };
    let vptr = obs.q(obj).unwrap_or(0);
    let vptr_rva = vptr.checked_sub(obs.base).filter(|r| *r < 0x800_0000);
    let field_60 = obs.q(obj + 0x60);
    let transport = field_60.filter(|t| *t != 0).map(|t| {
        let vptr_t = obs.q(t).unwrap_or(0);
        TransportReport {
            ptr: t,
            vptr: vptr_t,
            vptr_module: obs.module_of(vptr_t).map(|m| m.to_string()),
            slots: obs.vtable(vptr_t, SLOT_COUNT),
            head_hex: obs.hex(t, 0x40),
        }
    });
    SingletonReport {
        name,
        storage_rva,
        ctrl_rva,
        obj: Some(SingletonObj {
            obj,
            ctrl,
            ctrl_strong,
            ctrl_weak,
            vptr,
            vptr_rva,
            vptr_matches_anchor: vptr_rva == Some(anchor_vtbl),
            slots: obs.vtable(vptr, SLOT_COUNT),
            field_140: obs.q(obj + 0x140).unwrap_or(0),
            field_170_head: obs.q(obj + 0x170).unwrap_or(0),
            field_178_sentinel: obs.q(obj + 0x178).unwrap_or(0),
            field_50: obs.q(obj + 0x50).unwrap_or(0),
            field_60,
            field_70: obs.q(obj + 0x70).unwrap_or(0),
            field_1f0_hex: obs.hex(obj + 0x1F0, 0x20),
            field_270: obs.q(obj + 0x270).unwrap_or(0),
            transport,
        }),
    }
}

// —— TLS 自校准与线程枚举 ——

/// 本进程 TlsAlloc+魔数,经 RPM 读自身 TEB 验证布局规则。
/// 返回日志;验证结果应用于目标线程(规则常量,布局属 OS 而非进程)。
fn calibrate_tls_layout(_self_handle: windows_sys::Win32::Foundation::HANDLE) -> (TlsLayout, Vec<String>) {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TlsAlloc, TlsFree, TlsSetValue};
    let mut log = Vec::new();
    let mut layout = TlsLayout::default();
    // SAFETY: 校准只动本进程 TLS;index 用后即释放;TEB 扫描为只读。
    unsafe {
        let idx = TlsAlloc();
        if idx == u32::MAX {
            log.push("TlsAlloc 失败:TLS 读取规则未校准(线程枚举将只回退经典规则)".into());
            return (layout, log);
        }
        if TlsSetValue(idx, CAL_MAGIC as *mut _) == 0 {
            log.push("TlsSetValue 失败".into());
            TlsFree(idx);
            return (layout, log);
        }
        let teb: usize;
        core::arch::asm!("mov {}, gs:[0x30]", out(reg) teb, options(nostack, nomem));
        let h = GetCurrentProcess();
        log.push(format!("校准: 本进程 TEB={teb:#x} idx={idx} magic={CAL_MAGIC:#x}"));
        // 全 TEB 首段扫描魔数:不预设教科书偏移(本机 OS 构建实证)。
        const SCAN: usize = 0x2000;
        let mut buf = vec![0u8; SCAN];
        let mut got = 0usize;
        let ok = windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            h,
            teb as *const core::ffi::c_void,
            buf.as_mut_ptr().cast(),
            SCAN,
            &mut got,
        );
        if ok != 0 {
            buf.truncate(got);
            let needle = CAL_MAGIC.to_le_bytes();
            let mut offs = Vec::new();
            for i in 0..buf.len().saturating_sub(8) {
                if buf[i..i + 8] == needle {
                    offs.push(i);
                }
            }
            for off in &offs {
                log.push(format!("校准: 魔数命中 TEB+{off:#x}"));
                if idx < 64 && *off == 0xE10 + 8 * idx as usize {
                    layout.inline_base = Some(0xE10);
                    log.push("校准: 经典内联布局(TlsSlots@0xE10)命中".into());
                } else if idx < 64 && *off >= 8 * idx as usize && (*off - 8 * idx as usize) % 8 == 0 {
                    let base_off = *off - 8 * idx as usize;
                    layout.inline_base = Some(base_off as u32);
                    log.push(format!("校准: 非经典内联布局 TlsSlots@{base_off:#x}"));
                }
            }
            // idx>=64:魔数在扩展数组里;数组指针本身也是 TEB 首段内的一个 qword。
            if idx >= 64 && !offs.is_empty() {
                for w in (0..got.saturating_sub(8)).step_by(8) {
                    let p = u64::from_le_bytes(buf[w..w + 8].try_into().unwrap());
                    if p > 0x10000 && p < 0xFFFF_8000_0000_0000 {
                        for o in &offs {
                            let rel = (*o as u64).wrapping_sub(p);
                            if rel % 8 == 0 && rel / 8 == idx as u64 - 64 {
                                layout.expansion_arr_off = Some(w as u32);
                                log.push(format!("校准: 扩展数组指针在 TEB+{w:#x}(槽=数组+8*(idx-64))"));
                            }
                        }
                    }
                }
            }
            if layout.inline_base.is_none() && layout.expansion_arr_off.is_none() {
                log.push("校准: 魔数不在 TEB 首段(布局未知;线程枚举仅回退经典规则)".into());
            }
        } else {
            log.push("校准: 自身 TEB 读取失败".into());
        }
        TlsSetValue(idx, std::ptr::null_mut());
        TlsFree(idx);
    }
    (layout, log)
}

/// 校准得出的 TLS 布局(偏移为本机 OS 构建实证,不预设)。
#[derive(Default, Clone, Copy, Debug)]
struct TlsLayout {
    /// 内联 TlsSlots 数组在 TEB 内的基址偏移(idx<64)。
    inline_base: Option<u32>,
    /// 指向扩展槽数组的指针在 TEB 内的偏移(idx>=64)。
    expansion_arr_off: Option<u32>,
}

/// 枚举目标进程全部线程;按候选规则读取 executor TLS 槽。
/// 命中判定 = 值非空且 [值] 的 vptr 落在已知模块(对象头特征),并把全部
/// 候选如实记录 —— 不把未命中规则伪装成事实。
fn enumerate_thread_tls(
    obs: &Observer,
    pid: u32,
    idx: u64,
    layout: TlsLayout,
) -> Vec<ThreadReport> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, THREAD_QUERY_INFORMATION};
    let mut out = Vec::new();
    // SAFETY: 快照/句柄只读;句柄退出前关闭。
    unsafe {
        let nqit = load_ntqueryinformationthread();
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut te: THREADENTRY32 = std::mem::zeroed();
        te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        if Thread32First(snap, &mut te) != 0 {
            loop {
                if te.th32OwnerProcessID == pid {
                    let mut rep = ThreadReport {
                        tid: te.th32ThreadID,
                        teb: None,
                        slot_rule: None,
                        pair: None,
                        dispatcher_obj: None,
                        vptr: None,
                        vptr_rva: None,
                        vptr_module: None,
                        slots: Vec::new(),
                        ctrl: None,
                        ctrl_strong: None,
                        ctrl_weak: None,
                    };
                    if let Some(nqit) = nqit {
                        let th = OpenThread(THREAD_QUERY_INFORMATION, 0, te.th32ThreadID);
                        if !th.is_null() {
                            let h = th;
                            let mut info = [0u8; 48];
                            let mut ret = 0u32;
                            if nqit(h, 0, info.as_mut_ptr().cast(), 48, &mut ret) == 0 {
                                let teb = u64::from_le_bytes(info[8..16].try_into().unwrap());
                                if teb != 0 {
                                    rep.teb = Some(teb);
                                    read_slot_into(obs, &mut rep, teb, idx, layout);
                                }
                            }
                            CloseHandle(h);
                        }
                    }
                    out.push(rep);
                }
                if Thread32Next(snap, &mut te) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

type NtQueryInformationThreadFn = unsafe extern "system" fn(
    windows_sys::Win32::Foundation::HANDLE,
    u32,
    *mut core::ffi::c_void,
    u32,
    *mut u32,
) -> i32;

fn load_ntqueryinformationthread() -> Option<NtQueryInformationThreadFn> {
    // SAFETY: ntdll 必然已加载;GetProcAddress 返回函数指针。
    unsafe {
        let h = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
            crate::winutil::to_wide("ntdll.dll").as_ptr(),
        );
        if h.is_null() {
            return None;
        }
        let name = std::ffi::CString::new("NtQueryInformationThread").unwrap();
        let p = windows_sys::Win32::System::LibraryLoader::GetProcAddress(h, name.as_ptr() as *const u8);
        p.map(|p| std::mem::transmute::<usize, NtQueryInformationThreadFn>(p as usize))
    }
}

/// 按候选规则读取线程 TLS 槽;命中 = 值非空且 [值] 的 vptr 落在已知模块。
fn read_slot_into(obs: &Observer, rep: &mut ThreadReport, teb: u64, idx: u64, layout: TlsLayout) {
    let mut candidates: Vec<(&'static str, u64)> = Vec::new();
    // 校准规则优先;经典偏移作为带标签的回退候选(命中判定统一)。
    if idx < 64 {
        if let Some(base_off) = layout.inline_base {
            if let Some(v) = obs.q(teb + base_off as u64 + 8 * idx) {
                candidates.push(("inline(校准)", v));
            }
        }
        if let Some(v) = obs.q(teb + 0xE10 + 8 * idx) {
            candidates.push(("inline(TEB+0xE10 经典)", v));
        }
    } else {
        if let Some(arr_off) = layout.expansion_arr_off {
            if let Some(arr) = obs.q(teb + arr_off as u64).filter(|a| *a > 0x10000) {
                if let Some(v) = obs.q(arr + 8 * (idx - 64)) {
                    candidates.push(("expansion(校准)", v));
                }
            }
        }
        for (rule, arr_off) in [
            ("expansion(TEB+0x58 经典)", 0x58u64),
            ("expansion(TEB+0x48 经典)", 0x48u64),
        ] {
            if let Some(arr) = obs.q(teb + arr_off).filter(|a| *a > 0x10000) {
                if let Some(v) = obs.q(arr + 8 * (idx - 64)) {
                    candidates.push((rule, v));
                }
            }
        }
    }
    for (rule, pair) in candidates {
        // D32E5A 的槽布局:{对象, 控制块} pair 存储地址。
        let Some(obj) = obs.q(pair).filter(|v| *v != 0) else { continue };
        let Some(vptr) = obs.q(obj).filter(|v| *v != 0) else { continue };
        if obs.module_of(vptr).is_none() {
            continue; // vptr 不在任何已知模块:不像对象头
        }
        rep.slot_rule = Some(rule.into());
        rep.pair = Some(pair);
        rep.dispatcher_obj = Some(obj);
        rep.vptr = Some(vptr);
        rep.vptr_rva = vptr.checked_sub(obs.base);
        rep.vptr_module = obs.module_of(vptr).map(|m| m.to_string());
        rep.slots = obs.vtable(vptr, 8);
        let ctrl = obs.q(pair + 8).filter(|v| *v != 0);
        if let Some(c) = ctrl {
            rep.ctrl_strong = obs.i32(c + 8);
            rep.ctrl_weak = obs.i32(c + 0x10);
        }
        rep.ctrl = ctrl;
        return;
    }
}

/// SAFETY: 只读跨进程读取;句柄须具备 VM_READ。
unsafe fn rpm<T>(handle: windows_sys::Win32::Foundation::HANDLE, addr: u64) -> Option<T> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    let mut out = std::mem::MaybeUninit::<T>::uninit();
    let mut got = 0usize;
    // SAFETY: 缓冲由调用方持有;只读。
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            addr as *const core::ffi::c_void,
            out.as_mut_ptr().cast(),
            std::mem::size_of::<T>(),
            &mut got,
        )
    };
    if ok == 0 || got != std::mem::size_of::<T>() {
        return None;
    }
    {
        // SAFETY: 已完整写入。
        Some(unsafe { out.assume_init() })
    }
}

fn target_created_unix_ms(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    // SAFETY: 只读查询;句柄具备 QUERY_INFORMATION。
    unsafe {
        let mut created = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut exited = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut kernel = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut user = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        if windows_sys::Win32::System::Threading::GetProcessTimes(
            handle, &mut created, &mut exited, &mut kernel, &mut user,
        ) == 0
        {
            return None;
        }
        let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        Some((ticks / 10_000).wrapping_sub(11_644_473_600_000))
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
