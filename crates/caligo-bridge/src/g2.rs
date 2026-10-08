//! G2 接收:Manager+0x170 通知树 begin_node 单指针交换。
//!
//! 静态证据链(R2/R3 反编译+字节解码,详见 `qq-native-receive-design.md` §7):
//! - 通知结构属 `msf::internal::Manager`(vtable `0x413F7A8`,大小 0x1A0,
//!   唯一构造链 SC ctor `0xB78380` → 工厂 `0xB78946` → ctor `0x1B3EADC`);
//! - Manager+0x170 = libc++ `__tree` 容器 `{begin_node@+0x170, end_node@+0x178,
//!   size@+0x180}`,end_node 内嵌;
//! - OnRecv(`1B41AE6`)推送循环:`for (n = *(M+0x170); n != M+0x178;
//!   n = __tree_next(n)) notify_listener(*(n+0x20))`,回调 vtbl slot6(+0x30);
//!   `45e5` 字节码解码 ≡ libc++ `__tree_next`(`{left@0,right@8,parent@0x10}`,
//!   nil=nullptr);slot5(+0x28)/slot9(+0x48) 循环走同一树。
//!
//! 本模块(路线 B,主路线):保存原 begin_node B0 → 原子写入伪树节点
//! `Ours{left:0, right:B0, listener@+0x20 = 我们的对象}` → OnRecv 首个
//! 通知我们(`tree_min(B0)==B0`,因 B0 本为最左)、其后原序不变 → 窗口结束
//! 验证后写回 B0。QQ 自身监听器被原生遍历通知,无需转发。
//! 路线 A(+0x140 单观察者交换)保留为弃用代码(§8),运行时不再选择。
//!
//! 纪律:
//! - 证据:msg 对象头 0x40 字节 opaque hexdump(captured fixture,P5 字段
//!   验证回填用)——**不解析、不猜字段**;
//! - 环形缓冲:回调只做内存拷贝,落盘由运行线程在窗口结束后统一进行;
//! - 伪节点 value(+0x20 起)除 listener 指针外全零:若窗口内 QQ 插入节点
//!   触发 begin 重算/键比较,零值按"空键"处理,不会解引用越界;
//!   若 begin 被 QQ 覆写(座位丢失),**不回写** stale B0(会搁浅 QQ 新节点),
//!   如实记录。

use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// getter 机器级原型(72DE38;同 g1,作 wrapper 身份核验)。
type GetterFn = unsafe extern "system" fn(out: *mut ServicePair) -> *mut ServicePair;

/// GETTER_RVA(同 g1)。
const GETTER_RVA: usize = 0x72DE38;
const ANCHOR_SVC_VTBL_RVA: usize = 0x3F6DED8;

/// SC vtable RVA(0xB78380 ctor 独占;全 .text 仅 ctor/dtor 两处引用)。
const SC_VTBL_RVA: usize = 0x3FD3E28;
/// Manager vtable RVA(RTTI msf::internal::Manager;ctor 1B3EADC/dtor 独占)。
const MANAGER_VTBL_RVA: usize = 0x413F7A8;
/// Manager = *(SC+0x150)(SC ctor:`lea rcx,[rsi+0x150]; call 工厂`)。
const SC_FIELD_MANAGER: usize = 0x150;
/// libc++ __tree 容器位:begin_node@+0x170,end_node(内嵌)@+0x178,size@+0x180。
const FIELD_BEGIN_NODE: usize = 0x170;

pub const OK: u32 = 0;
pub const ERR_NO_OBSERVER: u32 = 21; // getter/锚点失败(沿用 v1 语义)
pub const ERR_SWAP_STATE: u32 = 22;
pub const ERR_NO_MANAGER: u32 = 24; // SC/Manager 定位失败

#[repr(C)]
pub struct G2Ctx {
    pub wrapper_base: usize,
    pub report_path: *const u16,
    pub observe_ms: u32,
}

