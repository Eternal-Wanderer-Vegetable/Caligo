//! Caligo 进程内 bridge:K1 最小加载/握手 probe。
//!
//! 安全边界(计划 §5/§6,K1 阶段):
//! - 本 crate 是项目内允许集中 `unsafe` 的位置之一;K1 阶段**不调用任何 QQ
//!   私有入口、不安装任何钩挂**,只报告自身加载状态与宿主进程模块快照。
//! - `DllMain` 不做实质工作:不创建线程、不等待、不获取 loader lock 之外的
//!   资源;实际探测由自有 loader 通过导出函数 [`caligo_probe_run`] 显式触发。
//! - 探测只读:枚举宿主模块、读自身路径,不写宿主内存、不读取消息数据。
//! - 报告写入由 loader 指定的路径完成;该写入发生在 loader 的远程线程上下文,
//!   不在 QQ 敏感回调内(计划 §3.1:回调内不做 IO 的约束不适用于本阶段,
//!   因为本阶段尚无任何 QQ 回调)。
//!
//! 版本拒绝行为:见 `docs/contracts/version-adapter-manifest.json`;
//! `caligo_bridge_protocol_version` 供 loader 与 manifest/核对握手使用。

use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

/// bridge C ABI 版本。导出函数集合或签名变化时递增。
pub const BRIDGE_ABI_VERSION: u32 = 1;
/// bridge 侧 IPC 协议版本,须与 `caligo-core::ipc::PROTOCOL_VERSION` 一致
/// (由 caligo-cli 的测试交叉断言)。
pub const PROTOCOL_VERSION: u32 = 1;
pub const BRIDGE_BUILD: &str = concat!("caligo-bridge ", env!("CARGO_PKG_VERSION"));

/// `DllMain` 在 DLL_PROCESS_ATTACH 时存入的自身模块句柄,
/// 用于 `GetModuleFileNameW` 取自身路径。
static OWN_INSTANCE: AtomicUsize = AtomicUsize::new(0);

// DLL_PROCESS_ATTACH(未引入 SystemServices feature,常量按 Win32 SDK 固定值)
const DLL_PROCESS_ATTACH: u32 = 1;

/// 探测结果码。
pub mod probe_code {
    /// 报告写入成功。
    pub const OK: u32 = 0;
    /// 报告路径指针无效(空)。
    pub const ERR_NULL_PATH: u32 = 1;
    /// 报告路径不是合法 UTF-16(到 NUL 为止无法构造)。
    pub const ERR_BAD_PATH: u32 = 2;
    /// 报告写入失败(路径不可写/磁盘错误)。
    pub const ERR_WRITE_FAILED: u32 = 3;
}

pub mod envrun;
pub mod intr;
pub mod obs;
pub mod register;

/// 注册入口结果码。
#[no_mangle]
/// 注册自有 linked binding(EXP-K2-00;详见 register.rs 与方案文档)。
/// 无参导出,由 loader 远程调用。返回 register_code 结果码。
pub extern "system" fn caligo_register_entry() -> u32 {
    register::register_entry()
}

#[no_mangle]
/// 本 DLL 实例是否已执行注册。
pub extern "system" fn caligo_entry_registered() -> u32 {
    u32::from(register::entry_state().0)
}

#[no_mangle]
/// 回调是否已被 Node 调用(0/1)。
pub extern "system" fn caligo_entry_fired() -> u32 {
    u32::from(register::entry_state().1)
}

#[no_mangle]
/// 启动自建 Environment 链路线程(envrun;EXP-K2-01)。立即返回(线程异步执行),
/// 结果以 JSONL 报告文件呈现(每阶段一行,崩溃亦保留已完成阶段)。
///
/// # Safety
///
/// `report_path` 必须指向有效的 NUL 结尾 UTF-16 缓冲区(由 loader 写入)。
pub unsafe extern "system" fn caligo_env_start(report_path: *const u16) -> u32 {
    let len = match wide_string_len(report_path) {
        Some(len) => len,
        None => return probe_code::ERR_NULL_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(unsafe { std::slice::from_raw_parts(report_path, len) })
    {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };
    envrun::env_start(&path_str)
}

/// 探测报告。K1 阶段字段只覆盖"已加载、可握手"这一层证据;
/// 真实账号与会话信息必须等入口契约确认后由确认入口提供,不在此伪造。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeReport {
    pub protocol_version: u32,
    pub bridge_abi_version: u32,
    pub bridge_build: String,
    /// bridge 自身在本进程内的加载路径。
    pub bridge_path: String,
    /// 宿主进程 PID。
    pub host_pid: u32,
    /// 宿主进程加载的 Tencent 目录模块与 *.node 模块快照(路径)。
    pub module_snapshot: Vec<String>,
    /// 快照采集是否完整(false = Toolhelp 失败,列表不完整)。
    pub module_snapshot_complete: bool,
}

