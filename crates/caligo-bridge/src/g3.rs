//! G3 发送探测 P1-v3:transport **对象 vptr** 交换 + 零分配捕获(零发送)。
//!
//! 静态证据链(r4-g3 树 + thread-contract §10,详见
//! `docs/research/qq-native-send-probe-design.md` §1):
//! - MSFService(this)+0x60 = transport,对象头 vptr = 原版 vtable RVA
//!   `0x41403B8`;slot7(+0x38)= `0x1B4E4EC`,六参:
//!   `(transport, string& uin, unique_ptr<task>*, policy48&, i32* token_out, callback&)`;
//!   **全部 SSO 发送(心跳、推送 ACK、用户消息)都经此槽**。
//!
//! v3 与 r1(镜像 vtable 补丁)的差别(2026-10-09 事故后修订,见台账):
//! - r1 改 wrapper .rdata 映像页(COW)= 可检测篡改面,11s 后进程以
//!   伪装 C++ length_error 终止 → **镜像补丁路线弃用**;
//! - v3 只交换 transport **对象头**的 vptr(单个堆 qword,原子,仅该
//!   对象受影响):伪造 vtable = 自分配 RW 内存中 16 qword 原表副本,
//!   slot7 指向蹦床,其余槽 = 原函数。QQ 自身即有堆垫片 vtable 先例
//!   (G2 发现的 Manager 垫片),对象级 vptr 多样性是 QQ 原生模式;
//! - 捕获处理器 **零分配/零锁/零 format**:固定 96-qword 记录 × 64 槽,
//!   seqlock 语义([0] 最低位 = 完成位,Release 发布),编码在窗口结束后
//!   由我们线程统一做。
//!
//! 纪律(承 3 次实例损失教训):
//! - 蹦床页/伪 vtable 页 **永不释放**(恢复后迟到调用仍完整工作);
//! - 捕获处理器全部读取经 safe_read(RPM-self,坏页=None;硬件 AV 不可捕获,
//!   禁 read_volatile 直读);有界(uin ≤128B、候选 ≤4、环 64 条);
//! - 座位(对象 vptr)被第三方覆写则不回写(同 G2 座位丢失语义),如实记录。

use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

/// getter 机器级原型(72DE38;同 g1/g2,作 wrapper 身份核验 + 租约)。
type GetterFn = unsafe extern "system" fn(out: *mut ServicePair) -> *mut ServicePair;

const GETTER_RVA: usize = 0x72DE38;
const ANCHOR_SVC_VTBL_RVA: usize = 0x3F6DED8;
/// transport 原版 vtable RVA(现场三次采样,thread-contract §10.1)。
const TRANSPORT_VTBL_RVA: usize = 0x41403B8;
/// slot7 = vtable +0x38;原函数 RVA 0x1B4E4EC。
const SLOT7_OFF: usize = 0x38;
const SLOT7_ORIG_RVA: usize = 0x1B4E4EC;
/// transport+0x60 = conn pair(b7ce8a 的状态检查对象)。
const TRANSPORT_FIELD_CONN: usize = 0x60;
/// MSFService+0x60 字段(transport 指针)。
const SVC_FIELD_TRANSPORT: usize = 0x60;
/// vtable 拷贝槽数(前 10 槽为函数指针,其后为非指针数据,原样拷贝)。
const VTBL_COPY_QWORDS: usize = 16;

/// 任务对象 0x88 字节(0x72ca0d:alloc(0x88)+ctor)。
const TASK_BYTES: usize = 0x88;
/// 捕获环容量(条)。
const RING_CAP: usize = 64;
/// 候选解引用上限(每调用)。
const CAND_CAP: usize = 4;
/// 字符串捕获上限(字节)。
const STR_CAP: usize = 128;
/// 每槽 qword 数(记录布局见下)。
const SLOT_QWORDS: usize = 96;

// 记录布局(槽内偏移):
//  [0]       seq|1(完成位;0=未完成)
//  [2..8]    a1..a6
//  [8..11]   uin string 原始 24B
//  [11..27]  uin 数据 ≤128B
//  [27]      task_ptr(*task_slot)
//  [28..45]  task 0x88B
//  [45]      n_cands
//  [46..58]  候选 4×{off,ptr,first}
//  [58..74]  候选头 4×0x20B
//  [74..80]  policy 0x30B
//  [80..84]  callback 0x20B
const OFF_ARGS: usize = 2;
const OFF_UIN_RAW: usize = 8;
const OFF_UIN_DATA: usize = 11;
const OFF_TASK_PTR: usize = 27;
const OFF_TASK: usize = 28;
const OFF_NCAND: usize = 45;
const OFF_CANDS: usize = 46;
const OFF_HEADS: usize = 58;
const OFF_POLICY: usize = 74;
const OFF_CB: usize = 80;