/// 捕获环(回调侧 push,窗口结束 drain)。有界:满则丢弃并计数。
struct Ring {
    items: Mutex<Vec<(u64, u64)>>, // (msg_obj 头 8 字节, 次八字节)
    dropped: AtomicU64,
    pushed: AtomicU64,
    cap: usize,
}

static RING: std::sync::OnceLock<Ring> = std::sync::OnceLock::new();

fn ring() -> &'static Ring {
    RING.get_or_init(|| Ring {
        items: Mutex::new(Vec::new()),
        dropped: AtomicU64::new(0),
        pushed: AtomicU64::new(0),
        cap: 512,
    })
}

/// 我们的单一监听器对象(vptr 槽;地址稳定性由 static 保证)。
static mut OUR_OBJECT: [*mut c_void; 4] = [core::ptr::null_mut(); 4];
static OUR_OBJECT_READY: AtomicU64 = AtomicU64::new(0);

/// 我们的 vtable:slot3(+0x18)推送=捕获处理器(垫片路径,活实例实证),
/// slot6(+0x30)=同处理器(原版 1B41AE6 路径备份;一次分派只会走其一),
/// slot5(+0x28)/slot9(+0x48)请求=no-op;其余槽位合法 no-op。
static mut OUR_VTABLE: [usize; 16] = [0; 16];

/// 伪树节点(libc++ __tree_node 几何):{left@0, right@8, parent@0x10,
/// is_black@0x18, value@0x20};value.first = 监听器对象,其余置零。
static mut OUR_NODE: [u64; 5] = [0; 5];
static OUR_NODE_READY: AtomicU64 = AtomicU64::new(0);

unsafe extern "system" fn g2_noop2(_a: *mut c_void, _b: *mut c_void) {}

unsafe extern "system" fn g2_noop3(_a: *mut c_void, _b: *mut c_void, _c: *mut c_void) {}

/// slot6 处理器:捕获 msg 对象头 → 环。无转发需求(QQ 监听器被原生遍历通知)。
unsafe extern "system" fn g2_slot6(_this: *mut c_void, msg_pair: *mut c_void) {
    // SAFETY: 边界内只做有界内存读 + ring 拷贝;panic 隔离。
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !msg_pair.is_null() {
            let pair = *(msg_pair as *const [u64; 2]); // {obj, ctrl}
            let obj = pair[0] as usize;
            if obj != 0 {
                // 捕获头 0x40 字节(opaque;8 qword)。
                let mut head = [0u64; 8];
                for (i, h) in head.iter_mut().enumerate() {
                    // SAFETY: QQ 构造的 msg 对象;读失败保持 0。
                    *h = unsafe { core::ptr::read_volatile((obj + i * 8) as *const u64) };
                }
                let r = ring();
                let mut q = r.items.lock().unwrap();
                if q.len() < r.cap {
                    // 打包 8 qword → 4 条 (u64,u64) 记录(顺序保留)。
                    for c in 0..4 {
                        q.push((head[c * 2], head[c * 2 + 1]));
                    }
                    r.pushed.fetch_add(4, Ordering::Relaxed);
                } else {
                    r.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }));
}

fn init_object_and_node() -> (*mut c_void, *mut u64) {
    // SAFETY: OUR_OBJECT/OUR_VTABLE/OUR_NODE 仅在启动阶段由本函数初始化一次
    // (单远程线程;READY 原子闸防重入),此后只读。
    unsafe {
        if OUR_OBJECT_READY.load(Ordering::Acquire) == 0 {
            for slot in OUR_VTABLE.iter_mut() {
                *slot = g2_noop2 as usize;
            }
            OUR_VTABLE[0] = g2_noop2 as usize; // slot0
            OUR_VTABLE[3] = g2_slot6 as usize; // slot3 +0x18 推送(垫片路径,实证)
            OUR_VTABLE[5] = g2_noop2 as usize; // slot5 +0x28 请求完成
            OUR_VTABLE[6] = g2_slot6 as usize; // slot6 +0x30 推送(原版路径备份)
            OUR_VTABLE[9] = g2_noop3 as usize; // slot9 +0x48 请求分派(1B3F740)
            OUR_OBJECT[0] = OUR_VTABLE.as_mut_ptr() as *mut c_void;
            OUR_OBJECT_READY.store(1, Ordering::Release);
        }
        if OUR_NODE_READY.load(Ordering::Acquire) == 0 {
            OUR_NODE = [0; 5]; // left/parent/value 其余位全零
            OUR_NODE_READY.store(1, Ordering::Release);
        }
        (
            OUR_OBJECT.as_mut_ptr() as *mut c_void,
            OUR_NODE.as_mut_ptr() as *mut u64,
        )
    }
}

