//! 响应消费者探针(N3 retCode 通道;research 构建)。
//!
//! 静态定案(2026-10-11;含 N3R30 现场教训):
//! - cb 块 = 32B std::function 式 delegate,**布局 L1**:
//!   `{storage0, storage1, manager, invoker}`;
//!   空 delegate = {0, 0, 0x72D596(管理器), 0x72D5C8(terminate 哨兵)}。
//! - 门 3fbab2 = 检查 **storage 非空**(inner+0x10 = cb[0])——空 → 响应
//!   不调用;非空 → 响应时 `(**(dispatch+0x30))(delegate起点, 0x10,
//!   errCode, errStr, msgUid)` = **cb[3](invoker)**,this = delegate 起点。
//! - manager 契约(0x72D596 反编译):`(dest=rcx, op=edx, ...)`;op 0/1 =
//!   克隆/销毁(空实现向 dest 写 {manager, invoker} 对;非空须复制全部
//!   4 qword),op 2/3 = no-op。QQ 在请求建立时会克隆 delegate——manager
//!   必须正确复制,否则 dest 残留垃圾 → 崩溃(**N3R30 教训:处理器误放
//!   manager 槽,克隆损坏,实例死亡**)。
//! - errCode 层级:transport 参数 → resp 空 = -1(180s 超时)→ resp+0x40
//!   → +0x60 → +0x64 → 0;msgUid = string@resp+0x38。
//!
//! 防御:invoke/manager 全部 RPM-self 安全读 + 定长写(布局误判不崩);
//! 回调纪律与 g2 捕获同(零 QQ 调用、零分配、不 unwind)。

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

/// delegate 析构/销毁臂(manager op 1;storage 为 leaked,no-op)。
///
/// # Safety
/// QQ 线程调用;不得 unwind。
pub unsafe extern "system" fn consumer_dtor(_ctx: *mut c_void) {}

/// manager(QQ 侧 delegate 管理器;契约 = 0x72D596 反编译 + 3fce5e 门查询):
/// `(dest=rcx, op=edx, src=r8, size=r9, out=[rsp+28h])`。
/// - op=0(克隆,submit 时):向 dest 写 {manager, invoker} 对(空管理器
///   同款;16B = size);
/// - op=4(门查询,3fce5e:dest/src/size 全零,arg5=&out16):向 out 写
///   非零({invoker, manager})—— 门 = out[0] != 0;
/// - 其他 op(含销毁):no-op(storage 为 leaked)。
/// v3:每次调用在契约完成后向 ctx 报告追加一行 mgr_call(op/args)——
/// 用于观测 QQ 对 delegate 的完整操作序列(含响应时是否到达门查询)。
/// 定长写,零读零分配(日志行固定格式,无 serde)。
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
    // SAFETY: dest/out 为 QQ 提供的 delegate/查询缓冲(空管理器同类操作
    // 写 2 qword,我们同级别);定长写。
    unsafe {
        match op {
            0 => {
                if !dest.is_null() {
                    let d = dest as *mut u64;
                    core::ptr::write(d, consumer_manager as usize as u64);
                    core::ptr::write(d.add(1), consumer_invoke as usize as u64);
                }
            }
            4 => {
                if !out.is_null() {
                    let o = out as *mut u64;
                    core::ptr::write(o, consumer_invoke as usize as u64);
                    core::ptr::write(o.add(1), consumer_manager as usize as u64);
                }
            }
            _ => {}
        }
        // v3 观测:契约完成后追加调用日志(定长栈格式化,零分配)。
        let ctx = match resolve_ctx_static() {
            Some(c) => c,
            None => return,
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut b = FixedBuf::new();
        b.push_str("{\"kind\":\"mgr_call\",\"ts\":");
        b.push_dec(ts);
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
        append_line_raw(ctx, b.as_slice());
    }
}

/// 栈上定长格式化缓冲(零堆分配;回调纪律)。
struct FixedBuf {
    data: [u8; 224],
    len: usize,
}

impl FixedBuf {
    fn new() -> Self {
        Self { data: [0; 224], len: 0 }
    }
    fn push_str(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(self.data.len() - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&b[..n]);
        self.len += n;
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

/// 进程级静态 ctx(v3:manager 无 this 可依,用注册时的全局)。
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

/// 定长原始行追加(无格式化依赖;回调纪律)。
fn append_line_raw(ctx: &ConsumerCtx, line: &[u8]) {
    use std::os::windows::ffi::OsStringExt;
    let end = ctx.report.iter().position(|c| *c == 0).unwrap_or(180);
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&ctx.report[..end]));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line);
        let _ = f.write_all(b"\n");
    }
}

/// 消费者回调(响应时被 QQ 调用;契约 = cb[3](delegate 起点, 0x10,
/// errCode, errStr*, msgUid*)。防御式:ctx 从 this+0 取,字符串 RPM-self 读。
///
/// # Safety
/// QQ 线程调用;只读;不得调用 QQ API、不得 unwind。
pub unsafe extern "system" fn consumer_invoke(
    this: *mut c_void,
    tag: u32,
    err_code: i32,
    err_str: *const c_void,
    msg_uid: *const c_void,
) {
    // SAFETY: 回调纪律 — RPM-self 安全读 + JSONL 追加,无 QQ 调用。
    unsafe {
        let this_addr = this as usize;
        let ctx = resolve_ctx(this_addr);
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
            "this": format!("{this_addr:#x}"),
            "tag": tag,
            "err_code": err_code,
            "err_str": err_str,
            "msg_uid": msg_uid,
        });
        append_line(ctx, &line.to_string());
    }
}

/// 从 invoker 首参(delegate 起点)恢复 ctx:依次尝试 this+0/+8/+0x10
/// 处的指针(魔数自检);失败用进程级 fallback(仍写诊断行)。
unsafe fn resolve_ctx(this_addr: usize) -> &'static ConsumerCtx {
    unsafe {
        if this_addr > 0x10000 {
            for off in [0usize, 8, 0x10] {
                if let Some(p) = safe_read_qword(this_addr + off) {
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

/// delegate 构造(布局 L1):{storage0=ctx, storage1=1, manager, invoker}。
/// storage0 非空 = 门开启;manager 正确克隆;invoker = 消费者。
/// 同时登记进程级 STATIC_CTX(manager 的 v3 日志用;OnceLock 首次生效)。
pub fn build_cb(ctx: &ConsumerCtx) -> [u64; 4] {
    let p = ctx as *const ConsumerCtx as usize;
    STATIC_CTX.get_or_init(|| p);
    [
        p as u64,
        1,
        consumer_manager as usize as u64,
        consumer_invoke as usize as u64,
    ]
}
