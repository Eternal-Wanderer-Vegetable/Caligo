//! 响应消费者探针(N3 retCode 通道;research 构建)。
//!
//! 静态定案(2026-10-11 v4;mgrlog2 几何 + 门函数全反编译):
//! - cb 块 = 32B delegate,**布局 L1**:`{storage0, storage1, manager, invoker}`。
//!   静态空 delegate = {0, 0, 0x2338(空管理器), 0x236a(空 invoker)};
//!   QQ 空管理器另一实例 = 0x72D596/0x72D5C8。
//! - **manager 契约(0x72D596 + 0x31C5F8 移动器反编译)**:
//!   `(dest=rcx = &new.manager槽, op=edx, src=r8 = 旧块起点, size=r9 = 0x10,
//!     out=[rsp+0x28] = 新块起点)`:
//!   * op=0(移动构造):manager 须把 src 的 storage0/1 复制到 out[0..2]
//!     (移动器**不**复制 storage,只重置源为空对),再向 dest[0..2] 写
//!     {manager, invoker} 对;
//!   * op=1 / op=3(销毁/重置):向 dest[0..2] 写空对(本实现 = leaked
//!     storage,无析构,重置为 wrapper 空对 0x2338/0x236a);
//!   * op>=4(**空查询**,3fce5e:`manager(NULL,4,NULL,0,&out16,0)`)——
//!     **out[0] = 0 表示"非空可调用";非零(原生空管理器写 1)表示"空"**。
//!     这是 N3R31/32/33 invoker 未触发的根因(旧探针写非零 = 自称空,
//!     73bd6c 门跳过 invoke 直接销毁;r1 时代"非零 = 门开"解读为极性反转)。
//! - 门 0x3fbab2 = `3fce5e(rcx+0x10)` 后 `xor al,1` → 返回"非空";
//!   73bd6c/739c66/3fb914/6a7d0c 均在"非空"时 invoke。
//! - 响应 invoke(73bd6c 实测 6 参):`invoker(inner块, 0x10, errCode(i32),
//!   errStr*(__qq string), writer*, extra*(msgUid 串))`;errCode 三级
//!   fallback = box+0x40 → +0x60 → +0x64。
//! - __qq string 布局:byte0&1 → 长形态 {cap|1, size, data*};否则短形态
//!   {len<<1, data 从 byte 1 内联}(make_short_string 同款,field-proven)。
//!
//! 防御:invoke/manager 全部 RPM-self 安全读 + 定长写(布局误判不崩);
//! 回调纪律与 g2 捕获同(零 QQ 调用、零堆分配(FixedBuf)、不 unwind、
//! 文件追加经进程级互斥锁防半行交错)。

use core::ffi::c_void;

/// 消费者上下文(进程生存期 leaked;QQ 线程回调只读)。
#[repr(C)]
pub struct ConsumerCtx {
    /// 报告路径(UTF-16,NUL 结尾;定长足够)。
    pub report: [u16; 180],
    /// wrapper 模块基址(op=1/3 重置空对用;0 = 未知则跳过重置)。
    pub wrapper_base: u64,
    /// 已记录条数(诊断)。
    pub hits: u64,
    /// 魔数(自检)。
    pub magic: u64,
}

const CTX_MAGIC: u64 = 0xC0A1_5E09_C0DE;

/// wrapper 空对(manager/invoker;0x31C5F8 移动器与 0x7333ac 同款)。
const EMPTY_MGR_RVA: u64 = 0x2338;
const EMPTY_INV_RVA: u64 = 0x236A;

