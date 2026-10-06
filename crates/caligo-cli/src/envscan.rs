//! 外部只读内存扫描(K2-02 WU2):在目标进程可读区域中定位指向
//! `qqnt_base + vtable_rva` 的指针(vfptr),得到候选 `node::Environment` 对象。
//!
//! 与路线文档 §10 的差异说明(有意为之,更安全):原路线写的是"obs 内存扫描"
//! (进程内 bridge);本实现改为**外部 ReadProcessMemory**——零注入、零加载,
//! 坏读只会让 RPM 调用失败而绝不触发目标进程 AV,是 checked_read 语义的严格
//! 超集。若 RPM 被拒绝(如受保护进程),回退到进程内 obs 扫描(未实现,记录)。
//!
//! 安全边界:OpenProcess 仅申请 QUERY_INFORMATION | VM_READ;不写、不创建
//! 远程线程;命中只读上下文字节作证据,不调用目标进程任何代码。

use serde::Serialize;

/// 一个扫描目标:vtable RVA + 该 vtable 在完整对象内的偏移(RTTI COL 提供)。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScanTarget {
    pub vtable_rva: u32,
    pub member_offset: u32,
}

/// 一次命中:vfptr 所在 VA 与据此推出的候选对象地址。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnvHit {
    /// vfptr 所在地址(指向 vtable 的指针的地址)。
    pub va: u64,
    pub vtable_rva: u32,
    pub member_offset: u32,
    /// 候选完整对象地址 = va - member_offset。
    pub candidate_env: u64,
    /// 区域类型:private / mapped / image / unknown。
    pub region_type: String,
    /// mbi.Type 原始值(与 region_type 对应,证据不翻译)。
    pub region_type_raw: u32,
    pub protect: u32,
    /// 命中所属模块短名(不在已知模块内为 None)。
    pub in_module: Option<String>,
    /// 候选对象起始处的上下文字节数(0 = RPM 失败)。
    pub context_len: usize,
    pub context_hex: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct EnvScanReport {
    pub pid: u32,
    pub qqnt_base: u64,
    pub targets: Vec<ScanTarget>,
    pub hits: Vec<EnvHit>,
    pub regions_scanned: u32,
    pub bytes_scanned: u64,
    /// 按区域类型统计的扫描字节数(private/image/mapped),证据不翻译。
    pub bytes_by_type: std::collections::BTreeMap<String, u64>,
    /// RPM 失败被跳过的区域计数(如实呈现,不掩盖)。
    pub regions_read_failed: u32,
    pub notes: Vec<String>,
}

struct ModuleInfo {
    name: String,
    base: usize,
    size: u32,
}

const MEM_COMMIT: u32 = 0x1000;
const MEM_PRIVATE: u32 = 0x2_0000;
const MEM_MAPPED: u32 = 0x4_0000;
const MEM_IMAGE: u32 = 0x100_0000;
const PAGE_NOACCESS: u32 = 0x01;
const PAGE_GUARD: u32 = 0x100;

fn region_type_name(t: u32) -> &'static str {
    match t {
        MEM_PRIVATE => "private",
        MEM_MAPPED => "mapped",
        MEM_IMAGE => "image",
        _ => "unknown",
    }
}

/// 朴素 8 字节模式搜索(首字节过滤;needle 长度恒为 8)。
/// 返回全部出现下标;调用方负责跨块重叠。
fn find_qwords(buf: &[u8], needle: &[u8; 8]) -> Vec<usize> {
    let mut out = Vec::new();
    if buf.len() < 8 {
        return out;
    }
    let mut from = 0;
    while from <= buf.len() - 8 {
        let Some(rel) = buf[from..].iter().position(|&b| b == needle[0]) else {
            break;
        };
        let i = from + rel;
        if i > buf.len() - 8 {
            break;
        }
        if &buf[i + 1..i + 8] == &needle[1..] {
            out.push(i);
        }
        from = i + 1;
    }
    out
}