/// RPM-self 安全读:经内核侧校验,页面已释放/去提交时返回 None 而非异常。
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

/// RPM-self 区块读取(4MB 分块,坏块截断不臆测)。
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
            out.truncate(begin);
            break;
        }
        off += n;
    }
    out
}

/// Manager 结构指纹(2026-10-09 活实例实证):ctor 字节不变量 +0x08 低位
/// 字节 == 1;vptr 要么是原版 Manager vtable,要么是堆垫片 vtable(前 5 槽
/// 全落在 wrapper 模块内,2026-10-09 实测);+0x170 begin_node 可读非零。
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
}

/// 进程内定位活 Manager:扫描自身 MEM_PRIVATE 提交区找 SC vptr needle →
/// m = *(SC+0x150),按结构指纹过验(原版 vtable 或垫片 vtable)。
/// 返回 (manager, 候选数)。
fn locate_manager(wrapper_base: usize) -> Option<(usize, usize)> {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_PRIVATE, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS,
        VirtualQuery,
    };
    let needle = (wrapper_base + SC_VTBL_RVA) as u64;
    let mut addr = 0x10000usize;
    let max = 0x7FFFFFFEFFFFusize;
    let mut candidates = 0usize;
    while addr < max {
        // SAFETY: 自进程查询;mbi 由系统填写。
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let q = unsafe {
            VirtualQuery(
                addr as *const c_void,
                &mut mbi,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if q == 0 {
            break;
        }
        let region = mbi.RegionSize as usize;
        let base_addr = mbi.BaseAddress as usize;
        let protect = mbi.Protect;
        if mbi.State == MEM_COMMIT
            && mbi.Type == MEM_PRIVATE
            && region >= 0x1000
            && region < 0x4000_0000
            && protect & PAGE_GUARD == 0
            && protect & PAGE_NOACCESS == 0
        {
            let buf = read_region_chunked(base_addr, region);
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
            }
        }
        addr = base_addr + region;
    }
    None
}

/// 阶段日志(同 g1 独立实现)。
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

