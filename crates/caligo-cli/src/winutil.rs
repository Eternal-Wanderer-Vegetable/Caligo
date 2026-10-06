//! Win32 进程/模块枚举与受门控的加载器(K1 最小实现)。
//!
//! 纪律(对应计划 §5/§8):
//! - 枚举只读:Toolhelp 快照 + GetProcessTimes,不触碰目标进程内存。
//! - 加载器是**本仓库自有的实现**(无第三方 loader、无授权服务、无文件修补);
//!   它把 bridge cdylib 用 `LoadLibraryW` 装入执行者明确指定的测试实例,
//!   然后调用 bridge 的 `caligo_probe_run` 取回探测报告。
//! - 门控在 CLI 层完成:manifest 摘要核对通过 + 执行者显式确认参数,两者缺一不可。
//! - 本模块的 unsafe 注入序列在 K1 尚未经真实验证;首次使用必须按
//!   docs/research/recovery-notes.md §2 的恢复路径执行。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{FILETIME, HANDLE};

pub struct ProcessInfo {
    pub pid: u32,
    pub exe_path: String,
    /// 进程创建时间(UTC,取不到时为 None)。
    pub started_utc: Option<String>,
    pub modules: Vec<String>,
}

pub fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn filetime_to_utc(ft: &FILETIME) -> Option<String> {
    let raw = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    if raw == 0 {
        return None;
    }
    // 1601-01-01 → 1970-01-01 的 100ns 区间数。
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    if raw < EPOCH_DIFF_100NS {
        return None;
    }
    let unix_secs = ((raw - EPOCH_DIFF_100NS) / 10_000_000) as i64;
    let days = unix_secs.div_euclid(86_400);
    let rem = unix_secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z"))
}

fn u16sz(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 枚举名字等于 `image_name`(不区分大小写)的进程及其模块路径。只读。
pub fn list_processes(image_name: &str) -> Result<Vec<ProcessInfo>, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
        MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let want = image_name.to_ascii_lowercase();

    // SAFETY: Toolhelp 快照是只读枚举;每个句柄都在对应块内关闭。
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err("CreateToolhelp32Snapshot(PROCESS) failed".into());
        }
        let mut pids: Vec<(u32, String)> = Vec::new();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let exe = u16sz(&entry.szExeFile);
                if exe.eq_ignore_ascii_case(&want) {
                    pids.push((entry.th32ProcessID, exe));
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);

        let mut out = Vec::new();
        for (pid, exe_path) in pids {
            // 创建时间:查询受限信息权即可,不申请写权限。
            let mut started_utc = None;
            let proc_h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !proc_h.is_null() {
                let mut created: FILETIME = std::mem::zeroed();
                let mut exit_t: FILETIME = std::mem::zeroed();
                let mut kernel_t: FILETIME = std::mem::zeroed();
                let mut user_t: FILETIME = std::mem::zeroed();
                if GetProcessTimes(
                    proc_h,
                    &mut created,
                    &mut exit_t,
                    &mut kernel_t,
                    &mut user_t,
                ) != 0
                {
                    started_utc = filetime_to_utc(&created);
                }
                CloseHandle(proc_h);
            }

            let mut modules = Vec::new();
            let msnap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
            if msnap != INVALID_HANDLE_VALUE {
                let mut ment: MODULEENTRY32W = std::mem::zeroed();
                ment.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
                if Module32FirstW(msnap, &mut ment) != 0 {
                    loop {
                        modules.push(u16sz(&ment.szExePath));
                        if Module32NextW(msnap, &mut ment) == 0 {
                            break;
                        }
                    }
                }
                CloseHandle(msnap);
            }
            out.push(ProcessInfo {
                pid,
                exe_path,
                started_utc,
                modules,
            });
        }
        Ok(out)
    }
}

pub struct InjectOutcome {
    /// bridge 在目标进程内的基址。
    pub remote_base: usize,
    /// `caligo_probe_run` 的返回码(见 caligo-bridge::probe_code)。
    pub probe_exit_code: u32,
    /// `caligo_obs_run` 的返回码(仅在 obs_report 提供时)。
    pub obs_exit_code: Option<u32>,
    /// `caligo_register_entry` 的返回码(仅在 register_entry 提供时)。
    pub register_exit_code: Option<u32>,
    /// `caligo_env_start` 的返回码(仅在 env_report 提供时;链路线程异步执行)。
    pub env_exit_code: Option<u32>,
}