/// 纯函数:把探测报告序列化为 JSON(K1 握手载体)。单测覆盖。
pub fn build_report_json(report: &ProbeReport) -> serde_json::Result<String> {
    serde_json::to_string_pretty(report)
}

fn wide_string_len(ptr: *const u16) -> Option<usize> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: 由 loader 约定传入 NUL 结尾的 UTF-16 缓冲区;最多扫描 32 KiB 防失控。
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 {
            if *ptr.add(len) == 0 {
                return Some(len);
            }
            len += 1;
        }
        None
    }
}

fn collect_host_snapshot() -> (Vec<String>, bool) {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    };
    // SAFETY: Toolhelp 快照为只读枚举;句柄在所有路径上都会被 CloseHandle 释放。
    unsafe {
        let handle = CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE,
            windows_sys::Win32::System::Threading::GetCurrentProcessId(),
        );
        if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return (Vec::new(), false);
        }
        let mut entries = Vec::new();
        let mut entry: MODULEENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        if Module32FirstW(handle, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExePath
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExePath.len());
                let path = String::from_utf16_lossy(&entry.szExePath[..len]);
                let lowered = path.to_ascii_lowercase();
                if lowered.contains("\\tencent\\") || lowered.ends_with(".node") {
                    entries.push(path);
                }
                if Module32NextW(handle, &mut entry) == 0 {
                    break;
                }
            }
        }
        windows_sys::Win32::Foundation::CloseHandle(handle);
        (entries, true)
    }
}

#[no_mangle]
/// 返回 bridge C ABI 版本,供 loader 在调用其他导出前核对。
pub extern "system" fn caligo_bridge_abi_version() -> u32 {
    BRIDGE_ABI_VERSION
}

#[no_mangle]
/// 返回 IPC 协议版本,供 loader 与 core 期望值核对(握手第一层)。
pub extern "system" fn caligo_bridge_protocol_version() -> u32 {
    PROTOCOL_VERSION
}

#[no_mangle]
/// 构建 probe 报告并写入 `report_path`(UTF-16,NUL 结尾;由 loader 提供)。
///
/// 返回 [`probe_code`] 中的结果码。只读宿主信息;不做其他副作用。
///
/// # Safety
///
/// `report_path` 必须指向有效的 NUL 结尾 UTF-16 缓冲区(由 loader 写入)。
pub unsafe extern "system" fn caligo_probe_run(report_path: *const u16) -> u32 {
    let len = match wide_string_len(report_path) {
        Some(len) => len,
        None => return probe_code::ERR_NULL_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(
        // SAFETY: 同上,读取范围 [0, len) 均在 loader 写入的缓冲区内。
        unsafe { std::slice::from_raw_parts(report_path, len) },
    ) {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };

    let own = OWN_INSTANCE.load(Ordering::Acquire);
    let bridge_path = if own != 0 {
        // SAFETY: own 是 DllMain 存入的本模块句柄;缓冲区由我们提供并按返回长度截断。
        unsafe {
            let mut buf = [0u16; 1024];
            let n = windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(
                own as _,
                buf.as_mut_ptr(),
                buf.len() as u32,
            ) as usize;
            if n == 0 || n >= buf.len() {
                String::new()
            } else {
                String::from_utf16_lossy(&buf[..n])
            }
        }
    } else {
        String::new()
    };

    let (module_snapshot, module_snapshot_complete) = collect_host_snapshot();
    let report = ProbeReport {
        protocol_version: PROTOCOL_VERSION,
        bridge_abi_version: BRIDGE_ABI_VERSION,
        bridge_build: BRIDGE_BUILD.to_string(),
        bridge_path,
        host_pid: current_pid(),
        module_snapshot,
        module_snapshot_complete,
    };
    match build_report_json(&report) {
        Ok(json) => match std::fs::write(&path_str, json) {
            Ok(()) => probe_code::OK,
            Err(_) => probe_code::ERR_WRITE_FAILED,
        },
        Err(_) => probe_code::ERR_WRITE_FAILED,
    }
}

fn current_pid() -> u32 {
    // SAFETY: GetCurrentProcessId 无副作用。
    unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() }
}