fn hex64(v: u64) -> String {
    format!("{v:#x}")
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

/// G2 监听运行(有界同步;路线 B):
/// 1. getter 租约 + 锚点核验(wrapper 身份);
/// 2. 进程内 SC 扫描 → 活 Manager;
/// 3. 读 B0 = *(M+0x170) → 原子写伪节点 → 观测窗口;
/// 4. 验证后写回(座位被 QQ 覆写则不回写,如实记录)→ drain 环。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`G2Ctx`]。
pub unsafe fn g2_listen_run(ctx: *const G2Ctx) -> u32 {
    // SAFETY: ctx 由 loader 写入。
    let (wrapper_base, report_path, observe_ms) =
        unsafe { ((*ctx).wrapper_base, (*ctx).report_path, (*ctx).observe_ms) };
    let observe_ms = observe_ms.min(120_000).max(500);

    // wrapper 身份核验(getter 租约 + 锚点;沿用 v1 机械)。
    let getter: GetterFn = unsafe { core::mem::transmute(wrapper_base + GETTER_RVA) };
    let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
    // SAFETY: getter 契约(R2 §4)。
    let ret = unsafe { getter(&mut pair as *mut ServicePair) };
    if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
        stage(report_path, "g2_adopt", false, "getter failed");
        return ERR_NO_OBSERVER;
    }
    let vptr = unsafe { core::ptr::read_volatile(pair.obj as *const u64) };
    if (vptr as usize).wrapping_sub(wrapper_base) != ANCHOR_SVC_VTBL_RVA {
        stage(report_path, "g2_adopt", false, "anchor mismatch");
        return ERR_SWAP_STATE;
    }
    stage(report_path, "g2_adopt", true, &format!("svc={}", pair.obj as u64));

    // 活 Manager 定位(SC+0x150 正路)。
    stage(report_path, "g2_scan", true, "SC scan begin (RPM-self)");
    let Some((manager, sc_n)) = locate_manager(wrapper_base) else {
        stage(report_path, "g2_manager", false, "SC scan found no verified Manager");
        return ERR_NO_MANAGER;
    };
    stage(report_path, "g2_scan", true, "SC scan done");
    let m_vptr = safe_read_qword(manager).unwrap_or(0);
    stage(
        report_path,
        "g2_manager",
        true,
        &format!(
            "manager={} vptr={} ({}) sc_candidates={sc_n}",
            manager as u64,
            hex64(m_vptr),
            if m_vptr == (wrapper_base + MANAGER_VTBL_RVA) as u64 {
                "raw"
            } else {
                "shim(堆垫片vtable)"
            }
        ),
    );

    // 容器快照 + 交换。
    let end_node = (manager + FIELD_BEGIN_NODE + 8) as u64;
    let begin_slot = (manager + FIELD_BEGIN_NODE) as *mut AtomicU64;
    let b0 = safe_read_qword(manager + FIELD_BEGIN_NODE).unwrap_or(0);
    if b0 == 0 {
        stage(report_path, "g2_tree", false, "begin_node unreadable/zero (Manager freed?)");
        return ERR_NO_MANAGER;
    }
    let root = safe_read_qword(end_node as usize).unwrap_or(0);
    let size = safe_read_qword(end_node as usize + 8).unwrap_or(0);
    stage(
        report_path,
        "g2_tree",
        true,
        &format!("b0={} end={end_node:#x} root={root:#x} size={size}", hex64(b0)),
    );
    let (ours_obj, our_node) = init_object_and_node();
    // SAFETY: our_node 指向 OUR_NODE(5 qword);仅初始化线程写。
    unsafe {
        *our_node = 0; // left
        *our_node.add(1) = b0; // right = B0(空树时 = end_node,同样成立)
        *our_node.add(2) = 0; // parent
        *our_node.add(3) = 1; // is_black
        *our_node.add(4) = ours_obj as u64; // value.first = 监听器
    }
    let prev = unsafe { (*begin_slot).swap(our_node as u64, Ordering::AcqRel) };
    stage(
        report_path,
        "g2_swap",
        prev == b0,
        &format!("ours={:x} prev={}", our_node as usize, hex64(prev)),
    );

    // 观测窗口。
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(observe_ms as u64);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // 恢复(仅当座位仍是我们的;被 QQ 覆写则不回写 stale B0)。
    let cur = safe_read_qword(manager + FIELD_BEGIN_NODE).unwrap_or(0);
    if cur == our_node as u64 {
        unsafe { (*begin_slot).store(b0, Ordering::Release) };
        stage(report_path, "g2_restore", true, "B0 restored");
    } else {
        stage(
            report_path,
            "g2_restore",
            false,
            &format!("begin changed during window: {} (B0 not rewritten)", hex64(cur)),
        );
    }

    // drain 环 → captured fixture。
    let r = ring();
    let q = r.items.lock().unwrap();
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(report_path_str(report_path)) {
        for (i, (a, b)) in q.iter().enumerate() {
            let _ = writeln!(f, "{{\"kind\":\"msg_head\",\"i\":{},\"q0\":{a},\"q1\":{b}}}", i / 4);
        }
        let _ = writeln!(
            f,
            "{{\"kind\":\"summary\",\"pushed\":{},\"dropped\":{}}}",
            r.pushed.load(Ordering::Relaxed),
            r.dropped.load(Ordering::Relaxed)
        );
    }
    drop(q);
    stage(report_path, "g2_done", true, &format!("captured={}", r.pushed.load(Ordering::Relaxed)));
    OK
}

