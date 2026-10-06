//! K2-02 WU3:`node::RequestInterrupt` 实验(js-context-acquisition.md §10 R-A 第 3 步)。
//!
//! 目的:对内存扫描(WU2)得到的候选 `node::Environment*` 调用
//! `RequestInterrupt(env, callback, ctx)`(ord 2070),验证:
//! 1. 候选对象确实是一个活 Environment(调用不崩溃即最强证据);
//! 2. 回调是否在 QQ 的 JS 线程上执行(记录回调线程 id 与宿主线程对照);
//! 3. 从请求到触发时延(JS 线程忙时中断被推迟,时延反映其调度)。
//!
//! 纪律(计划 §3.1):
//! - 回调内部**只做原子写入**(tid/时刻/序号),零 IO、零分配、零锁;
//! - 调用前页校验候选对象(checked_read)+ vfptr 与扫描目标核对,不匹配
//!   **拒绝调用**(宁可放弃轮次,不拿实例赌博);
//! - 报告为 JSONL 增量落盘,中途崩溃保留已完成阶段;
//! - 远程线程内不调用加载器锁敏感 API:QQNT 基址由加载器经 ctx 传入,
//!   导出解析用 obs::export_addr(裸读导出表)。

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::envrun::append_stage;

/// 执行结果码(caligo_interrupt_run 的返回值)。
pub mod intr_code {
    /// 流程完成(回调是否触发以报告内 callback_fired 为准)。
    pub const OK: u32 = 0;
    /// ctx 或报告路径指针无效。
    pub const ERR_NULL_PATH: u32 = 1;
    /// 报告路径不是合法 UTF-16。
    pub const ERR_BAD_PATH: u32 = 2;
    /// QQNT.dll 基址无效(0 或 DOS 头不可读)。
    pub const ERR_NO_QQNT: u32 = 3;
    /// 候选 env 地址页不可读(拒绝调用)。
    pub const ERR_ENV_INVALID: u32 = 4;
    /// RequestInterrupt 导出缺失。
    pub const ERR_EXPORT_MISSING: u32 = 5;
    /// vfptr 与扫描目标不匹配(拒绝调用)。
    pub const ERR_VFTABLE_MISMATCH: u32 = 6;
}

/// 远程调用上下文(由加载器写进目标进程,#[repr(C)]:6 个字段)。
#[repr(C)]
pub struct IntrCtx {
    /// 候选 node::Environment*(0 = 干跑:只验证导出解析与报告链路,不调用)。
    pub env: usize,
    /// 期望的 vfptr 值(qqnt_base + vtable_rva;0 = 跳过该核对)。
    pub expected_vftable: usize,
    /// QQNT.dll 基址(加载器侧快照解析;远程线程内禁用 GetModuleHandleW,F1-R4)。
    pub qqnt_base: usize,
    /// 远程已写入的 NUL 结尾 UTF-16 报告路径缓冲地址。
    pub report_path: usize,
    /// 等待回调触发的时长(毫秒)。
    pub wait_ms: u32,
    pub _pad: u32,
}

// 回调侧状态:回调内只允许原子写,无锁无分配。
static FIRED: AtomicU32 = AtomicU32::new(0);
static CB_TID: AtomicU32 = AtomicU32::new(0);
static CB_TICK: AtomicU64 = AtomicU64::new(0);

/// 中断回调:在 QQ 的 JS 线程上执行。只做原子存储(计划 §3.1)。
extern "system" fn interrupt_cb(_ctx: *mut c_void) {
    // SAFETY: GetCurrentThreadId/GetTickCount64 无副作用、无锁。
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
}

fn reset_state() {
    FIRED.store(0, Ordering::Release);
    CB_TID.store(0, Ordering::Release);
    CB_TICK.store(0, Ordering::Release);
}

type FnRequestInterrupt =
    unsafe extern "C" fn(env: *mut c_void, callback: extern "system" fn(*mut c_void), ctx: *mut c_void);