pub const OK: u32 = 0;
pub const ERR_NO_OBSERVER: u32 = 21; // getter/锚点失败(沿用 v1 语义)
pub const ERR_SWAP_STATE: u32 = 22;
pub const ERR_NO_TRANSPORT: u32 = 31;

#[repr(C)]
pub struct G3Ctx {
    pub wrapper_base: usize,
    pub report_path: *const u16,
    pub observe_ms: u32,
}

/// 固定捕获环(回调侧零分配写入;窗口结束单次 drain)。
static mut RING_SLOTS: [u64; RING_CAP * SLOT_QWORDS] = [0; RING_CAP * SLOT_QWORDS];
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 蹦床页地址(交换期间写入伪 vtable;恢复后仍有效,页不释放)。
static TRAMPOLINE: AtomicU64 = AtomicU64::new(0);
/// 伪 vtable 页地址(交换期间写入对象头;恢复后仍有效,页不释放)。
static FAKE_VTBL: AtomicU64 = AtomicU64::new(0);
/// 原对象 vptr(= wrapper_base + 0x41403B8)。
static ORIGINAL_VPTR: AtomicU64 = AtomicU64::new(0);
/// 捕获激活闸(交换后置 1,恢复前置 0;0 时蹦床仍安全:直接进原函数)。
static ACTIVE: AtomicU64 = AtomicU64::new(0);

fn hx(v: u64) -> String {
    format!("{v:#x}")
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn bytes_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(HEX_DIGITS[(x >> 4) as usize] as char);
        s.push(HEX_DIGITS[(x & 0xF) as usize] as char);
    }
    s
}

/// RPM-self 安全读:坏页返回 None 而非异常(g2 同源实现)。
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

/// 逐 qword 读入固定缓冲(零分配)。返回成功读取的 qword 数。
fn read_qwords_into(addr: usize, out: &mut [u64]) -> usize {
    for (i, o) in out.iter_mut().enumerate() {
        match safe_read_qword(addr + i * 8) {
            Some(v) => *o = v,
            None => return i,
        }
    }
    out.len()
}

fn addr_plausible(v: u64) -> bool {
    (0x1_0000..0x7FFF_FFFE_FFFF).contains(&v) && v & 7 == 0
}

/// libc++ 同源 string 判形(纯函数,可测试)。
/// 短形态:byte0 = size<<1(≤22,内联 +1);长形态:{cap|1, size@8, data*@0x10}。
fn string_form(q0: u64) -> Option<(bool, usize)> {
    if q0 & 1 == 0 {
        let size = ((q0 & 0xFF) >> 1) as usize;
        if size > 22 {
            return None;
        }
        Some((true, size)) // short, size
    } else {
        Some((false, 0)) // long,size 由 +8 处读取
    }
}