/// getter 输出 pair(与 g1 同几何;避免跨模块私有依赖)。
#[repr(C)]
#[derive(Clone, Copy)]
struct ServicePair {
    obj: *mut c_void,
    ctrl: *mut c_void,
}

/// ===== 路线 A(弃用,保留供回滚/对照)=====
///
/// +0x140 单观察者指针交换(v1 实现)。2026-10-08 深夜起不再被
/// [`g2_listen_run`] 选择:活 Manager 上该字段为 0(陈旧对象亦然),且路线 B
/// 无转发负担。保留原因:若未来观测到 o0 非空,本路线是最小耦合备份。
///
/// 原设计(§2b):OnRecv 在树遍历后通知 `*(M+0x140)` 的单观察者
/// `obs->vtbl[1](obs, &msg_pair)`;交换为我们的对象并转发 O0。
#[cfg(test)]
mod route_a_deprecated {
    use super::*;

    pub const FIELD_OBSERVER: usize = 0x140;

    /// O0(原观察者)——交换期间由本模块独占写。
    pub static O0: std::sync::atomic::AtomicPtr<c_void> =
        std::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

    /// slot1 处理器:捕获 msg 对象头 → 原样转发 O0。
    pub unsafe extern "system" fn g2_slot1(_this: *mut c_void, msg_pair: *mut c_void) {
        // SAFETY: 边界内只做有界内存读 + ring 拷贝 + 转发;panic 隔离。
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !msg_pair.is_null() {
                let pair = *(msg_pair as *const [u64; 2]);
                let obj = pair[0] as usize;
                if obj != 0 {
                    let mut head = [0u64; 8];
                    for (i, h) in head.iter_mut().enumerate() {
                        // SAFETY: QQ 构造的 msg 对象;读失败保持 0。
                        *h = unsafe { core::ptr::read_volatile((obj + i * 8) as *const u64) };
                    }
                    let r = ring();
                    let mut q = r.items.lock().unwrap();
                    if q.len() < r.cap {
                        for c in 0..4 {
                            q.push((head[c * 2], head[c * 2 + 1]));
                        }
                        r.pushed.fetch_add(4, Ordering::Relaxed);
                    } else {
                        r.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let o0 = O0.load(Ordering::Acquire);
            if !o0.is_null() {
                let vptr = unsafe { core::ptr::read_volatile(o0 as *const usize) };
                let slot = unsafe { core::ptr::read_volatile((vptr + 8) as *const usize) };
                let f: unsafe extern "system" fn(*mut c_void, *mut c_void) =
                    unsafe { core::mem::transmute(slot) };
                unsafe { f(o0, msg_pair) };
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex as SyncMutex;

    /// libc++ __tree_next 的忠实模拟(45e5 字节解码;nil=nullptr)。
    /// node = [left@0, right@8, parent@0x10];值区从 +0x20 起(此处用数组模拟)。
    fn tree_next(n: *mut u64) -> *mut u64 {
        // SAFETY: 测试构造的节点几何。
        unsafe {
            let m = *n.add(1) as *mut u64; // right
            if !m.is_null() {
                let mut x = m;
                while *x != 0 {
                    x = *x as *mut u64; // left
                }
                return x;
            }
            let mut cur = n;
            let mut p = *cur.add(2) as *mut u64; // parent
            loop {
                if p.is_null() {
                    // libc++ 在头节点处必返回;测试树保证 parent 链到头。
                    return p;
                }
                if *p as *mut u64 == cur {
                    return p; // cur 是 parent 的左子 → 后继 = parent
                }
                cur = p;
                p = *cur.add(2) as *mut u64;
            }
        }
    }

    /// OnRecv 推送循环等价(起点 *(begin_slot),终点 end_node)。
    fn onrecv_walk(begin_slot: *mut u64, end_node: *mut u64, visit: &mut Vec<usize>) {
        // SAFETY: 测试构造。
        unsafe {
            let mut n = *begin_slot as *mut u64;
            while n != end_node {
                // notify(*(n+0x20)) —— 测试只记录访问序。
                let listener = *n.add(4) as usize;
                visit.push(listener);
                n = tree_next(n);
            }
        }
    }

    /// 建节点:[left, right, parent, is_black, listener]。
    fn node(left: usize, right: usize, parent: usize, listener: usize) -> Box<[u64; 5]> {
        Box::new([left as u64, right as u64, parent as u64, 1, listener as u64])
    }

    /// QQ 假监听器:被通知时记录自身 id。
    struct QQ {
        notified: Arc<SyncMutex<Vec<usize>>>,
    }

    #[test]
    fn route_b_swap_preserves_order_and_restores() {
        // 树:header(end_node)root=5;5 为根,3/8 为子;中序 = 3,5,8。
        let end = Box::leak(Box::new([0u64; 5])); // end_node:left=root@0
        let n3 = Box::leak(node(0, 0, end.as_mut_ptr() as usize, 0x3000));
        let n5 = Box::leak(node(n3.as_mut_ptr() as usize, 0, 0, 0x5000));
        let n8 = Box::leak(node(0, 0, n5.as_mut_ptr() as usize, 0x8000));
        // 修几何:root=5 → end.left=5;5.left=3,5.right=8;3.parent=5,8.parent=5。
        end[0] = n5.as_mut_ptr() as u64;
        n5[0] = n3.as_mut_ptr() as u64;
        n5[1] = n8.as_mut_ptr() as u64;
        n3[2] = n5.as_mut_ptr() as u64;
        n8[2] = n5.as_mut_ptr() as u64;
        // libc++:begin_node = 最左 = 3;header 左子 = root(见上)。
        let begin_slot = Box::leak(Box::new(n3.as_mut_ptr() as u64));
        // parent 指针:libc++ 最左.parent 指向祖先链,这里 3.parent=5 够用;
        // 根 5.parent=0(无头父),爬升路径测试含 end 终止 —— 5 的后继:
        // 5.right=8≠0 → tree_min(8)=8。8 后继:8.right=0 → 爬 8.parent=5;
        // *5 != 8(5.left=3)→ cur=5, p=5.parent=0 → 测试树返回 null?? 
        // libc++ 真树:根.parent = &end_node(头节点!)。补上:5.parent=end。
        n5[2] = end.as_mut_ptr() as u64;
        // 8 的后继:爬到 5,*5==3≠8 → cur=5,p=end;*end==5(root)==cur → 返回 end ✓
        // (5 是 end 的"左子" —— libc++ 头节点几何约定)。

        // 未挂钩的基线遍历。
        let mut base = Vec::new();
        onrecv_walk(begin_slot, end.as_mut_ptr() as *mut u64, &mut base);
        assert_eq!(base, vec![0x3000, 0x5000, 0x8000], "基线中序 3,5,8");

        // 挂钩:伪节点 right=B0(=3)。
        let (ours_obj, our_node) = init_object_and_node();
        // SAFETY: 测试线程独占初始化。
        unsafe {
            *our_node = 0;
            *our_node.add(1) = n3.as_mut_ptr() as u64;
            *our_node.add(2) = 0;
            *our_node.add(3) = 1;
            *our_node.add(4) = ours_obj as u64;
            let slot = begin_slot as *mut u64 as *mut AtomicU64;
            let prev = (*slot).swap(our_node as u64, Ordering::AcqRel);
            assert_eq!(prev, n3.as_mut_ptr() as u64, "换出原 begin");

            // 挂钩后遍历:我们最先,其后原序不变。
            let mut hooked = Vec::new();
            onrecv_walk(begin_slot, end.as_mut_ptr() as *mut u64, &mut hooked);
            let ours = ours_obj as usize;
            assert_eq!(hooked, vec![ours, 0x3000, 0x5000, 0x8000], "伪节点最先 + 原序保持");

            // 恢复对称。
            let cur = (*slot).load(Ordering::Acquire);
            assert_eq!(cur, our_node as u64);
            (*slot).store(prev, Ordering::Release);
            assert_eq!(
                *begin_slot as usize,
                n3.as_mut_ptr() as usize,
                "恢复后 B0 回位"
            );

            // 垫片路径通知(活实例实证):listener->vtbl slot3(+0x18) 落入捕获环。
            let pushed_before = ring().pushed.load(Ordering::Relaxed);
            let msg_obj = [0xCADEu64, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8];
            let pair = [msg_obj.as_ptr() as u64, 0u64];
            let lptr = ours_obj as *mut usize;
            let vptr = *lptr; // 我们的对象 vptr = OUR_VTABLE
            // SAFETY: vtable[3] = g2_slot6(捕获);测试构造的 pair。
            let f: unsafe extern "system" fn(*mut c_void, *mut c_void) =
                unsafe { core::mem::transmute(core::ptr::read_volatile((vptr + 3 * 8) as *const usize)) };
            unsafe { f(ours_obj, core::ptr::from_ref(&pair).cast::<c_void>().cast_mut()) };
            assert!(
                ring().pushed.load(Ordering::Relaxed) > pushed_before,
                "slot3(+0x18) 通知已入捕获环"
            );
        }
    }

    #[test]
    fn route_b_empty_tree_terminates_after_ours() {
        // 空树:begin = end_node;root = 0。
        let end = Box::leak(Box::new([0u64; 5]));
        let begin_slot = Box::leak(Box::new(end.as_mut_ptr() as u64));
        let (ours_obj, our_node) = init_object_and_node();
        // SAFETY: 测试构造。
        unsafe {
            *our_node = 0;
            *our_node.add(1) = end.as_mut_ptr() as u64; // right = end(空树语义)
            *our_node.add(2) = 0;
            *our_node.add(3) = 1;
            *our_node.add(4) = ours_obj as u64;
            let slot = begin_slot as *mut u64 as *mut AtomicU64;
            let _ = (*slot).swap(our_node as u64, Ordering::AcqRel);

            let mut visit = Vec::new();
            onrecv_walk(begin_slot, end.as_mut_ptr() as *mut u64, &mut visit);
            // succ(ours): right=end≠0 → tree_min(end):end.left=root=0 → 返回 end
            // → 终止。只有我们被通知一次。
            assert_eq!(visit, vec![ours_obj as usize], "空树:仅我们一次后终止");
        }
    }

    #[test]
    fn route_b_concurrent_insert_recomputes_begin_seat_loss_detected() {
        // 窗口内 QQ 插入更小键(成为新最左)→ begin 被覆写为真节点;
        // 我们的恢复路径必须检测到并放弃回写。
        let end = Box::leak(Box::new([0u64; 5]));
        let n5 = Box::leak(node(0, 0, end.as_mut_ptr() as usize, 0x5000));
        end[0] = n5.as_mut_ptr() as u64; // root=5,单节点
        let begin_slot = Box::leak(Box::new(n5.as_mut_ptr() as u64)); // begin=5
        let (_ours_obj, our_node) = init_object_and_node();
        // SAFETY: 测试构造。
        unsafe {
            *our_node = 0;
            *our_node.add(1) = n5.as_mut_ptr() as u64;
            *our_node.add(2) = 0;
            *our_node.add(3) = 1;
            *our_node.add(4) = 0x1234; // value(键为零键:QQ 比较视为空)
            let slot = begin_slot as *mut u64 as *mut AtomicU64;
            let prev = (*slot).swap(our_node as u64, Ordering::AcqRel);

            // QQ 并发插入 3(<5):BST 从 root 插入为新最左,libc++ 重算 begin。
            let n3 = Box::leak(node(0, 0, n5.as_mut_ptr() as usize, 0x3000));
            n5[0] = n3.as_mut_ptr() as u64;
            *begin_slot = n3.as_mut_ptr() as u64;

            // 恢复路径:cur != ours → 不回写。
            let cur = (*slot).load(Ordering::Acquire);
            assert_ne!(cur, our_node as u64, "座位已被 QQ 覆写");
            // 不执行 store(prev) —— 分支与运行时一致(检测即放弃)。
            assert_eq!(*begin_slot as usize, n3.as_mut_ptr() as usize);
            let _ = prev;
        }
    }
}
