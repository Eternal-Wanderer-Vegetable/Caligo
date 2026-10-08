# -*- coding: utf-8 -*-
from pathlib import Path
p = Path(r'E:\stella\Caligo\crates\caligo-bridge\src\g2.rs')
t = p.read_text(encoding='utf-8')

# 1. replace fingerprint raw reads with safe RPM-self
old_fp = '''fn manager_fingerprint(m: usize, wrapper_base: usize) -> Option<(bool, u64)> {
    // SAFETY: 调用方保证 m 可读(来自堆扫持有指针)。
    unsafe {
        let flag = core::ptr::read_volatile((m + 8) as *const u64);
        if flag & 0xFF != 1 {
            return None;
        }
        let vptr = core::ptr::read_volatile(m as *const u64);
        if vptr < 0x10000 {
            return None;
        }
        let mut shim_slots = 0usize;
        for k in 0..5usize {
            let slot = core::ptr::read_volatile((vptr as usize + k * 8) as *const u64);
            let rva = (slot as usize).wrapping_sub(wrapper_base);
            if rva < 0x800_0000 {
                shim_slots += 1;
            }
        }
        let raw = vptr == (wrapper_base + MANAGER_VTBL_RVA) as u64;
        if !raw && shim_slots < 4 {
            return None;
        }
        let begin = core::ptr::read_volatile((m + FIELD_BEGIN_NODE) as *const u64);
        if begin == 0 {
            return None;
        }
        Some((shim_slots >= 4, vptr))
    }
}'''
new_fp = '''/// RPM-self 安全读:经内核侧校验,页面已释放/去提交时返回 None 而非异常。
/// (直接解引用与 QQ 堆释放存在竞态 —— 2026-10-09 实例损失根因,禁用。)
fn safe_read_qword(addr: usize) -> Option<u64> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut out = 0u64;
    let mut got = 0usize;
    // SAFETY: 自进程 RPM;缓冲在栈上。
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const c_void,
            (&mut out as *mut u64).cast(),
            8,
            &mut got,
        )
    };
    if ok == 0 || got != 8 {
        None
    } else {
        Some(out)
    }
}

/// RPM-self 区块读取(4MB 分块,坏块截断)。
fn read_region_chunked(base: usize, size: usize) -> Vec<u8> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    const CHUNK: usize = 4 * 1024 * 1024;
    let mut out = Vec::with_capacity(size);
    let mut off = 0usize;
    while off < size {
        let n = CHUNK.min(size - off);
        let begin = out.len();
        out.resize(begin + n, 0);
        let mut got = 0usize;
        // SAFETY: 自进程 RPM;缓冲由 out 持有。
        let ok = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                (base + off) as *const c_void,
                out[begin..].as_mut_ptr().cast(),
                n,
                &mut got,
            )
        };
        if ok == 0 || got != n {
            out.truncate(begin); // 坏块:截断,不臆测内容
            break;
        }
        off += n;
    }
    out
}

fn manager_fingerprint(m: usize, wrapper_base: usize) -> Option<(bool, u64)> {
    // 全部经 RPM-self 读取:对象可能在读取瞬间被 QQ 释放。
    let flag = safe_read_qword(m + 8)?;
    if flag & 0xFF != 1 {
        return None;
    }
    let vptr = safe_read_qword(m)?;
    if vptr < 0x10000 {
        return None;
    }
    let mut shim_slots = 0usize;
    for k in 0..5usize {
        let slot = safe_read_qword(vptr as usize + k * 8)?;
        let rva = (slot as usize).wrapping_sub(wrapper_base);
        if rva < 0x800_0000 {
            shim_slots += 1;
        }
    }
    let raw = vptr == (wrapper_base + MANAGER_VTBL_RVA) as u64;
    if !raw && shim_slots < 4 {
        return None;
    }
    let begin = safe_read_qword(m + FIELD_BEGIN_NODE)?;
    if begin == 0 {
        return None;
    }
    Some((shim_slots >= 4, vptr))
}'''
assert old_fp in t, 'fingerprint block not found'
t = t.replace(old_fp, new_fp)

# 2. replace slice-scan with chunked RPM scan
old_scan = '''            // SAFETY: 已提交可读私有内存的进程内切片直读(GB 级扫描;
            // read_volatile 逐 qword 版本实测超时)。区域前提 commit+可读。
            let words = unsafe { std::slice::from_raw_parts(base_addr as *const u64, region / 8) };
            if let Some(off) = words.iter().position(|&w| w == needle) {
                let p = base_addr + off * 8;
                let m = unsafe { ((p + SC_FIELD_MANAGER) as *const u64).read_volatile() as usize };
                if m != 0 && manager_fingerprint(m, wrapper_base).is_some() {
                    candidates += 1;
                    return Some((m, candidates));
                }
            }'''
new_scan = '''            let buf = read_region_chunked(base_addr, region);
            let mut i = 0usize;
            while i + 8 <= buf.len() {
                if u64::from_le_bytes(buf[i..i + 8].try_into().unwrap()) == needle {
                    let p = base_addr + i;
                    if let Some(m) = safe_read_qword(p + SC_FIELD_MANAGER) {
                        let m = m as usize;
                        if m != 0 && manager_fingerprint(m, wrapper_base).is_some() {
                            candidates += 1;
                            return Some((m, candidates));
                        }
                    }
                }
                i += 8;
            }'''
assert old_scan in t, 'scan block not found'
t = t.replace(old_scan, new_scan)

# 3. stage logs around scan
old_loc = '''    // 活 Manager 定位(SC+0x150 正路)。
    let Some((manager, sc_n)) = locate_manager(wrapper_base) else {'''
new_loc = '''    // 活 Manager 定位(SC+0x150 正路)。
    stage(report_path, "g2_scan", true, "SC scan begin (RPM-self)");
    let Some((manager, sc_n)) = locate_manager(wrapper_base) else {'''
assert old_loc in t, 'locate block not found'
t = t.replace(old_loc, new_loc)

old_ok = '''    let m_vptr = unsafe { core::ptr::read_volatile(manager as *const u64) };'''
new_ok = '''    stage(report_path, "g2_scan", true, "SC scan done");
    let m_vptr = safe_read_qword(manager).unwrap_or(0);'''
assert old_ok in t, 'vptr stage block not found'
t = t.replace(old_ok, new_ok)

# 4. tree snapshot + restore reads via safe_read_qword
old_tree = '''    let b0 = unsafe { (*begin_slot).load(Ordering::Acquire) };
    let root = unsafe { core::ptr::read_volatile((end_node as usize) as *const u64) };
    let size = unsafe { core::ptr::read_volatile((end_node as usize + 8) as *const u64) };'''
new_tree = '''    let b0 = safe_read_qword(manager + FIELD_BEGIN_NODE).unwrap_or(0);
    if b0 == 0 {
        stage(report_path, "g2_tree", false, "begin_node unreadable/zero (Manager freed?)");
        return ERR_NO_MANAGER;
    }
    let root = safe_read_qword(end_node as usize).unwrap_or(0);
    let size = safe_read_qword(end_node as usize + 8).unwrap_or(0);'''
assert old_tree in t, 'tree block not found'
t = t.replace(old_tree, new_tree)

old_restore = '''    let cur = unsafe { (*begin_slot).load(Ordering::Acquire) };'''
new_restore = '''    let cur = safe_read_qword(manager + FIELD_BEGIN_NODE).unwrap_or(0);'''
assert old_restore in t, 'restore block not found'
t = t.replace(old_restore, new_restore)

p.write_text(t, encoding='utf-8')
print('ALL REPLACEMENTS OK')