/// 捕获处理器(蹦床调用;六参原样)。零分配、零锁、零 format。
///
/// SAFETY: 各参为 QQ 线程上 slot7 的实参;全部解引用经 safe_read。
unsafe extern "system" fn g3_capture(
    transport: usize,
    uin_str: usize,
    task_slot: usize,
    policy: usize,
    token_out: usize,
    callback: usize,
) {
    if ACTIVE.load(Ordering::Acquire) == 0 {
        return;
    }
    // SAFETY: 边界内只做固定缓冲写入 + safe_read;无 unwrap/分配/锁,
    // 无 panic 路径(catch_unwind 仅作纵深防御)。
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let idx64 = NEXT.fetch_add(1, Ordering::Relaxed);
        if idx64 as usize >= RING_CAP {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let seq = idx64;
        // SAFETY: idx < RING_CAP;单写者(原子取号);drain 在 ACTIVE=0 后。
        let slot = unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::addr_of_mut!(RING_SLOTS).cast::<u64>(),
                RING_CAP * SLOT_QWORDS,
            )
        };
        let s = &mut slot[idx64 as usize * SLOT_QWORDS..(idx64 as usize + 1) * SLOT_QWORDS];
        s[0] = 0; // 未完成标记(偶)
        s[1] = 0; // flags
        s[OFF_ARGS] = transport as u64;
        s[OFF_ARGS + 1] = uin_str as u64;
        s[OFF_ARGS + 2] = task_slot as u64;
        s[OFF_ARGS + 3] = policy as u64;
        s[OFF_ARGS + 4] = token_out as u64;
        s[OFF_ARGS + 5] = callback as u64;

        // uin string(双形态;数据 ≤128B)。
        if addr_plausible(uin_str as u64) {
            let mut raw = [0u64; 3];
            if read_qwords_into(uin_str, &mut raw) == 3 {
                s[OFF_UIN_RAW..OFF_UIN_RAW + 3].copy_from_slice(&raw);
                if let Some((short, size)) = string_form(raw[0]) {
                    if short {
                        let bytes = raw.map(|v| v.to_le_bytes());
                        let flat: [u8; 24] = {
                            let mut f = [0u8; 24];
                            let mut n = 0;
                            for b in bytes {
                                f[n..n + 8].copy_from_slice(&b);
                                n += 8;
                            }
                            f
                        };
                        if size <= 22 {
                            let mut data = [0u64; 16];
                            let mut fb = [0u8; 128];
                            fb[..24].copy_from_slice(&flat);
                            let mut n8 = 0;
                            while n8 * 8 < 128 && n8 < 16 {
                                let mut w = [0u8; 8];
                                w.copy_from_slice(&fb[n8 * 8..n8 * 8 + 8]);
                                data[n8] = u64::from_le_bytes(w);
                                n8 += 1;
                            }
                            s[OFF_UIN_DATA..OFF_UIN_DATA + 16].copy_from_slice(&data);
                            s[1] |= 1; // has uin(short)
                        }
                    } else {
                        let size = safe_read_qword(uin_str + 8).unwrap_or(0) as usize;
                        let data_ptr = safe_read_qword(uin_str + 0x10).unwrap_or(0);
                        s[OFF_UIN_RAW + 1] = size as u64;
                        s[OFF_UIN_RAW + 2] = data_ptr;
                        if addr_plausible(data_ptr) {
                            let take = size.min(STR_CAP);
                            let mut data = [0u64; 16];
                            let n = read_qwords_into(data_ptr as usize, &mut data[..(take + 7) / 8]);
                            s[OFF_UIN_DATA..OFF_UIN_DATA + n].copy_from_slice(&data[..n]);
                            s[1] |= 1 | 2; // has uin(long)
                        }
                    }
                }
            }
        }

        // task(0x88B)+ 候选追踪。
        if addr_plausible(task_slot as u64) {
            if let Some(tp) = safe_read_qword(task_slot) {
                s[OFF_TASK_PTR] = tp;
                if addr_plausible(tp) {
                    let mut task = [0u64; TASK_BYTES / 8];
                    let n = read_qwords_into(tp as usize, &mut task);
                    s[OFF_TASK..OFF_TASK + n].copy_from_slice(&task[..n]);
                    let mut ncand = 0usize;
                    for (i, q) in task.iter().take(n).enumerate() {
                        if ncand >= CAND_CAP {
                            break;
                        }
                        if !addr_plausible(*q) {
                            continue;
                        }
                        // 对象资格:首 qword(疑似 vptr)必须可读。
                        let Some(first) = safe_read_qword(*q as usize) else {
                            continue;
                        };
                        let base = OFF_CANDS + ncand * 3;
                        s[base] = (i * 8) as u64;
                        s[base + 1] = *q;
                        s[base + 2] = first;
                        let hbase = OFF_HEADS + ncand * 4;
                        let mut head = [0u64; 4];
                        let hn = read_qwords_into(*q as usize, &mut head);
                        s[hbase..hbase + hn].copy_from_slice(&head[..hn]);
                        ncand += 1;
                    }
                    s[OFF_NCAND] = ncand as u64;
                    s[1] |= 4; // has task
                }
            }
        }

        // policy 48B。
        if addr_plausible(policy as u64) {
            let mut p = [0u64; 6];
            let n = read_qwords_into(policy, &mut p);
            s[OFF_POLICY..OFF_POLICY + n].copy_from_slice(&p[..n]);
            if n == 6 {
                s[1] |= 8;
            }
        }
        // callback 32B。
        if addr_plausible(callback as u64) {
            let mut c = [0u64; 4];
            let n = read_qwords_into(callback, &mut c);
            s[OFF_CB..OFF_CB + n].copy_from_slice(&c[..n]);
            if n == 4 {
                s[1] |= 16;
            }
        }

        // 发布(完成位 + seq;Release 保证先前写入可见)。
        s[0] = ((seq as u64) << 1) | 1;
    }));
}