type RemoteThreadFn = unsafe extern "system" fn(*mut core::ffi::c_void) -> u32;

/// 把 bridge 装入指定进程并调用其 `caligo_probe_run`(可选:`caligo_obs_run`)。
///
/// 步骤(每步失败即中止,不给"部分成功"):
/// 1. OpenProcess(创建远程线程 + 虚拟内存读写 + 查询权限);
/// 2. VirtualAllocEx + WriteProcessMemory 写入 bridge 路径(UTF-16);
/// 3. CreateRemoteThread 调 kernel32!LoadLibraryW;
/// 4. 在目标模块快照中定位 bridge 基址(x64 上线程退出码会截断,不可靠),
///    用 ReadProcessMemory 解析其远程 PE 导出表,找到 `caligo_probe_run`;
/// 5. 分配报告路径缓冲,CreateRemoteThread 调用 probe;
/// 6. `obs_report` 给出时,再以同样方式调用 `caligo_obs_run`(只读运行时观测);
/// 7. 释放远端内存与句柄。
///
/// 门控(manifest 摘要核对、执行者显式确认)在 CLI 入口完成,不在此重复。
///
/// # Safety
///
/// 仅可对执行者明确指定的测试实例调用;调用方负责目标选择的全部授权。
pub unsafe fn inject_and_probe(
    pid: u32,
    bridge_path: &Path,
    report_path: &Path,
    obs_report: Option<&Path>,
    env_report: Option<&Path>,
    register_entry: bool,
    wait_ms: u32,
) -> Result<InjectOutcome, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, INFINITE,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };

    if !bridge_path.is_file() {
        return Err(format!("bridge not found: {}", bridge_path.display()));
    }
    let bridge_wide = to_wide(&bridge_path.to_string_lossy());
    let report_wide = to_wide(&report_path.to_string_lossy());

    // SAFETY: 文档化的远程加载序列;门控在 CLI 入口完成。
    // 两块远端缓冲用 Option 跟踪,收尾统一释放;句柄在函数出口关闭。
    unsafe {
        let proc_h = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if proc_h.is_null() {
            return Err(format!("OpenProcess: Win32 error {}", GetLastError()));
        }
        let mut buf_bridge: Option<*mut core::ffi::c_void> = None;
        let mut buf_report: Option<*mut core::ffi::c_void> = None;
        let mut buf_obs: Option<*mut core::ffi::c_void> = None;
        let mut buf_env: Option<*mut core::ffi::c_void> = None;

        let result = (|| -> Result<InjectOutcome, String> {
            let alloc_and_write = |bytes: &[u16],
                                   step: &str|
             -> Result<*mut core::ffi::c_void, String> {
                let size = bytes.len() * 2;
                let remote = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    size,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote.is_null() {
                    return Err(format!("{step}: Win32 error {}", GetLastError()));
                }
                let mut written: usize = 0;
                let ok =
                    WriteProcessMemory(proc_h, remote, bytes.as_ptr().cast(), size, &mut written);
                if ok == 0 || written != size {
                    VirtualFreeEx(proc_h, remote, 0, MEM_RELEASE);
                    return Err(format!(
                        "{step}: WriteProcessMemory incomplete ({written}/{size})"
                    ));
                }
                Ok(remote)
            };

            let remote_bridge_path = alloc_and_write(&bridge_wide, "alloc bridge path")?;
            buf_bridge = Some(remote_bridge_path);

            let kernel32 = GetModuleHandleW(to_wide("kernel32.dll").as_ptr());
            let load_library_addr: Option<unsafe extern "system" fn(*const u16) -> isize> =
                if kernel32.is_null() {
                    None
                } else {
                    GetProcAddress(kernel32, c"LoadLibraryW".as_ptr() as *const u8).map(|f| {
                        std::mem::transmute::<
                            usize,
                            unsafe extern "system" fn(*const u16) -> isize,
                        >(f as usize)
                    })
                };
            let Some(load_library_addr) = load_library_addr else {
                return Err("kernel32!LoadLibraryW not resolved".into());
            };

            let start_load: RemoteThreadFn =
                std::mem::transmute::<usize, RemoteThreadFn>(load_library_addr as usize);
            let thread = CreateRemoteThread(
                proc_h,
                std::ptr::null(),
                0,
                Some(start_load),
                remote_bridge_path,
                0,
                std::ptr::null_mut(),
            );
            if thread.is_null() {
                return Err(format!(
                    "CreateRemoteThread(LoadLibraryW): Win32 error {}",
                    GetLastError()
                ));
            }
            let wait = WaitForSingleObject(thread, if wait_ms > 0 { wait_ms } else { INFINITE });
            CloseHandle(thread);
            if wait != 0 {
                // WAIT_OBJECT_0 == 0;超时/放弃都按失败处理(不假设加载完成)。
                return Err(format!("remote LoadLibraryW wait failed: code {wait:#x}"));
            }

            // 在目标模块快照中定位 bridge 基址(x64 线程退出码截断,不可靠)。
            let remote_base = {
                let msnap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
                if msnap == INVALID_HANDLE_VALUE {
                    return Err("CreateToolhelp32Snapshot(MODULE) failed after load".into());
                }
                let mut found: Option<usize> = None;
                let mut ment: MODULEENTRY32W = std::mem::zeroed();
                ment.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
                if Module32FirstW(msnap, &mut ment) != 0 {
                    loop {
                        let loaded = u16sz(&ment.szExePath);
                        if loaded.eq_ignore_ascii_case(&bridge_path.to_string_lossy()) {
                            found = Some(ment.modBaseAddr as usize);
                            break;
                        }
                        if Module32NextW(msnap, &mut ment) == 0 {
                            break;
                        }
                    }
                }
                CloseHandle(msnap);
                found.ok_or_else(|| {
                    "bridge module not found in target snapshot after load".to_string()
                })?
            };

            // 可选:注册自有 linked binding(caligo_register_entry,无参调用)。
            let mut register_exit: Option<u32> = None;
            if register_entry {
                let reg_addr =
                    match read_remote_export(proc_h, remote_base, "caligo_register_entry") {
                        Ok(Some(a)) => a,
                        Ok(None) => return Err("caligo_register_entry not found in remote".into()),
                        Err(e) => return Err(format!("remote export lookup (register): {e}")),
                    };
                let reg_fn: unsafe extern "system" fn() -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn() -> u32>(reg_addr);
                let start_reg: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(reg_fn as usize);
                let reg_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_reg),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                );
                if reg_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(register): Win32 error {}",
                        GetLastError()
                    ));
                }
                let wait =
                    WaitForSingleObject(reg_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(reg_thread, &mut code);
                CloseHandle(reg_thread);
                if wait != 0 {
                    return Err(format!(
                        "register remote thread wait failed: code {wait:#x}"
                    ));
                }
                register_exit = Some(code);
            }

            // 解析远程 PE 导出表,找 caligo_probe_run。
            let probe_addr = match read_remote_export(proc_h, remote_base, "caligo_probe_run") {
                Ok(Some(a)) => a,
                Ok(None) => return Err("caligo_probe_run not found in remote export table".into()),
                Err(e) => return Err(format!("remote export lookup: {e}")),
            };

            let remote_report_path = alloc_and_write(&report_wide, "alloc report path")?;
            buf_report = Some(remote_report_path);

            let probe_fn: unsafe extern "system" fn(*const u16) -> u32 = std::mem::transmute::<
                usize,
                unsafe extern "system" fn(*const u16) -> u32,
            >(probe_addr);
            let start_probe: RemoteThreadFn =
                std::mem::transmute::<usize, RemoteThreadFn>(probe_fn as usize);
            let probe_thread = CreateRemoteThread(
                proc_h,
                std::ptr::null(),
                0,
                Some(start_probe),
                remote_report_path,
                0,
                std::ptr::null_mut(),
            );
            if probe_thread.is_null() {
                return Err(format!(
                    "CreateRemoteThread(probe): Win32 error {}",
                    GetLastError()
                ));
            }
            let wait =
                WaitForSingleObject(probe_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
            let mut exit_code: u32 = u32::MAX;
            GetExitCodeThread(probe_thread, &mut exit_code);
            CloseHandle(probe_thread);
            if wait != 0 {
                return Err(format!("probe remote thread wait failed: code {wait:#x}"));
            }

            // 可选:只读运行时观测(caligo_obs_run)。
            let mut obs_exit: Option<u32> = None;
            if let Some(obs_path) = obs_report {
                let obs_wide = to_wide(&obs_path.to_string_lossy());
                let obs_addr = match read_remote_export(proc_h, remote_base, "caligo_obs_run") {
                    Ok(Some(a)) => a,
                    Ok(None) => {
                        return Err("caligo_obs_run not found in remote export table".into())
                    }
                    Err(e) => return Err(format!("remote export lookup (obs): {e}")),
                };
                let remote_obs_path = alloc_and_write(&obs_wide, "alloc obs report path")?;
                buf_obs = Some(remote_obs_path);
                let obs_fn: unsafe extern "system" fn(*const u16) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const u16) -> u32>(
                        obs_addr,
                    );
                let start_obs: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(obs_fn as usize);
                let obs_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_obs),
                    remote_obs_path,
                    0,
                    std::ptr::null_mut(),
                );
                if obs_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(obs): Win32 error {}",
                        GetLastError()
                    ));
                }
                let wait =
                    WaitForSingleObject(obs_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(obs_thread, &mut code);
                CloseHandle(obs_thread);
                if wait != 0 {
                    return Err(format!("obs remote thread wait failed: code {wait:#x}"));
                }
                obs_exit = Some(code);
            }

            // 可选:自建 Environment 链路(caligo_env_start;执行线程异步运行,
            // 阶段结果增量写入 env 报告文件——中途崩溃亦保留已完成阶段)。
            let mut env_exit: Option<u32> = None;
            if let Some(env_path) = env_report {
                let env_wide = to_wide(&env_path.to_string_lossy());
                let env_addr = match read_remote_export(proc_h, remote_base, "caligo_env_start") {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_env_start not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (env): {e}")),
                };
                let remote_env_path = alloc_and_write(&env_wide, "alloc env report path")?;
                buf_env = Some(remote_env_path);
                let env_fn: unsafe extern "system" fn(*const u16) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const u16) -> u32>(
                        env_addr,
                    );
                let start_env: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(env_fn as usize);
                let env_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_env),
                    remote_env_path,
                    0,
                    std::ptr::null_mut(),
                );
                if env_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(env): Win32 error {}",
                        GetLastError()
                    ));
                }
                let wait =
                    WaitForSingleObject(env_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(env_thread, &mut code);
                CloseHandle(env_thread);
                if wait != 0 {
                    return Err(format!("env remote thread wait failed: code {wait:#x}"));
                }
                env_exit = Some(code);
            }

            Ok(InjectOutcome {
                remote_base,
                probe_exit_code: exit_code,
                obs_exit_code: obs_exit,
                register_exit_code: register_exit,
                env_exit_code: env_exit,
            })
        })();

        if let Some(p) = buf_bridge {
            VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
        }
        if let Some(p) = buf_report {
            VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
        }
        if let Some(p) = buf_obs {
            VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
        }
        if let Some(p) = buf_env {
            VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
        }
        CloseHandle(proc_h);
        result
    }
}

