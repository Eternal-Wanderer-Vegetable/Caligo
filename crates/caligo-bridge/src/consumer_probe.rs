//! 响应消费者探针(N3 retCode 通道;research 构建)。
//!
//! 静态依据(2026-10-11 反汇编定案):
//! - 732A80 内部 0x732e2e 构造响应 dispatch 对象(73bbf8),内嵌 32B delegate
//!   (即我们传入的第 5 参 cb 块);
//! - 响应到达 → 73bd6c(无条件记 errCode/errStr 日志)→ 门 3fbab2(检查
//!   inner+0x10 非空)→ `(**(dispatch+0x30))(dispatch+0x18, 0x10, errCode,
//!   errStr, msgUid)`;
//! - dispatch+0x30 = cb[2](invoke)、dispatch+0x18 = cb 起点(ctx);
//!   errCode 提取层级:transport 参数 → resp 空 = -1(超时;配置 180s)→
//!   resp+0x40 → resp+0x60 → resp+0x64 → 0;msgUid = string@resp+0x38。
//!
//! 实验:把 cb 从 QQ 空函数 delegate 换成我们的 {ctx, flag, invoke, dtor} —
//! 若门开启,响应(或 180s 超时 -1)直接写入我们的 JSONL。
//! 防御:invoke/dtor 全部经 RPM-self 安全读(参数布局误判也不崩);
//! 本侧零 QQ 调用、零分配、不抛不绕(回调纪律与 g2 捕获同)。

use core::ffi::c_void;

/// 消费者上下文(进程生存期 leaked;QQ 线程回调只读)。
#[repr(C)]
pub struct ConsumerCtx {
    /// 报告路径(UTF-16,NUL 结尾;定长足够)。
    pub report: [u16; 180],
    /// 已记录条数(诊断)。
    pub hits: u64,
    /// 魔数(自检)。
    pub magic: u64,
}

const CTX_MAGIC: u64 = 0xC0A1_5E09_C0DE;

/// 从报告路径构造 leaked ctx(每 probe 一次;进程退出回收)。
pub fn leak_ctx(report_path: *const u16) -> &'static mut ConsumerCtx {
    let mut ctx = ConsumerCtx {
        report: [0; 180],
        hits: 0,
        magic: CTX_MAGIC,
    };
    // SAFETY: loader 写入的 NUL 结尾 UTF-16;上限内拷贝。
    unsafe {
        let mut i = 0usize;
        while i < 179 && *report_path.add(i) != 0 {
            ctx.report[i] = *report_path.add(i);
            i += 1;
        }
    }
    Box::leak(Box::new(ctx))
}

/// RPM-self 安全读(g2/g3 同款内核校验读;坏指针返回 None 不崩)。
fn safe_read_qword(addr: usize) -> Option<u64> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut out = 0u64;
    let mut got = 0usize;
    // SAFETY: 自进程 RPM;栈缓冲。
    unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const c_void,
            (&mut out as *mut u64).cast(),
            8,
            &mut got,
        )
    };
    if got == 8 { Some(out) } else { None }
}

/// libc++ string 读取(短形态 24B 内联 / 长形态 data 指针;只读不持)。
/// 地址不可读 → None。
fn read_cpp_string(ptr: usize) -> Option<String> {
    if ptr < 0x10000 {
        return None;
    }
    let mut raw = [0u64; 3];
    for (i, slot) in raw.iter_mut().enumerate() {
        *slot = safe_read_qword(ptr + i * 8)?;
    }
    let q0 = raw[0].to_le_bytes();
    let (size, data_ptr) = if q0[0] & 1 == 0 {
        ((q0[0] >> 1) as usize, ptr)
    } else {
        (raw[1] as usize, raw[2] as usize)
    };
    if size == 0 || size > 512 || data_ptr < 0x10000 {
        return None;
    }
    let nq = (size + 7) / 8;
    let mut buf = Vec::with_capacity(nq * 8);
    for i in 0..nq {
        let q = safe_read_qword(data_ptr + i * 8)?;
        buf.extend_from_slice(&q.to_le_bytes());
    }
    Some(String::from_utf8_lossy(&buf[..size]).into_owned())
}

/// JSONL 追加(回调内唯一写动作;失败静默——回调纪律)。
fn append_line(ctx: &ConsumerCtx, line: &str) {
    use std::os::windows::ffi::OsStringExt;
    let end = ctx.report.iter().position(|c| *c == 0).unwrap_or(180);
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &ctx.report[..end],
    ));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
        let _ = f.write_all(b"\n");
    }
}