/// drain:固定环 → JSON 行(窗口结束后由我们线程调用,可分配)。
pub fn drain_records() -> (Vec<String>, u64, u64) {
    let mut out = Vec::new();
    // SAFETY: 唯一读者在 ACTIVE=0 后运行;写者已停。
    let slots = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(RING_SLOTS).cast::<u64>(),
            RING_CAP * SLOT_QWORDS,
        )
    };
    for idx in 0..RING_CAP {
        let s = &slots[idx * SLOT_QWORDS..(idx + 1) * SLOT_QWORDS];
        let head = s[0];
        if head & 1 == 0 {
            continue;
        }
        let seq = (head >> 1) as usize;
        let flags = s[1];
        let args: Vec<String> = s[OFF_ARGS..OFF_ARGS + 6].iter().map(|v| hx(*v)).collect();
        let uin = decode_uin(s);
        let task_ptr = s[OFF_TASK_PTR];
        let task_hex = bytes_hex(&qw_slice_bytes(&s[OFF_TASK..OFF_TASK + TASK_BYTES / 8]));
        let ncand = s[OFF_NCAND] as usize;
        let mut cands = String::new();
        for c in 0..ncand.min(CAND_CAP) {
            let off = s[OFF_CANDS + c * 3];
            let ptr = s[OFF_CANDS + c * 3 + 1];
            let first = s[OFF_CANDS + c * 3 + 2];
            let head_hex = bytes_hex(&qw_slice_bytes(&s[OFF_HEADS + c * 4..OFF_HEADS + c * 4 + 4]));
            if !cands.is_empty() {
                cands.push(',');
            }
            cands.push_str(&format!(
                "{{\"off\":{off:#x},\"ptr\":{},\"first\":{},\"head\":\"{head_hex}\"}}",
                hx(ptr),
                hx(first)
            ));
        }
        let policy_hex = bytes_hex(&qw_slice_bytes(&s[OFF_POLICY..OFF_POLICY + 6]));
        let cb_hex = bytes_hex(&qw_slice_bytes(&s[OFF_CB..OFF_CB + 4]));
        out.push(format!(
            "{{\"kind\":\"sso_call\",\"seq\":{seq},\"flags\":{flags},\"transport\":{},\
             \"uin_str\":{},\"uin\":{uin},\"task_slot\":{},\"task_ptr\":{},\"task\":\"{task_hex}\",\
             \"cands\":[{cands}],\"policy\":\"{policy_hex}\",\"cb\":\"{cb_hex}\",\
             \"token_out\":{}}}",
            args[0],
            args[1],
            args[2],
            hx(task_ptr),
            args[4],
        ));
    }
    let pushed = NEXT.load(Ordering::Relaxed);
    let dropped = DROPPED.load(Ordering::Relaxed);
    (out, pushed, dropped)
}

/// uin 字段解码为 JSON 字符串(hex + 尽力 UTF-8 文本)。
fn decode_uin(s: &[u64]) -> String {
    if s[1] & 1 == 0 {
        return "null".into();
    }
    let short = s[1] & 2 == 0;
    let (bytes, _n) = if short {
        (qw_slice_bytes(&s[OFF_UIN_RAW..OFF_UIN_RAW + 3])[1..23].to_vec(), 0usize)
    } else {
        (qw_slice_bytes(&s[OFF_UIN_DATA..OFF_UIN_DATA + 16]).to_vec(), 0usize)
    };
    let hexs = bytes_hex(&bytes);
    let text: String = String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .replace('\\', "\\\\")
        .replace('"', "'");
    format!("{{\"t\":\"{text}\",\"hex\":\"{hexs}\"}}")
}

fn qw_slice_bytes(q: &[u64]) -> Vec<u8> {
    let mut v = Vec::with_capacity(q.len() * 8);
    for w in q {
        v.extend_from_slice(&w.to_le_bytes());
    }
    v
}

/// 蹦床字节编码(可测试)。
///
/// 入口(MSVC x64):rcx=a1 rdx=a2 r8=a3 r9=a4;[rsp]=ret;
/// [rsp+8..0x28]=0x20 字节 shadow;**a5=[rsp+0x28],a6=[rsp+0x30]**;
/// rsp≡8 (mod 16)。保存四个寄存器参 → 对齐 → 以完整六参调 `capture`
/// → 恢复 → 尾跳 `original`(栈面与直调完全一致)。rax 为易失,可占用。
pub fn trampoline_bytes(capture: usize, original: usize) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x40);
    b.extend_from_slice(&[0x51]); // push rcx
    b.extend_from_slice(&[0x52]); // push rdx
    b.extend_from_slice(&[0x41, 0x50]); // push r8
    b.extend_from_slice(&[0x41, 0x51]); // push r9
    b.extend_from_slice(&[0x48, 0x83, 0xEC, 0x28]); // sub rsp,0x28(rsp=R-0x48)
    // a5: 入口 [R+0x28] = [rsp+0x70];a6: [R+0x30] = [rsp+0x78]。
    // capture 的栈参:call 前 [rsp+0x20]=a5,[rsp+0x28]=a6。
    b.extend_from_slice(&[0x48, 0x8B, 0x44, 0x24, 0x70]); // mov rax,[rsp+0x70]
    b.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x20]); // mov [rsp+0x20],rax
    b.extend_from_slice(&[0x48, 0x8B, 0x44, 0x24, 0x78]); // mov rax,[rsp+0x78]
    b.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x28]); // mov [rsp+0x28],rax
    b.extend_from_slice(&[0x48, 0xB8]); // movabs rax, capture
    b.extend_from_slice(&(capture as u64).to_le_bytes());
    b.extend_from_slice(&[0xFF, 0xD0]); // call rax
    b.extend_from_slice(&[0x48, 0x83, 0xC4, 0x28]); // add rsp,0x28
    b.extend_from_slice(&[0x41, 0x59]); // pop r9
    b.extend_from_slice(&[0x41, 0x58]); // pop r8
    b.extend_from_slice(&[0x5A]); // pop rdx
    b.extend_from_slice(&[0x59]); // pop rcx
    b.extend_from_slice(&[0xFF, 0x25, 0x00, 0x00, 0x00, 0x00]); // jmp qword [rip]
    b.extend_from_slice(&(original as u64).to_le_bytes());
    b
}