/// 读取目标进程内存的上下文转储(失败返回空)。
unsafe fn read_context(
    handle: windows_sys::Win32::Foundation::HANDLE,
    addr: u64,
    len: usize,
) -> Vec<u8> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    let mut buf = vec![0u8; len];
    let mut got: usize = 0;
    // SAFETY: 只读转储;失败返回空,不影响主流程。
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            addr as *const core::ffi::c_void,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut got,
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    buf.truncate(got);
    buf
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// 扫描目标进程的地址空间。`targets` 的 vtable RVA 相对 QQNT.dll 基址;
/// `context_bytes` 为每个命中转储的字节数(候选对象起始)。
///
/// # Safety
///
/// 目标须为执行者已记录的观察对象;本函数只读,但仍应遵守 test-scope 的
/// 实例指定纪律。
pub unsafe fn scan_process(
    pid: u32,
    targets: &[ScanTarget],
    context_bytes: usize,
    include_mapped: bool,
) -> Result<EnvScanReport, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    // SAFETY: 只读权限打开;句柄在出口关闭。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        let err = unsafe { GetLastError() };
        return Err(format!("OpenProcess(pid={pid}) failed: Win32 error {err}"));
    }

    let mut report = EnvScanReport {
        pid,
        ..Default::default()
    };
    report.targets = targets.to_vec();

    // 目标进程模块快照(在本进程解析,不触碰目标加载器锁)。
    let modules: Vec<ModuleInfo> = match super::winutil::modules_in(pid) {
        Ok(list) => list
            .into_iter()
            .map(|m| ModuleInfo {
                name: m.name,
                base: m.base,
                size: m.size,
            })
            .collect(),
        Err(e) => {
            report
                .notes
                .push(format!("module snapshot failed: {e}; 命中将无模块归属"));
            Vec::new()
        }
    };
    let qqnt_base = modules
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("QQNT.dll"))
        .map(|m| m.base as u64)
        .unwrap_or(0);
    report.qqnt_base = qqnt_base;
    if qqnt_base == 0 {
        report
            .notes
            .push("QQNT.dll not found in target module snapshot".to_string());
    }

    // 目标指针模式 = qqnt_base + vtable_rva。
    let needles: Vec<[u8; 8]> = targets
        .iter()
        .map(|t| (qqnt_base + t.vtable_rva as u64).to_le_bytes())
        .collect();

    const CHUNK: usize = 4 * 1024 * 1024;
    let mut buf = vec![0u8; CHUNK + 7]; // 前 7 字节留给跨块重叠。
    let mut addr: u64 = 0x1_0000;
    const ADDR_LIMIT: u64 = 0x7FFF_FFFF_FFFF;

    while addr < ADDR_LIMIT {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: VirtualQueryEx 只读查询。
        let n = unsafe {
            VirtualQueryEx(
                handle,
                addr as *const core::ffi::c_void,
                &mut mbi,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if n == 0 {
            break;
        }
        let region_base = mbi.BaseAddress as u64;
        let region_size = mbi.RegionSize as u64;
        let next = region_base
            .checked_add(region_size)
            .filter(|v| *v > addr)
            .unwrap_or(ADDR_LIMIT);

        let scannable = mbi.State == MEM_COMMIT
            && (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)) == 0
            && (include_mapped || mbi.Type != MEM_MAPPED)
            && qqnt_base != 0;
        if scannable {
            report.regions_scanned += 1;
            let region_type = region_type_name(mbi.Type).to_string();
            let protect = mbi.Protect;
            let mut offset: u64 = 0;
            let mut carried: usize = 0; // 块前重叠字节数(0 或 7)。
            while offset < region_size {
                let want = ((region_size - offset) as usize).min(CHUNK);
                let read_at = region_base + offset;
                if carried == 7 {
                    buf.copy_within(CHUNK..CHUNK + 7, 0);
                }
                let mut got: usize = 0;
                // SAFETY: 只读;写入缓冲为 buf[carried..carried+want]。
                let ok = unsafe {
                    ReadProcessMemory(
                        handle,
                        read_at as *const core::ffi::c_void,
                        buf[carried..].as_mut_ptr().cast(),
                        want,
                        &mut got,
                    )
                };
                if ok == 0 {
                    report.regions_read_failed += 1;
                    break; // 区域保护均匀;失败放弃本区域(计数如实呈现)。
                }
                let search_len = carried + got;
                // 第一块从 0 起;后续块带 7 字节重叠,命中下标须 >= carried 避免重复。
                for (ti, needle) in needles.iter().enumerate() {
                    for pos in find_qwords(&buf[..search_len], needle) {
                        if pos < carried {
                            continue;
                        }
                        let hit_va = region_base + offset - carried as u64 + pos as u64;
                        let target = &targets[ti];
                        let candidate = hit_va.saturating_sub(target.member_offset as u64);
                        let in_module = modules
                            .iter()
                            .find(|m| {
                                hit_va >= m.base as u64
                                    && hit_va < (m.base + m.size as usize) as u64
                            })
                            .map(|m| m.name.clone());
                        let context =
                            unsafe { read_context(handle, candidate, context_bytes.min(4096)) };
                        let context_hex = to_hex(&context);
                        report.hits.push(EnvHit {
                            va: hit_va,
                            vtable_rva: target.vtable_rva,
                            member_offset: target.member_offset,
                            candidate_env: candidate,
                            region_type: region_type.clone(),
                            region_type_raw: mbi.Type,
                            protect,
                            in_module,
                            context_len: context.len(),
                            context_hex,
                        });
                    }
                }
                report.bytes_scanned += got as u64;
                *report
                    .bytes_by_type
                    .entry(region_type.clone())
                    .or_insert(0) += got as u64;
                carried = if got == want { 7 } else { 0 };
                offset += want as u64;
            }
        }
        addr = next;
    }

    // SAFETY: 句柄已用毕。
    unsafe { CloseHandle(handle) };
    // 命中按地址排序去重(同 VA 同针只记一次)。
    report
        .hits
        .sort_by(|a, b| (a.va, a.vtable_rva).cmp(&(b.va, b.vtable_rva)));
    report
        .hits
        .dedup_by(|a, b| a.va == b.va && a.vtable_rva == b.vtable_rva);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_qwords_positions() {
        let needle = 0x1122_3344_5566_7788u64.to_le_bytes();
        let mut buf = vec![0u8; 100];
        buf[10..18].copy_from_slice(&needle);
        buf[90..98].copy_from_slice(&needle);
        assert_eq!(find_qwords(&buf, &needle), vec![10, 90]);
        // 未对齐命中也要找到(vfptr 天然 8 对齐,但数据垃圾未必)。
        buf[33..41].copy_from_slice(&needle);
        assert_eq!(find_qwords(&buf, &needle), vec![10, 33, 90]);
        assert_eq!(find_qwords(&[0u8; 4], &needle), Vec::<usize>::new());
    }

    #[test]
    fn scan_target_serialization() {
        let t = ScanTarget {
            vtable_rva: 0x1234,
            member_offset: 0,
        };
        let back: serde_json::Value = serde_json::from_str(&serde_json::to_string(&t).unwrap())
            .unwrap();
        assert_eq!(back["vtable_rva"], 0x1234);
        assert_eq!(back["member_offset"], 0);
    }
}