/// 消费者回调(被 QQ dispatcher 线程调用;契约 = (inner, 0x10, errCode,
/// errStr*, msgUid*)。防御式:全部字段 RPM-self 读,布局误判不崩。
///
/// # Safety
/// QQ 线程调用;只读;不得调用 QQ API、不得 unwind。
pub unsafe extern "system" fn consumer_invoke(
    inner: *mut c_void,
    tag: u32,
    err_code: i32,
    err_str: *const c_void,
    msg_uid: *const c_void,
) {
    // SAFETY: 回调纪律 — 全程 RPM-self 安全读 + JSONL 追加,无 QQ 调用。
    unsafe {
        let inner_addr = inner as usize;
        let ctx: &ConsumerCtx = resolve_ctx(inner_addr);
        let err_str = read_cpp_string(err_str as usize)
            .unwrap_or_else(|| format!("<unreadable {:#x}>", err_str as usize));
        let msg_uid = read_cpp_string(msg_uid as usize)
            .unwrap_or_else(|| format!("<unreadable {:#x}>", msg_uid as usize));
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let line = serde_json::json!({
            "kind": "consumer_result",
            "ts": ts,
            "inner": format!("{inner_addr:#x}"),
            "tag": tag,
            "err_code": err_code,
            "err_str": err_str,
            "msg_uid": msg_uid,
        });
        append_line(ctx, &line.to_string());
    }
}

/// 从 invoke 首参恢复 ctx:接受直接指针或 delegate 对象(其 +0/+8 可能持
/// ctx 指针);魔数自检失败则用进程级 fallback(最后一线程仍写诊断行)。
unsafe fn resolve_ctx(inner_addr: usize) -> &'static ConsumerCtx {
    unsafe {
        // 直接指针?
        if inner_addr > 0x10000 {
            if let Some(m) = safe_read_qword(inner_addr + 16) {
                if m == CTX_MAGIC {
                    return &*(inner_addr as *const ConsumerCtx);
                }
            }
        }
        // delegate 对象:+0/+8 持 ctx 指针?
        for off in [0usize, 8] {
            if inner_addr > 0x10000 {
                if let Some(p) = safe_read_qword(inner_addr + off) {
                    let p = p as usize;
                    if p > 0x10000 {
                        if let Some(m) = safe_read_qword(p + 16) {
                            if m == CTX_MAGIC {
                                return &*(p as *const ConsumerCtx);
                            }
                        }
                    }
                }
            }
        }
        fallback_ctx()
    }
}

static FALLBACK: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// 进程级 fallback ctx(报告路径 = 消费者探针缺省;注册于 leak_ctx 时)。
unsafe fn fallback_ctx() -> &'static ConsumerCtx {
    let p = *FALLBACK.get_or_init(|| {
        leak_ctx_fallback() as *const ConsumerCtx as usize
    });
    &*(p as *const ConsumerCtx)
}

static FALLBACK_PATH: std::sync::Mutex<Option<Vec<u16>>> = std::sync::Mutex::new(None);

/// 登记 fallback 路径(注册 ctx 时调用)。
pub fn set_fallback(report_path: *const u16) {
    let mut v = Vec::new();
    // SAFETY: loader 写入的 NUL 结尾 UTF-16。
    unsafe {
        let mut i = 0usize;
        while i < 179 && *report_path.add(i) != 0 {
            v.push(*report_path.add(i));
            i += 1;
        }
    }
    *FALLBACK_PATH.lock().unwrap() = Some(v);
}

fn leak_ctx_fallback() -> &'static ConsumerCtx {
    let report = FALLBACK_PATH
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| Vec::new());
    let mut ctx = ConsumerCtx {
        report: [0; 180],
        hits: 0,
        magic: CTX_MAGIC,
    };
    for (i, c) in report.iter().take(179).enumerate() {
        ctx.report[i] = *c;
    }
    Box::leak(Box::new(ctx))
}

/// delegate 析构(QQ 释放 dispatch 对象时调用;no-op —— ctx 为 leaked,
/// 零释放动作)。
///
/// # Safety
/// QQ 线程调用;不得 unwind。
pub unsafe extern "system" fn consumer_dtor(_ctx: *mut c_void) {}

/// cb 块构造:{ctx, flag, invoke, dtor}。
pub fn build_cb(ctx: &ConsumerCtx) -> [u64; 4] {
    [
        ctx as *const ConsumerCtx as usize as u64,
        1,
        consumer_invoke as usize as u64,
        consumer_dtor as usize as u64,
    ]
}