fn stage(path: *const u16, stage: &str, ok: bool, detail: &str) {
    // SAFETY: path 由 loader 写入,NUL 结尾 UTF-16。
    let text = unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        if len >= 32 * 1024 {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(path, len)))
        }
    };
    if let Some(p) = text {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        let line = format!(
            "{{\"ts\":{},\"tid\":{},\"stage\":\"{}\",\"ok\":{},\"detail\":\"{}\"}}\n",
            ts,
            tid,
            stage,
            ok,
            detail.replace('\\', "\\\\").replace('"', "'")
        );
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn report_path_str(path: *const u16) -> String {
    // SAFETY: loader 写入的 NUL 结尾 UTF-16。
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(path, len))
    }
}

/// 单 qword 交换(VirtualProtect → 原子 8B 写 → 恢复保护;堆页本已 RW,
/// protect 为无害往返)。可测试:slot 可为进程内任意可读写地址。
fn swap_slot(slot_addr: usize, new_val: u64) -> Result<u64, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Memory::VirtualProtect;
    let mut old_prot = 0u32;
    // SAFETY: 自进程;8B 窗口;old_prot 由系统填写。
    if unsafe { VirtualProtect(slot_addr as *const c_void, 8, 0x04 /*PAGE_READWRITE*/, &mut old_prot) } == 0 {
        return Err(format!("VirtualProtect(rw): Win32 {}", unsafe { GetLastError() }));
    }
    // SAFETY: 已确认可写;8B 对齐原子访问。
    let prev = unsafe { core::ptr::read_volatile(slot_addr as *const u64) };
    unsafe { core::ptr::write_volatile(slot_addr as *mut u64, new_val) };
    let mut restored = 0u32;
    // SAFETY: 恢复原保护。
    unsafe { VirtualProtect(slot_addr as *const c_void, 8, old_prot, &mut restored) };
    Ok(prev)
}

#[repr(C)]
struct ServicePair {
    obj: *mut c_void,
    ctrl: *mut c_void,
}