/// 远程线程主体:校验 → 解析导出 → 调用 RequestInterrupt → 轮询 → 报告。
///
/// # Safety
///
/// ctx 必须指向本进程内由加载器写入的有效 [`IntrCtx`];report_path 须指向
/// NUL 结尾 UTF-16 缓冲。env 必须来自同进程内存扫描结果。
pub unsafe fn interrupt_run(ctx: &IntrCtx) -> u32 {
    reset_state();
    let path_ptr = ctx.report_path;
    if path_ptr == 0 {
        return intr_code::ERR_NULL_PATH;
    }
    let report = match read_wide(path_ptr) {
        Some(s) => s,
        None => return intr_code::ERR_BAD_PATH,
    };

    append_stage(&report, "start", true, &format!("env={:#x} expect_vftable={:#x} wait_ms={}", ctx.env, ctx.expected_vftable, ctx.wait_ms));

    // 干跑:只验证报告链路,不解析不调用(env=0 由执行者显式给出)。
    if ctx.env == 0 {
        append_stage(&report, "dry_run", true, "no env provided; plumbing only");
        append_stage(&report, "done", true, "dry run complete");
        return intr_code::OK;
    }

    // QQNT 基址(ctx 传入,远程线程不调加载器 API——F1-R4 纪律)。
    if ctx.qqnt_base == 0 {
        append_stage(&report, "qqnt", false, "qqnt_base not provided");
        return intr_code::ERR_NO_QQNT;
    }
    if !page_readable_span(ctx.env, 0x40) {
        append_stage(&report, "validate_env", false, "candidate env pages unreadable");
        return intr_code::ERR_ENV_INVALID;
    }
    let Some(vfptr) = read_usize(ctx.env) else {
        append_stage(&report, "validate_env", false, "vfptr read failed");
        return intr_code::ERR_ENV_INVALID;
    };
    append_stage(&report, "validate_env", true, &format!("vfptr={vfptr:#x}"));
    if ctx.expected_vftable != 0 && vfptr != ctx.expected_vftable {
        append_stage(
            &report,
            "validate_env",
            false,
            &format!("vfptr {vfptr:#x} != expected {:#x}; refusing to call", ctx.expected_vftable),
        );
        return intr_code::ERR_VFTABLE_MISMATCH;
    }

    // 解析 RequestInterrupt(裸读导出表,远程线程安全)。
    let Some(request_interrupt) = export_addr_or_note(&report, ctx.qqnt_base) else {
        return intr_code::ERR_EXPORT_MISSING;
    };
    let request_interrupt: FnRequestInterrupt = std::mem::transmute::<usize, FnRequestInterrupt>(request_interrupt);

    // 调用。此刻起 QQ 的 JS 线程可能在任意时刻进入回调。
    let started = std::time::Instant::now();
    append_stage(&report, "request", true, "calling RequestInterrupt");
    // SAFETY: env 已页校验且 vfptr 与扫描目标核对;callback 为纯原子写函数。
    unsafe { request_interrupt(ctx.env as *mut c_void, interrupt_cb, std::ptr::null_mut()) };

    // 轮询(10ms 步进;不忙等)。
    let deadline = started + std::time::Duration::from_millis(ctx.wait_ms.max(1) as u64);
    while FIRED.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let fired = FIRED.load(Ordering::Acquire);
    let tid = CB_TID.load(Ordering::Acquire);
    let tick = CB_TICK.load(Ordering::Acquire);
    let latency_ms = started.elapsed().as_millis() as u64;
    append_stage(
        &report,
        "callback_fired",
        fired != 0,
        &format!("fired={fired} cb_thread_id={tid} cb_tick={tick} latency_ms={latency_ms}"),
    );
    // 触发后再确认一轮稳定性窗口(仅读静态,无其他动作)。
    if fired != 0 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let still_ok = page_readable_span(ctx.env, 0x40);
        append_stage(&report, "post_check", still_ok, &format!("env pages still readable={still_ok}"));
    }
    append_stage(&report, "done", true, "interrupt round complete");
    intr_code::OK
}

fn export_addr_or_note(report: &str, qqnt_base: usize) -> Option<usize> {
    // SAFETY: 裸读导出表(obs::export_addr),无加载器锁 API。
    let addr = unsafe {
        crate::obs::export_addr(
            qqnt_base,
            "?RequestInterrupt@node@@YAXPEAVEnvironment@1@P6AXPEAX@Z1@Z",
        )
    };
    match addr {
        Some(a) => {
            append_stage(report, "resolve_interrupt", true, &format!("addr={a:#x} base={qqnt_base:#x}"));
            Some(a)
        }
        None => {
            append_stage(report, "resolve_interrupt", false, "export missing");
            None
        }
    }
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

// --- 页校验读(与 obs::checked_read 同语义;obs 侧为本模块可见性做了导出) ---

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
    // SAFETY: 页已校验;校验后瞬时卸载的窗口由报告如实呈现为读取失败。
    unsafe { Some((p as *const usize).read_unaligned()) }
}