/// 从报告路径构造 leaked ctx(每 probe 一次;进程退出回收)。
pub fn leak_ctx(wrapper_base: usize, report_path: *const u16) -> &'static mut ConsumerCtx {
    let mut ctx = ConsumerCtx {
        report: [0; 180],
        wrapper_base: wrapper_base as u64,
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

/// RPM-self 连续字节读(qword 步进;任一失败 → None)。
fn read_bytes(addr: usize, len: usize) -> Option<Vec<u8>> {
    if addr < 0x10000 || len > 4096 {
        return None;
    }
    let nq = (len + 7) / 8;
    let mut buf = Vec::with_capacity(nq * 8);
    for i in 0..nq {
        let q = safe_read_qword(addr + i * 8)?;
        buf.extend_from_slice(&q.to_le_bytes());
    }
    buf.truncate(len);
    Some(buf)
}

/// __qq string 读取(短形态 data@+1 内联 / 长形态 {cap|1,size,data*};
/// 只读不持)。地址不可读 → None;size=0 → 空串。
fn read_qq_string(ptr: usize) -> Option<String> {
    if ptr < 0x10000 {
        return None;
    }
    let mut raw = [0u64; 3];
    for (i, slot) in raw.iter_mut().enumerate() {
        *slot = safe_read_qword(ptr + i * 8)?;
    }
    let q0 = raw[0].to_le_bytes();
    let (size, data_ptr) = if q0[0] & 1 == 0 {
        ((q0[0] >> 1) as usize, ptr + 1)
    } else {
        (raw[1] as usize, raw[2] as usize)
    };
    if size == 0 {
        return Some(String::new());
    }
    if size > 512 || data_ptr < 0x10000 {
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

/// 进程级写锁(回调线程交错追加曾产生半行 JSON;临界区微秒级)。
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// JSONL 追加(回调内唯一写动作;持锁防交错;失败静默——回调纪律)。
fn append_line(ctx: &ConsumerCtx, line: &[u8]) {
    use std::os::windows::ffi::OsStringExt;
    let end = ctx.report.iter().position(|c| *c == 0).unwrap_or(180);
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &ctx.report[..end],
    ));
    use std::io::Write;
    // 中毒锁 = 上次写时 panic(不该发生);跳过本次,不放大故障。
    if let Ok(_g) = FILE_LOCK.lock() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(line);
            let _ = f.write_all(b"\n");
        }
    }
}

/// 栈上定长格式化缓冲(零堆分配;回调纪律)。
struct FixedBuf {
    data: [u8; 768],
    len: usize,
}