/// G3-P1(v3)探测主入口(loader 经 `caligo_g3_probe_run` 远程线程调用)。
///
/// 返回 [`OK`] 时机制链(adopt→site→swap→restore→drain)已完整走完;
/// captured=0 属正常(窗口内无 SSO 发送),看报告 stages 判定。
///
/// SAFETY: ctx 必须指向本进程内有效的 [`G3Ctx`]。
pub unsafe fn g3_probe_run(ctx: *const G3Ctx) -> u32 {
    // SAFETY: ctx 由 loader 写入。
    let (wrapper_base, report_path, observe_ms) =
        unsafe { ((*ctx).wrapper_base, (*ctx).report_path, (*ctx).observe_ms) };
    let observe_ms = observe_ms.min(300_000).max(500);

    // wrapper 身份核验(getter 租约 + 锚点;g1/g2 同机械)。
    let getter: GetterFn = unsafe { core::mem::transmute(wrapper_base + GETTER_RVA) };
    let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
    // SAFETY: getter 契约(R2 §4)。
    let ret = unsafe { getter(&mut pair as *mut ServicePair) };
    if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
        stage(report_path, "g3_adopt", false, "getter failed");
        return ERR_NO_OBSERVER;
    }
    let vptr = unsafe { core::ptr::read_volatile(pair.obj as *const u64) };
    if (vptr as usize).wrapping_sub(wrapper_base) != ANCHOR_SVC_VTBL_RVA {
        stage(report_path, "g3_adopt", false, "anchor mismatch");
        return ERR_SWAP_STATE;
    }
    stage(report_path, "g3_adopt", true, &format!("svc={}", pair.obj as u64));

    // 发送侧选址:transport = *(svc+0x60);conn pair = *(transport+0x60)。
    let Some(transport) = safe_read_qword(pair.obj as usize + SVC_FIELD_TRANSPORT) else {
        stage(report_path, "g3_site", false, "svc+0x60 unreadable");
        return ERR_NO_TRANSPORT;
    };
    if !addr_plausible(transport) {
        stage(report_path, "g3_site", false, &format!("transport implausible {}", hx(transport)));
        return ERR_NO_TRANSPORT;
    }
    let orig_vptr = safe_read_qword(transport as usize).unwrap_or(0);
    let vptr_ok = orig_vptr == (wrapper_base + TRANSPORT_VTBL_RVA) as u64;
    let conn = safe_read_qword(transport as usize + TRANSPORT_FIELD_CONN).unwrap_or(0);
    let conn_vptr =
        if addr_plausible(conn) { safe_read_qword(conn as usize).unwrap_or(0) } else { 0 };
    stage(
        report_path,
        "g3_site",
        vptr_ok,
        &format!(
            "transport={} vptr={} ({}) conn={} conn_vptr={}",
            hx(transport),
            hx(orig_vptr),
            if vptr_ok { "raw" } else { "MISMATCH" },
            hx(conn),
            hx(conn_vptr)
        ),
    );
    if !vptr_ok {
        return ERR_NO_TRANSPORT;
    }

    // 伪 vtable(自分配 RW 数据页;原表 16 qword 副本,slot7 → 蹦床)。
    let bytes = trampoline_bytes(g3_capture as usize, (wrapper_base + SLOT7_ORIG_RVA) as usize);
    let tramp = unsafe {
        use windows_sys::Win32::System::Memory::{
            VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
        };
        VirtualAlloc(std::ptr::null(), 0x100, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE)
    };
    if tramp.is_null() {
        stage(report_path, "g3_swap", false, "VirtualAlloc(trampoline) failed");
        return ERR_SWAP_STATE;
    }
    // SAFETY: 蹦床页为本进程新分配 RWX,大小 0x100 ≥ bytes.len()。
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len()) };
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        FlushInstructionCache(GetCurrentProcess(), tramp, bytes.len());
    }
    let mut fake_tbl = [0u64; VTBL_COPY_QWORDS];
    let n = read_qwords_into(orig_vptr as usize, &mut fake_tbl);
    if n < 10 {
        stage(report_path, "g3_swap", false, &format!("vtable copy short: {n}"));
        return ERR_SWAP_STATE;
    }
    fake_tbl[SLOT7_OFF / 8] = tramp as u64;
    let fake_vtbl = unsafe {
        use windows_sys::Win32::System::Memory::{
            VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
        };
        VirtualAlloc(std::ptr::null(), 0x100, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
    };
    if fake_vtbl.is_null() {
        stage(report_path, "g3_swap", false, "VirtualAlloc(fake vtable) failed");
        return ERR_SWAP_STATE;
    }
    // SAFETY: 伪 vtable 页为本进程新分配 RW,大小 0x100 ≥ 16*8。
    unsafe {
        core::ptr::copy_nonoverlapping(
            fake_tbl.as_ptr().cast::<u8>(),
            fake_vtbl as *mut u8,
            VTBL_COPY_QWORDS * 8,
        )
    };

    TRAMPOLINE.store(tramp as u64, Ordering::Release);
    FAKE_VTBL.store(fake_vtbl as u64, Ordering::Release);
    ORIGINAL_VPTR.store(orig_vptr, Ordering::Release);

    // 交换对象头 vptr(纯堆 8B 原子;身份双门已在 site 阶段核验)。
    let prev = match swap_slot(transport as usize, fake_vtbl as u64) {
        Ok(p) => p,
        Err(e) => {
            stage(report_path, "g3_swap", false, &e);
            return ERR_SWAP_STATE;
        }
    };
    if prev != orig_vptr {
        let _ = swap_slot(transport as usize, orig_vptr);
        stage(report_path, "g3_swap", false, &format!("prev changed mid-swap: {}", hx(prev)));
        return ERR_SWAP_STATE;
    }
    ACTIVE.store(1, Ordering::Release);
    stage(
        report_path,
        "g3_swap",
        true,
        &format!(
            "fake_vtbl={} trampoline={} prev={}",
            hx(fake_vtbl as u64),
            hx(tramp as u64),
            hx(prev)
        ),
    );

    // 观测窗口(心跳/推送 ACK/用户发送均经 transport slot7)。
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(observe_ms as u64);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // 恢复:先关闸(迟到调用安全直入原函数),再写回。
    ACTIVE.store(0, Ordering::Release);
    let cur = safe_read_qword(transport as usize).unwrap_or(0);
    if cur == fake_vtbl as u64 {
        match swap_slot(transport as usize, orig_vptr) {
            Ok(_) => {
                let after = safe_read_qword(transport as usize).unwrap_or(0);
                if after == orig_vptr {
                    stage(
                        report_path,
                        "g3_restore",
                        true,
                        "transport vptr restored (fake/trampoline pages retained)",
                    );
                } else {
                    stage(
                        report_path,
                        "g3_restore",
                        false,
                        &format!("post-restore verify failed: {}", hx(after)),
                    );
                }
            }
            Err(e) => stage(report_path, "g3_restore", false, &e),
        }
    } else {
        stage(
            report_path,
            "g3_restore",
            false,
            &format!("vptr changed during window (not rewriting stale): {}", hx(cur)),
        );
    }

    // drain 固定环 → 报告(编码在本线程,可分配)。
    let (records, pushed, dropped) = drain_records();
    use std::io::Write;
    if let Ok(mut f) =
        std::fs::OpenOptions::new().create(true).append(true).open(report_path_str(report_path))
    {
        for rec in &records {
            let _ = writeln!(f, "{rec}");
        }
        let _ = writeln!(
            f,
            "{{\"kind\":\"summary\",\"pushed\":{pushed},\"dropped\":{dropped},\"encoded\":{}}}",
            records.len()
        );
    }
    stage(report_path, "g3_done", true, &format!("captured={pushed} dropped={dropped}"));
    OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex as SyncMutex;

    /// ACTIVE 闸/固定环是进程级共享静态,涉及它们的测试必须串行。
    static EXEC_LOCK: SyncMutex<()> = SyncMutex::new(());

    /// 编码形状:长度 66、分段字节、内嵌地址偏移。
    #[test]
    fn trampoline_encoding_exact() {
        let b = trampoline_bytes(0x1234_5678_9ABC_DEF0, 0x0FED_CBA9_8765_4321);
        // 4 push(6B)+sub(4)+4 mov(20)+movabs(10)+call(2)+add(4)+4 pop(6)+jmp(6)+imm64(8)
        assert_eq!(b.len(), 66);
        assert_eq!(&b[0..4], &[0x51, 0x52, 0x41, 0x50]);
        assert_eq!(&b[4..6], &[0x41, 0x51]);
        assert_eq!(&b[6..10], &[0x48, 0x83, 0xEC, 0x28]);
        assert_eq!(&b[30..32], &[0x48, 0xB8]);
        assert_eq!(&b[32..40], &0x1234_5678_9ABC_DEF0u64.to_le_bytes());
        assert_eq!(&b[40..42], &[0xFF, 0xD0]);
        assert_eq!(&b[42..46], &[0x48, 0x83, 0xC4, 0x28]);
        assert_eq!(&b[46..50], &[0x41, 0x59, 0x41, 0x58]);
        assert_eq!(&b[50..52], &[0x5A, 0x59]);
        assert_eq!(&b[52..58], &[0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(&b[58..66], &0x0FED_CBA9_8765_4321u64.to_le_bytes());
    }

    /// 蹦床执行:真实 VirtualAlloc RWX + 以 6 参调用,验证零分配捕获记录与
    /// 尾跳传参(第 5/6 参经栈)完整到达 original。
    #[test]
    fn trampoline_executes_capture_and_tailjump() {
        let _g = EXEC_LOCK.lock().unwrap();
        static ORIG_A5: AtomicUsize = AtomicUsize::new(0);
        static ORIG_A6: AtomicUsize = AtomicUsize::new(0);
        static ORIG_HITS: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "system" fn fake_orig(
            _a1: usize,
            _a2: usize,
            _a3: usize,
            _a4: usize,
            a5: usize,
            a6: usize,
        ) {
            ORIG_A5.store(a5, Ordering::Relaxed);
            ORIG_A6.store(a6, Ordering::Relaxed);
            ORIG_HITS.fetch_add(1, Ordering::Relaxed);
        }

        // 假 uin string(短形态 "3089665724")。
        let mut uin_buf = [0u8; 24];
        let s = b"3089665724";
        uin_buf[0] = (s.len() as u8) << 1;
        uin_buf[1..1 + s.len()].copy_from_slice(s);
        let uin = Box::leak(Box::new(uin_buf));

        // 假 task:0x88B,+0x00 放一个可解引用的候选(指向 blob 对象)。
        let blob: Box<[u64; 4]> = Box::new([0xDEAD_BEEF_0000_1000, 1, 2, 3]);
        let blob_ptr = Box::leak(blob) as *mut u64 as usize;
        let mut task = [0u64; TASK_BYTES / 8];
        task[0] = blob_ptr as u64;
        task[3] = 0x31;
        let task = Box::leak(Box::new(task));
        // slot7 的 a3 = 指向"任务指针槽"的指针(*a3 = task)。
        let task_slot = Box::leak(Box::new(task.as_mut_ptr() as u64));

        let policy = Box::leak(Box::new([180000u64, 1, 180000, 1, 2, 1]));
        let cb = Box::leak(Box::new([0x1111_2222_3333_4444u64, 0x5555_6666_7777_8888]));
        let token_out = Box::leak(Box::new(0u32));

        ACTIVE.store(1, Ordering::SeqCst);
        let before = NEXT.load(Ordering::Relaxed);

        let bytes = trampoline_bytes(g3_capture as usize, fake_orig as usize);
        let tramp = unsafe {
            use windows_sys::Win32::System::Memory::{
                VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
            };
            VirtualAlloc(std::ptr::null(), 0x100, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE)
        };
        assert!(!tramp.is_null());
        // SAFETY: 测试进程内新分配 RWX 页。
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len()) };

        let f: unsafe extern "system" fn(usize, usize, usize, usize, usize, usize) =
            unsafe { core::mem::transmute(tramp as usize) };
        // SAFETY: 蹦床由本测试写入,目标假宿主无副作用。
        unsafe {
            f(
                0x1000,
                uin.as_mut_ptr() as usize,
                task_slot as *mut u64 as usize,
                policy.as_mut_ptr() as usize,
                token_out as *mut u32 as usize,
                cb.as_mut_ptr() as usize,
            )
        };

        assert_eq!(ORIG_HITS.load(Ordering::Relaxed), 1, "original not reached");
        assert_eq!(ORIG_A5.load(Ordering::Relaxed), token_out as *mut u32 as usize, "a5 mangled");
        assert_eq!(ORIG_A6.load(Ordering::Relaxed), cb.as_mut_ptr() as usize, "a6 mangled");

        let (records, pushed, _dropped) = drain_records();
        assert!(pushed > before, "no capture record");
        let rec = records.last().expect("no encoded record");
        assert!(rec.contains("\"kind\":\"sso_call\""));
        assert!(rec.contains("3089665724"), "uin short-form not parsed: {rec}");
        assert!(rec.contains(&format!("\"task_ptr\":{}", hx(task.as_mut_ptr() as u64))));
        assert!(
            rec.contains(&format!("\"ptr\":{}", hx(blob_ptr as u64))),
            "blob candidate missing: {rec}"
        );
        // policy 以 hex 记录:180000 = 0x2BF20(LE "20bf02…")。
        assert!(rec.contains("\"policy\":\"20bf02"), "policy not captured: {rec}");
        // callback 以 hex 记录(LE 4444 3333 2222 1111)。
        assert!(rec.contains("4444333322221111"), "cb not captured: {rec}");
        ACTIVE.store(0, Ordering::SeqCst);
    }

    /// ACTIVE=0 时蹦床仍安全直入原函数(恢复后迟到调用路径)。
    #[test]
    fn inactive_trampoline_skips_capture() {
        let _g = EXEC_LOCK.lock().unwrap();
        static HITS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "system" fn nop_orig(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) {
            HITS.fetch_add(1, Ordering::Relaxed);
        }
        ACTIVE.store(0, Ordering::SeqCst);
        let before = NEXT.load(Ordering::Relaxed);
        let bytes = trampoline_bytes(g3_capture as usize, nop_orig as usize);
        let tramp = unsafe {
            use windows_sys::Win32::System::Memory::{
                VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
            };
            VirtualAlloc(std::ptr::null(), 0x100, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE)
        };
        assert!(!tramp.is_null());
        // SAFETY: 测试进程内新分配 RWX 页。
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len()) };
        let f: unsafe extern "system" fn(usize, usize, usize, usize, usize, usize) =
            unsafe { core::mem::transmute(tramp as usize) };
        // SAFETY: 假宿主。
        unsafe { f(0, 0, 0, 0, 0, 0) };
        assert_eq!(HITS.load(Ordering::Relaxed), 1);
        assert_eq!(NEXT.load(Ordering::Relaxed), before, "capture ran while inactive");
    }

    /// swap_slot 往返对称(自进程 RW 页模拟对象头)。
    #[test]
    fn swap_slot_roundtrip_symmetric() {
        let mut table = [0u64; 16];
        table[7] = 0xAAAA_BBBB_CCCC_DDDD;
        let slot = &mut table[7] as *mut u64 as usize;
        let prev = swap_slot(slot, 0x1234_5678_9ABC_DEF0).expect("swap failed");
        assert_eq!(prev, 0xAAAA_BBBB_CCCC_DDDD);
        // SAFETY: 测试自有内存。
        assert_eq!(unsafe { core::ptr::read_volatile(slot as *const u64) }, 0x1234_5678_9ABC_DEF0);
        let prev2 = swap_slot(slot, 0xAAAA_BBBB_CCCC_DDDD).expect("restore failed");
        assert_eq!(prev2, 0x1234_5678_9ABC_DEF0);
        // SAFETY: 测试自有内存。
        assert_eq!(unsafe { core::ptr::read_volatile(slot as *const u64) }, 0xAAAA_BBBB_CCCC_DDDD);
    }

    /// libc++ string 判形(短/长)。
    #[test]
    fn libcxx_string_form() {
        assert_eq!(string_form((b"abc".len() as u64) << 1), Some((true, 3)));
        assert_eq!(string_form(0x1001), Some((false, 0)));
        assert_eq!(string_form(48), None); // size 24 > 22 → 非短形态且低位 0
    }

    /// 候选上限:满是指针对象的 task 也只解引用 ≤CAND_CAP 个。
    #[test]
    fn candidate_chasing_capped() {
        let mut task = [0u64; TASK_BYTES / 8];
        for (i, slot) in task.iter_mut().take(8).enumerate() {
            let blob: Box<[u64; 2]> = Box::new([0x7000_0000 + i as u64, 0]);
            *slot = Box::leak(blob) as *mut u64 as u64;
        }
        let mut n = 0usize;
        for (i, q) in task.iter().enumerate() {
            if n >= CAND_CAP {
                break;
            }
            if !addr_plausible(*q) {
                continue;
            }
            if safe_read_qword(*q as usize).is_none() {
                continue;
            }
            let _ = i;
            n += 1;
        }
        assert_eq!(n, CAND_CAP);
    }
}