/// 读远程进程内存并解析 PE 导出表,返回指定导出函数的绝对地址。
///
/// # Safety
///
/// `handle` 须具备 PROCESS_VM_READ,`base` 须是该进程内已加载映像基址。
/// 全部读取量有上界(头部 4 KiB;导出名 256 B)。
unsafe fn read_remote_export(
    handle: HANDLE,
    base: usize,
    want: &str,
) -> Result<Option<usize>, String> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;

    let read = |addr: usize, buf: &mut [u8]| -> Result<(), String> {
        let mut got: usize = 0;
        let ok = ReadProcessMemory(
            handle,
            addr as *const core::ffi::c_void,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut got,
        );
        if ok == 0 || got != buf.len() {
            return Err(format!("ReadProcessMemory failed at {addr:#x}"));
        }
        Ok(())
    };

    let mut dos = [0u8; 0x40];
    read(base, &mut dos)?;
    if &dos[0..2] != b"MZ" {
        return Err("remote image lacks MZ".into());
    }
    let e_lfanew = u32::from_le_bytes([dos[0x3C], dos[0x3D], dos[0x3E], dos[0x3F]]) as usize;

    // PE 头 + 段表整体读取(位于映像头页内,映射存在)。
    let mut hdr = [0u8; 4096];
    read(base + e_lfanew, &mut hdr)?;
    if &hdr[0..4] != b"PE\0\0" {
        return Err("remote PE signature missing".into());
    }
    let number_of_sections = u16::from_le_bytes([hdr[6], hdr[7]]) as usize;
    let size_of_optional = u16::from_le_bytes([hdr[20], hdr[21]]) as usize;
    let opt = 4 + 20;
    let magic = u16::from_le_bytes([hdr[opt], hdr[opt + 1]]);
    let data_dir_offset = match magic {
        0x20B => opt + 112, // PE32+
        0x10B => opt + 96,  // PE32
        other => return Err(format!("remote optional header magic 0x{other:04X}")),
    };
    if data_dir_offset + 8 > hdr.len() {
        return Err("remote header read too small for data directories".into());
    }
    let export_rva = u32::from_le_bytes(
        hdr[data_dir_offset..data_dir_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let export_size = u32::from_le_bytes(
        hdr[data_dir_offset + 4..data_dir_offset + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    if export_rva == 0 || export_size == 0 || export_size > 16 * 1024 * 1024 {
        return Err("remote export directory absent or implausible".into());
    }

    // 段表范围检查(RVA 是否落在映像映射范围内)。
    let sec_base = opt + size_of_optional;
    let mut sections: Vec<(usize, usize)> = Vec::with_capacity(number_of_sections);
    for i in 0..number_of_sections {
        let off = sec_base + i * 40;
        if off + 40 > hdr.len() {
            return Err("remote section table beyond header read".into());
        }
        let vsize = u32::from_le_bytes(hdr[off + 8..off + 12].try_into().unwrap()) as usize;
        let va = u32::from_le_bytes(hdr[off + 12..off + 16].try_into().unwrap()) as usize;
        sections.push((va, va + vsize));
    }
    let rva_in_image = |rva: usize| {
        sections
            .iter()
            .any(|(start, end)| rva >= *start && rva < *end)
    };

    if !rva_in_image(export_rva) {
        return Err(format!(
            "export dir rva {export_rva:#x} outside mapped sections"
        ));
    }

    let dir_remote = base + export_rva;
    let mut dir = [0u8; 40];
    read(dir_remote, &mut dir)?;
    let number_of_names = u32::from_le_bytes(dir[24..28].try_into().unwrap()) as usize;
    let addr_names = u32::from_le_bytes(dir[32..36].try_into().unwrap()) as usize;
    let addr_ordinals = u32::from_le_bytes(dir[36..40].try_into().unwrap()) as usize;
    let addr_functions = u32::from_le_bytes(dir[28..32].try_into().unwrap()) as usize;
    if number_of_names > 100_000 {
        return Err("implausible export name count".into());
    }

    for i in 0..number_of_names {
        let mut name_ptr = [0u8; 4];
        read(base + addr_names + i * 4, &mut name_ptr)?;
        let name_rva = u32::from_le_bytes(name_ptr) as usize;
        let mut ordinal = [0u8; 2];
        read(base + addr_ordinals + i * 2, &mut ordinal)?;
        let oi = u16::from_le_bytes(ordinal) as usize;

        let mut name = [0u8; 256];
        read(base + name_rva, &mut name)?;
        let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        if std::str::from_utf8(&name[..len])
            .map(|s| s == want)
            .unwrap_or(false)
        {
            let mut fn_rva = [0u8; 4];
            read(base + addr_functions + oi * 4, &mut fn_rva)?;
            let fn_rva = u32::from_le_bytes(fn_rva) as usize;
            // probe 是本 DLL 的真实导出,不会是转发器。
            return Ok(Some(base + fn_rva));
        }
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversion_known_value() {
        // 2026-01-01T00:00:00Z = 1_767_225_600 unix 秒。
        // FILETIME 基准用硬编码正确值,避免与实现共用同一常量形成循环验证。
        let raw: u64 = 134_116_992_000_000_000;
        let ft = FILETIME {
            dwLowDateTime: (raw & 0xFFFF_FFFF) as u32,
            dwHighDateTime: (raw >> 32) as u32,
        };
        assert_eq!(
            filetime_to_utc(&ft).as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
    }

    #[test]
    fn own_process_creation_time_is_plausible() {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, GetCurrentProcessId());
            assert!(!h.is_null(), "OpenProcess(self) failed");
            let mut created: FILETIME = std::mem::zeroed();
            let mut exit_t: FILETIME = std::mem::zeroed();
            let mut kernel_t: FILETIME = std::mem::zeroed();
            let mut user_t: FILETIME = std::mem::zeroed();
            let ok = GetProcessTimes(h, &mut created, &mut exit_t, &mut kernel_t, &mut user_t);
            CloseHandle(h);
            assert_eq!(ok, 1, "GetProcessTimes(self) failed");
            let started = filetime_to_utc(&created).expect("creation time parseable");
            let year: i64 = started[0..4].parse().unwrap();
            assert!(
                (2024..=2036).contains(&year),
                "implausible creation year {year} in {started}"
            );
        }
    }
}