#[no_mangle]
/// 只读运行时观测(EXP-K1-02):解析 QQNT.dll 导出定位 node_module 注册链表头,
/// 遍历输出全部已注册原生模块的 JS 名字。详见 [`obs`] 模块文档。
///
/// 返回 [`probe_code`] 结果码。不调用任何 QQ/Node 函数;唯一写动作是报告文件。
///
/// # Safety
///
/// `report_path` 必须指向有效的 NUL 结尾 UTF-16 缓冲区(由 loader 写入)。
pub unsafe extern "system" fn caligo_obs_run(report_path: *const u16) -> u32 {
    let len = match wide_string_len(report_path) {
        Some(len) => len,
        None => return probe_code::ERR_NULL_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(unsafe { std::slice::from_raw_parts(report_path, len) })
    {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };
    let report = obs::observe();
    match obs::build_report_json(&report) {
        Ok(json) => match std::fs::write(&path_str, json) {
            Ok(()) => probe_code::OK,
            Err(_) => probe_code::ERR_WRITE_FAILED,
        },
        Err(_) => probe_code::ERR_WRITE_FAILED,
    }
}

#[no_mangle]
/// 只读运行时观测 v2(EXP-F1-R4 修复版):基址由加载器侧经 [`obs::ObsCtx`]
/// 传入,远程线程内**不调用任何加载器锁敏感 API**,只做裸内存读 + 写文件。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`obs::ObsCtx`](结构体布局见 obs.rs):
/// wrapper/qqnt/major 基址来自加载器侧 Toolhelp 快照,report_path 指向
/// 远程已写入的 NUL 结尾 UTF-16 缓冲。
pub unsafe extern "system" fn caligo_obs_run2(ctx: *const obs::ObsCtx) -> u32 {
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;字段按 obs::ObsCtx 布局解读。
    let ctx_view = unsafe { &*ctx };
    let report = obs::observe_ctx(ctx_view);
    let path_ptr = ctx_view.report_path;
    if path_ptr == 0 {
        return probe_code::ERR_NULL_PATH;
    }
    let len = match wide_string_len(path_ptr as *const u16) {
        Some(len) => len,
        None => return probe_code::ERR_BAD_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(unsafe {
        std::slice::from_raw_parts(path_ptr as *const u16, len)
    }) {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };
    match obs::build_report_json(&report) {
        Ok(json) => match std::fs::write(&path_str, json) {
            Ok(()) => probe_code::OK,
            Err(_) => probe_code::ERR_WRITE_FAILED,
        },
        Err(_) => probe_code::ERR_WRITE_FAILED,
    }
}

#[no_mangle]
/// K2-02 WU3:对候选 Environment 调用 `node::RequestInterrupt`(详见 intr.rs)。
/// 同步执行:解析导出 → 校验 env → 调用 → 轮询回调 → JSONL 报告。
///
/// 返回 [`intr::intr_code`] 结果码。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`intr::IntrCtx`](布局见 intr.rs):
/// env/expected_vftable/qqnt_base 来自加载器侧解析与 WU2 扫描结果,
/// report_path 指向远程已写入的 NUL 结尾 UTF-16 缓冲。
pub unsafe extern "system" fn caligo_interrupt_run(ctx: *mut intr::IntrCtx) -> u32 {
    if ctx.is_null() {
        return intr::intr_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;字段按 intr::IntrCtx 布局解读。
    let ctx_view = unsafe { &*ctx };
    intr::interrupt_run(ctx_view)
}

#[no_mangle]
/// 进程/线程入口。严格最小实现:仅记录自身句柄,不创建线程、不分配、不等待。
pub extern "system" fn DllMain(hinst: isize, reason: u32, _reserved: isize) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        OWN_INSTANCE.store(hinst as usize, Ordering::Release);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_json_contains_handshake_fields() {
        let report = ProbeReport {
            protocol_version: PROTOCOL_VERSION,
            bridge_abi_version: BRIDGE_ABI_VERSION,
            bridge_build: BRIDGE_BUILD.to_string(),
            bridge_path: "C:\\tmp\\caligo_bridge.dll".into(),
            host_pid: 4242,
            module_snapshot: vec!["C:\\t\\QQNT.dll".into()],
            module_snapshot_complete: true,
        };
        let json = build_report_json(&report).unwrap();
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["host_pid"], 4242);
        assert!(back["bridge_build"]
            .as_str()
            .unwrap()
            .starts_with("caligo-bridge"));
        assert_eq!(back["protocol_version"], 1);
        assert_eq!(back["module_snapshot_complete"], true);
        assert_eq!(back["bridge_abi_version"], BRIDGE_ABI_VERSION);
    }

    #[test]
    fn report_serialization_is_deterministic_for_same_input() {
        let report = ProbeReport {
            protocol_version: PROTOCOL_VERSION,
            bridge_abi_version: BRIDGE_ABI_VERSION,
            bridge_build: BRIDGE_BUILD.to_string(),
            bridge_path: "p".into(),
            host_pid: 1,
            module_snapshot: vec![],
            module_snapshot_complete: false,
        };
        assert_eq!(
            build_report_json(&report).unwrap(),
            build_report_json(&report).unwrap()
        );
    }
}