impl FixedBuf {
    fn new() -> Self {
        Self { data: [0; 768], len: 0 }
    }
    fn push_str(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(self.data.len() - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&b[..n]);
        self.len += n;
    }
    /// JSON 字符串转义(定长截断到 max 字节;控制字符转 \u00XX)。
    fn push_json_escaped(&mut self, s: &str, max: usize) {
        let b = s.as_bytes();
        let n = b.len().min(max);
        for &c in &b[..n] {
            if self.data.len() - self.len < 8 {
                break;
            }
            match c {
                b'"' => self.push_str("\\\""),
                b'\\' => self.push_str("\\\\"),
                b'\n' => self.push_str("\\n"),
                b'\r' => self.push_str("\\r"),
                b'\t' => self.push_str("\\t"),
                0x20..=0x7E => {
                    self.data[self.len] = c;
                    self.len += 1;
                }
                _ => {
                    // 定长 \u00XX(单字节值;UTF-8 多字节逐字节转义,可逆性
                    // 不保——诊断用途)。
                    let hex = b"0123456789abcdef";
                    self.push_str("\\u00");
                    self.data[self.len] = hex[(c >> 4) as usize];
                    self.len += 1;
                    self.data[self.len] = hex[(c & 0xf) as usize];
                    self.len += 1;
                }
            }
        }
    }
    fn push_dec(&mut self, v: u64) {
        if v == 0 {
            self.push_str("0");
            return;
        }
        let mut tmp = [0u8; 20];
        let mut i = 20;
        let mut v = v;
        while v > 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        // SAFETY: tmp[i..] 全为 ASCII 数字。
        self.push_str(unsafe { core::str::from_utf8_unchecked(&tmp[i..]) });
    }
    fn push_i32(&mut self, v: i32) {
        if v < 0 {
            self.push_str("-");
            self.push_dec((-(v as i64)) as u64);
        } else {
            self.push_dec(v as u64);
        }
    }
    fn push_hex(&mut self, v: u64) {
        self.push_str("0x");
        if v == 0 {
            self.push_str("0");
            return;
        }
        let mut tmp = [0u8; 16];
        let mut i = 16;
        let mut v = v;
        while v > 0 {
            i -= 1;
            tmp[i] = b"0123456789abcdef"[(v & 0xf) as usize];
            v >>= 4;
        }
        // SAFETY: tmp[i..] 全为 hex ASCII。
        self.push_str(unsafe { core::str::from_utf8_unchecked(&tmp[i..]) });
    }
    fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

/// 进程级静态 ctx(manager 无 this 可依,用注册时的全局)。
static STATIC_CTX: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

fn resolve_ctx_static() -> Option<&'static ConsumerCtx> {
    let p = *STATIC_CTX.get_or_init(|| 0);
    if p > 0x10000 {
        // SAFETY: p 来自 leak_ctx(进程生存期)。
        Some(unsafe { &*(p as *const ConsumerCtx) })
    } else {
        None
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// manager(QQ 侧 delegate 管理器;契约 = 0x72D596 + 0x31C5F8 反编译,v4 修正):
/// `(dest=rcx = &new.manager槽, op=edx, src=r8 = 旧块起点, size=r9,
///   out=栈参5 = 新块起点)`。
/// - op=0(移动构造):复制 src storage0/1 → out[0..2](移动器不复制!),
///   再向 dest[0..2] 写 {manager, invoker} 对;
/// - op=1/3(销毁/重置):dest[0..2] = wrapper 空对(leaked storage 无析构);
/// - op=4(**空查询**):out[0] = **0**(非空可调用)——v4 极性修正;
/// - 其他 op:no-op。
/// 每次调用追加 mgr_call 观测行(定长栈格式化,零分配)。
///
/// # Safety
/// QQ 线程调用;dest/out 为 QQ 提供缓冲;不得 unwind。
pub unsafe extern "system" fn consumer_manager(
    dest: *mut c_void,
    op: u32,
    src: *mut c_void,
    size: usize,
    out: *mut c_void,
) {
    // SAFETY: dest/out 为 QQ 提供的 delegate/查询缓冲;写 = 契约同款
    // (空管理器写 2 qword;我们加 storage 复制 = 移动器职责)。
    unsafe {
        match op {
            0 => {
                // storage 复制(RPM-self 读;坏地址写 0,不崩)。
                if !out.is_null() && !src.is_null() {
                    let o = out as *mut u64;
                    let s = src as *const u64;
                    let v0 = safe_read_qword(s as usize).unwrap_or(0);
                    let v1 = safe_read_qword(s.add(1) as usize).unwrap_or(0);
                    core::ptr::write(o, v0);
                    core::ptr::write(o.add(1), v1);
                }
                if !dest.is_null() {
                    let d = dest as *mut u64;
                    core::ptr::write(d, consumer_manager as usize as u64);
                    core::ptr::write(d.add(1), consumer_invoke as usize as u64);
                }
            }
            1 | 3 => {
                if !dest.is_null() {
                    let d = dest as *mut u64;
                    let base = resolve_ctx_static()
                        .map(|c| c.wrapper_base)
                        .unwrap_or(0);
                    if base > 0x10000 {
                        core::ptr::write(d, base + EMPTY_MGR_RVA);
                        core::ptr::write(d.add(1), base + EMPTY_INV_RVA);
                    } else {
                        core::ptr::write(d, consumer_manager as usize as u64);
                        core::ptr::write(d.add(1), consumer_invoke as usize as u64);
                    }
                }
            }
            4 => {
                // 空查询:0 = 非空(可调用)。v4 极性修正(r2/r3 写非零 =
                // 自称空 → 73bd6c 跳过 invoke —— N3R31/32/33 根因)。
                if !out.is_null() {
                    core::ptr::write(out as *mut u64, 0u64);
                }
            }
            _ => {}
        }
        // 观测:契约完成后追加调用日志(定长栈格式化,零分配)。
        let ctx = match resolve_ctx_static() {
            Some(c) => c,
            None => return,
        };
        let mut b = FixedBuf::new();
        b.push_str("{\"kind\":\"mgr_call\",\"ts\":");
        b.push_dec(now_ms());
        b.push_str(",\"op\":");
        b.push_dec(op as u64);
        b.push_str(",\"dest\":\"");
        b.push_hex(dest as usize as u64);
        b.push_str("\",\"src\":\"");
        b.push_hex(src as usize as u64);
        b.push_str("\",\"size\":");
        b.push_hex(size as u64);
        b.push_str(",\"out\":\"");
        b.push_hex(out as usize as u64);
        b.push_str("\"}");
        append_line(ctx, b.as_slice());
    }
}

/// 消费者回调(响应时经 73bd6c 门后调用;6 参 = (inner块, 0x10, errCode,
/// errStr*, writer*, extra*)。防御式:RPM-self 读 + FixedBuf + 锁内追加。
///
/// # Safety
/// QQ 线程调用;只读;不得调用 QQ API、不得 unwind。
pub unsafe extern "system" fn consumer_invoke(
    this: *mut c_void,
    tag: u32,
    err_code: i32,
    err_str: *const c_void,
    writer: *const c_void,
    extra: *const c_void,
) {
    // SAFETY: 回调纪律 — RPM-self 安全读 + 定长格式化 + 锁内追加。
    unsafe {
        let ctx = resolve_ctx_static().or_else(fallback_ctx);
        let ctx = match ctx {
            Some(c) => c,
            None => return,
        };
        let err_str = read_qq_string(err_str as usize).unwrap_or_default();
        // extra(73bd6c arg6)= 0x32c4 构造的 span {begin,end,cap_end}
        // (0x38aa 分配 + 0x38e6 拷贝;box 空时 0x20 全零 = 空 span)。
        // 原始 qword 一并记录(诊断;疑似 16B msgUid cookie)。
        let (extra_hex, extra_txt) = {
            let mut q = [0u64; 3];
            let mut ok = true;
            for (i, slot) in q.iter_mut().enumerate() {
                match safe_read_qword(extra as usize + i * 8) {
                    Some(v) => *slot = v,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                (String::from("<unreadable>"), String::new())
            } else {
                let begin = q[0] as usize;
                let end = q[1] as usize;
                let len = end.saturating_sub(begin);
                let bytes = if len == 0 {
                    Some(Vec::new())
                } else if len <= 512 && begin > 0x10000 {
                    read_bytes(begin, len)
                } else {
                    None
                };
                let mut hb = FixedBuf::new();
                match bytes.as_deref() {
                    None => hb.push_str(if len == 0 { "" } else { "<bad-span>" }),
                    Some(bs) => {
                        if bs.is_empty() {
                            hb.push_str("");
                        } else {
                            for (i, byte) in bs.iter().take(64).enumerate() {
                                if i > 0 {
                                    hb.push_str(" ");
                                }
                                hb.push_hex(*byte as u64);
                            }
                        }
                    }
                }
                let hexs = core::str::from_utf8(hb.as_slice())
                    .unwrap_or("")
                    .to_string();
                let txt = match bytes.as_deref() {
                    Some(bs) if !bs.is_empty() => {
                        String::from_utf8_lossy(bs).into_owned()
                    }
                    _ => String::new(),
                };
                (hexs, txt)
            }
        };
        let mut b = FixedBuf::new();
        b.push_str("{\"kind\":\"consumer_result\",\"ts\":");
        b.push_dec(now_ms());
        b.push_str(",\"this\":\"");
        b.push_hex(this as usize as u64);
        b.push_str("\",\"tag\":");
        b.push_dec(tag as u64);
        b.push_str(",\"err_code\":");
        b.push_i32(err_code);
        b.push_str(",\"err_str\":\"");
        b.push_json_escaped(&err_str, 200);
        b.push_str("\",\"writer\":\"");
        b.push_hex(writer as usize as u64);
        b.push_str("\",\"extra_hex\":\"");
        b.push_json_escaped(&extra_hex, 220);
        b.push_str("\",\"extra_txt\":\"");
        b.push_json_escaped(&extra_txt, 100);
        b.push_str("\"}");
        append_line(ctx, b.as_slice());
    }
}

static FALLBACK: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

static FALLBACK_BASE: std::sync::Mutex<Option<(usize, Vec<u16>)>> =
    std::sync::Mutex::new(None);

/// 登记 fallback(wrapper 基址 + 报告路径;注册 ctx 时调用)。
pub fn set_fallback(wrapper_base: usize, report_path: *const u16) {
    let mut v = Vec::new();
    // SAFETY: loader 写入的 NUL 结尾 UTF-16。
    unsafe {
        let mut i = 0usize;
        while i < 179 && *report_path.add(i) != 0 {
            v.push(*report_path.add(i));
            i += 1;
        }
    }
    *FALLBACK_BASE.lock().unwrap() = Some((wrapper_base, v));
}

/// 进程级 fallback ctx(报告路径 = 消费者探针缺省;注册于 leak_ctx 时)。
fn fallback_ctx() -> Option<&'static ConsumerCtx> {
    let p = *FALLBACK.get_or_init(|| {
        let guard = FALLBACK_BASE.lock().unwrap();
        let (base, report) = guard.clone().unwrap_or((0, Vec::new()));
        drop(guard);
        let mut ctx = ConsumerCtx {
            report: [0; 180],
            wrapper_base: base as u64,
            hits: 0,
            magic: CTX_MAGIC,
        };
        for (i, c) in report.iter().take(179).enumerate() {
            ctx.report[i] = *c;
        }
        Box::leak(Box::new(ctx)) as *const ConsumerCtx as usize
    });
    if p > 0x10000 {
        // SAFETY: p 来自本函数 leak(进程生存期)。
        Some(unsafe { &*(p as *const ConsumerCtx) })
    } else {
        None
    }
}

/// delegate 构造(布局 L1):{storage0=ctx, storage1=1, manager, invoker}。
/// storage0 非空 = 旧版门判据;v4 门 = manager op=4 答 0。manager 正确
/// 克隆(含 storage 复制);invoker = 6 参消费者。
/// 同时登记进程级 STATIC_CTX 与 fallback(OnceLock/Mutex 首次生效)。
pub fn build_cb(wrapper_base: usize, ctx: &ConsumerCtx) -> [u64; 4] {
    let p = ctx as *const ConsumerCtx as usize;
    STATIC_CTX.get_or_init(|| p);
    set_fallback(wrapper_base, ctx.report.as_ptr());
    [
        p as u64,
        1,
        consumer_manager as usize as u64,
        consumer_invoke as usize as u64,
    ]
}
