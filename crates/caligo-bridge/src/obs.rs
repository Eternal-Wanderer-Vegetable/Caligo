//! 只读运行时观测(EXP-K1-02,K1 第 2 条:确认账号/会话入口候选的注册面)。
//!
//! 原理(来自对本机冻结 QQNT.dll 的静态解码,见 docs/research/native-entry-contract §4):
//! - QQNT.dll 导出的 `node::binding::get_linked_module` 开头用
//!   `mov rsi,[rip+disp32]` 载入 node_module 已注册模块链表头(`node_module::_linked`);
//! - `qq_magic_napi_register(napi_module*)` 把 napi 模块包装成 node_module
//!   (nm_modname = napi_module->nm_modname,即 JS 可见模块名)后走同一个注册链表;
//! - 因此遍历该链表即可取得**全部已注册原生模块的 JS 名字**,无须调用任何 QQ 代码。
//!
//! 安全边界:
//! - 只读自身进程内存(bridge 已运行在宿主进程内);
//! - 不调用任何 QQ/Node 导出函数、不写宿主内存、不安装钩挂;
//! - 唯一写动作是观测报告文件(路径由 loader 提供);
//! - 链表头必须落在 QQNT.dll 映像范围内,否则放弃遍历并记录 note;
//! - 遍历上限 4096 节点并做环检测;注册/注销并发竞争(QM_F_DELETEME 类)导致的
//!   读取窗口风险如实登记为本方法限制。

use serde::{Deserialize, Serialize};

/// node_module 字段偏移(本机 QQNT.dll 实证,见 get_linked_module 解码)。
pub mod node_module_offset {
    pub const VERSION: usize = 0x00;
    pub const FLAGS: usize = 0x04;
    pub const FILENAME: usize = 0x10;
    pub const REGISTER_FUNC: usize = 0x18;
    pub const CONTEXT_REGISTER_FUNC: usize = 0x20;
    pub const MODNAME: usize = 0x28;
    pub const PRIV: usize = 0x30;
    pub const LINK: usize = 0x38;
}

/// 观测行为常量。
mod limits {
    /// 链表遍历节点数上限。
    pub const MAX_NODES: usize = 4096;
    /// 单个字符串拷贝上限(字节)。
    pub const MAX_STRING: usize = 512;
    /// get_linked_module 开头模式扫描窗口(字节)。
    pub const HEAD_SCAN_WINDOW: usize = 32;
    /// 注册回调代码转储上限(字节)。
    pub const CALLBACK_DUMP_BYTES: usize = 4096;
}

/// Node node_module.nm_flags 的已实证位。其余位保留原值输出,不解释。
pub mod nm_flags {
    /// NM_F_LINKED(get_linked_module 源码 `test byte [rsi+4], 2` 实证)。
    pub const LINKED: u32 = 0x2;
    /// qq_magic_napi_register 注册时 `or ecx, 8` 加入的位(语义待查,原样记录)。
    pub const QQ_NAPI: u32 = 0x8;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QqntObs {
    pub base: usize,
    pub image_size: u32,
    pub linked_head_va: usize,
    pub linked_head_rva: u32,
    pub get_linked_module_rva: u32,
    pub qq_magic_napi_register_rva: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservedNodeModule {
    pub name: String,
    pub filename: Option<String>,
    pub flags: u32,
    pub version: i32,
    pub linked_binding: bool,
    pub napi_registered: bool,
}

/// 对选定模块的注册回调捕获(只读):地址、所属映像、代码字节转储。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallbackCapture {
    pub module_name: String,
    pub register_func_va: Option<usize>,
    pub context_register_func_va: Option<usize>,
    pub register_func_image: Option<String>,
    pub context_register_func_image: Option<String>,
    /// context_register_func 处的代码字节(hex),上限 [`limits::CALLBACK_DUMP_BYTES`]。
    pub context_register_code_hex: Option<String>,
    pub image_of_module_struct: Option<String>,
    /// node+0x30(nm_priv)指向的 napi_module 结构 VA。
    pub napi_module_va: Option<usize>,
    pub napi_module_image: Option<String>,
    /// napi_module 结构原始字节(hex,0x40)。
    pub napi_module_hex: Option<String>,
    /// napi_module+0x10 的真正注册函数。
    pub napi_register_func_va: Option<usize>,
    pub napi_register_func_image: Option<String>,
    pub napi_register_code_hex: Option<String>,
    /// napi_module+0x18 的模块名字符串。
    pub napi_modname: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default, Deserialize)]
pub struct ObsReport {
    pub protocol_version: u32,
    pub qqnt: Option<QqntObs>,
    pub modules: Vec<ObservedNodeModule>,
    pub callback_captures: Vec<CallbackCapture>,
    /// 本 DLL 实例的自有绑定状态(register.rs;跨实例不可见,链表层另见 modules)。
    pub entry_registered: bool,
    pub entry_fired: bool,
    /// 回调收到的 env 指针值(0 = 未触发)。
    pub entry_env_hint: usize,
    /// wrapper.node 分发表槽位实时内容(F1-R4;版本绑定 manifest 的 wrapper.node 摘要)。
    pub dispatch_slots: Vec<DispatchSlot>,
    /// 活内存方法字节转储(F1-R5;内存域真相,文件不可信)。读取失败的项记空串。
    pub code_dumps: Vec<CodeDump>,
    pub notes: Vec<String>,
}

/// 活内存代码转储(F1-R5)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodeDump {
    /// wrapper.node 内 RVA。
    pub rva: u32,
    /// 转储用途说明。
    pub label: String,
    /// 实际读取的字节数(0 = 页不可读/读取失败)。
    pub len: usize,
    /// 字节十六进制。
    pub hex: String,
}

/// 分发表槽位探测结果(F1-R4)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispatchSlot {
    /// 槽位在 wrapper.node 内的 RVA。
    pub slot_rva: u32,
    /// 槽内实时指针值(0 = 未填充)。
    pub live_ptr: usize,
    /// 指针所属映像(短名;不在已知映像内为 "heap/unknown")。
    pub target_module: String,
    /// 指针在目标映像内的 RVA(heap/unknown 时为 0)。
    pub target_rva: u32,
}

pub fn build_report_json(report: &ObsReport) -> serde_json::Result<String> {
    serde_json::to_string_pretty(report)
}

// --- 自身进程内存的原始读取(只读) ---

/// 页可读性缓存(远程线程单线程使用;thread_local 隔离)。
/// EXP-F1-R4 事故后引入:任何裸读前先做 VirtualQuery 校验,坏地址返回 None
/// 而非触发 AV(QQ 的 crashpad 会全进程接管 AV,导致实例死亡)。
fn page_readable(addr: usize) -> bool {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static PAGE_CACHE: RefCell<HashMap<usize, bool>> = RefCell::new(HashMap::new());
    }
    let page = addr & !0xFFF;
    PAGE_CACHE.with(|c| {
        if let Some(v) = c.borrow().get(&page) {
            return *v;
        }
        // SAFETY: VirtualQuery 为无锁内核查询,只读自身进程页表信息。
        let ok = unsafe {
            let mut mbi: windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION =
                std::mem::zeroed();
            let n = windows_sys::Win32::System::Memory::VirtualQuery(
                addr as *const core::ffi::c_void,
                &mut mbi,
                std::mem::size_of::<windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION>(),
            );
            if n == 0 {
                None
            } else {
                Some(mbi)
            }
        };
        let Some(mbi) = ok else {
            c.borrow_mut().insert(page, false);
            return false;
        };
        // MEM_COMMIT = 0x1000;PAGE_NOACCESS = 0x01;PAGE_GUARD = 0x100。
        const MEM_COMMIT: u32 = 0x1000;
        const PAGE_NOACCESS: u32 = 0x01;
        const PAGE_GUARD: u32 = 0x100;
        let readable = mbi.State == MEM_COMMIT && (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)) == 0;
        c.borrow_mut().insert(page, readable);
        readable
    })
}

/// 查询地址所在页的 (State, Protect),供诊断 note 使用;查询失败返回 None。
fn page_state(addr: usize) -> Option<(u32, u32)> {
    // SAFETY: VirtualQuery 无锁查询,同 page_readable。
    unsafe {
        let mut mbi: windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION =
            std::mem::zeroed();
        let n = windows_sys::Win32::System::Memory::VirtualQuery(
            addr as *const core::ffi::c_void,
            &mut mbi,
            std::mem::size_of::<windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION>(),
        );
        if n == 0 {
            None
        } else {
            Some((mbi.State, mbi.Protect))
        }
    }
}

/// 带页校验的区间读:覆盖 [p, p+len) 的每一页,任一页不可读即失败(不触发 AV)。
///
/// # Safety
///
/// `out` 须指向至少 `len` 字节的可写缓冲。
unsafe fn checked_read(p: usize, out: *mut u8, len: usize) -> bool {
    let start_page = p & !0xFFF;
    let end_page = (p + len - 1) & !0xFFF;
    let mut page = start_page;
    while page <= end_page {
        if !page_readable(page) {
            return false;
        }
        page += 0x1000;
    }
    // SAFETY: 页已校验为可读;校验后瞬间被卸载的窗口由观测 notes 暴露。
    unsafe {
        core::ptr::copy_nonoverlapping(p as *const u8, out, len);
    }
    true
}

unsafe fn rd_u8(p: usize) -> Option<u8> {
    let mut b = 0u8;
    // SAFETY: 单字节读,页校验由 checked_read 完成。
    unsafe { checked_read(p, &mut b, 1).then_some(b) }
}

unsafe fn rd_u16(p: usize) -> Option<u16> {
    let mut b = [0u8; 2];
    // SAFETY: 两字节读,页校验由 checked_read 完成。
    unsafe { checked_read(p, b.as_mut_ptr(), 2) }.then(|| u16::from_le_bytes(b))
}

unsafe fn rd_u32(p: usize) -> Option<u32> {
    let mut b = [0u8; 4];
    // SAFETY: 四字节读,页校验由 checked_read 完成。
    unsafe { checked_read(p, b.as_mut_ptr(), 4) }.then(|| u32::from_le_bytes(b))
}

unsafe fn rd_i32(p: usize) -> Option<i32> {
    rd_u32(p).map(|v| v as i32)
}

unsafe fn rd_usize(p: usize) -> Option<usize> {
    let mut b = [0u8; 8];
    // SAFETY: 八字节读,页校验由 checked_read 完成。
    unsafe { checked_read(p, b.as_mut_ptr(), 8) }.then(|| usize::from_le_bytes(b))
}

/// 读取 NUL 结尾字节串(上限 [`limits::MAX_STRING`]),lossy UTF-8。
unsafe fn read_cstring(ptr: usize) -> Option<String> {
    let mut buf = Vec::with_capacity(64);
    for i in 0..limits::MAX_STRING {
        let b = rd_u8(ptr + i)?;
        if b == 0 {
            return Some(String::from_utf8_lossy(&buf).into_owned());
        }
        buf.push(b);
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

// --- PE 导出表解析(自身进程内,映像基址) ---

/// 在映像内按导出名解析函数地址。失败返回 None(报告为 note,不猜测)。
/// `pub(crate)`:intr(WU3)在远程线程内复用同一裸读导出解析。
pub(crate) unsafe fn export_addr(image_base: usize, name: &str) -> Option<usize> {
    let e_lfanew = rd_u32(image_base + 0x3C)? as usize;
    let pe = image_base + e_lfanew;
    if rd_u32(pe)? != 0x0000_4550 {
        return None; // "PE\0\0"
    }
    let opt = pe + 24;
    let magic = rd_u16(opt)?;
    let data_dir = match magic {
        0x20B => opt + 112,
        0x10B => opt + 96,
        _ => return None,
    };
    let export_rva = rd_u32(data_dir)? as usize;
    let export_size = rd_u32(data_dir + 4)? as usize;
    if export_rva == 0 || export_size == 0 || export_size > 16 * 1024 * 1024 {
        return None;
    }
    let dir = image_base + export_rva;
    let number_of_names = rd_u32(dir + 24)? as usize;
    let addr_names = image_base + rd_u32(dir + 32)? as usize;
    let addr_ordinals = image_base + rd_u32(dir + 36)? as usize;
    let addr_functions = image_base + rd_u32(dir + 28)? as usize;
    if number_of_names > 100_000 {
        return None;
    }
    for i in 0..number_of_names {
        let name_rva = rd_u32(addr_names + i * 4)? as usize;
        let ordinal = usize::from(rd_u16(addr_ordinals + i * 2)?);
        let cand = read_cstring(image_base + name_rva)?;
        if cand == name {
            let fn_rva = rd_u32(addr_functions + ordinal * 4)? as usize;
            // 转发器(函数 RVA 落在导出目录内)对本用途无意义,视为未找到。
            if fn_rva >= export_rva && fn_rva < export_rva + export_size {
                return None;
            }
            return Some(image_base + fn_rva);
        }
    }
    None
}

/// 模式扫描:get_linked_module 开头 `48 8B 35 disp32`(mov rsi,[rip+d32])或
/// `48 8B 05 disp32`(mov rax,[rip+d32])。返回模式在缓冲区中的起始下标,
/// 供调用方计算 RIP 相对目标。纯函数,单测覆盖。
fn scan_head_pattern(bytes: &[u8]) -> Option<usize> {
    (0..bytes.len().saturating_sub(7)).find(|&i| {
        bytes[i] == 0x48 && bytes[i + 1] == 0x8B && (bytes[i + 2] == 0x35 || bytes[i + 2] == 0x05)
    })
}

unsafe fn rd_i32_bytes(p: usize) -> Option<i32> {
    rd_i32(p)
}

// --- 映像范围与回调捕获 ---

struct ImageRange {
    base: usize,
    size: u32,
    path: String,
}

/// 当前进程的映像列表(基址/大小/路径)。只读快照。
#[allow(dead_code)]
unsafe fn collect_images() -> Vec<ImageRange> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    };
    let pid = windows_sys::Win32::System::Threading::GetCurrentProcessId();
    let mut out = Vec::new();
    // SAFETY: Toolhelp 只读快照;句柄在退出前关闭。
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut ment: MODULEENTRY32W = std::mem::zeroed();
        ment.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        if Module32FirstW(snap, &mut ment) != 0 {
            loop {
                out.push(ImageRange {
                    base: ment.modBaseAddr as usize,
                    size: ment.modBaseSize,
                    path: u16sz(&ment.szExePath),
                });
                if Module32NextW(snap, &mut ment) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

fn image_of(images: &[ImageRange], va: usize) -> Option<&ImageRange> {
    images
        .iter()
        .find(|img| va >= img.base && va < img.base + img.size as usize)
}

fn short_image_name(img: &ImageRange) -> String {
    img.path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&img.path)
        .to_string()
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

#[allow(dead_code)]
fn u16sz(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 读映像代码字节(全部可读才返回,部分可读返回 None)。
unsafe fn read_code(va: usize, len: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(len);
    for i in 0..len {
        buf.push(rd_u8(va + i)?);
    }
    Some(buf)
}

// --- 观测主体 ---

/// observe_qqnt 的返回:(QQNT 元数据, 已注册模块, 回调捕获目标, notes)。
type QqntObservation = (
    Option<QqntObs>,
    Vec<ObservedNodeModule>,
    Vec<(String, usize)>,
    Vec<String>,
);

unsafe fn observe_qqnt_at(qqnt_base: usize) -> QqntObservation {
    let mut notes = Vec::new();
    let mut targets: Vec<(String, usize)> = Vec::new();
    let base = qqnt_base;
    if base == 0 {
        notes.push("QQNT base not provided (loader-side lookup failed)".to_string());
        return (None, Vec::new(), targets, notes);
    }
    const CAPTURE_NAMES: [&str; 2] = ["major", "QQNT"];

    // 映像大小(SizeOfImage @ optional header +56)。
    let e_lfanew = match rd_u32(base + 0x3C) {
        Some(v) => v as usize,
        None => {
            notes.push("QQNT.dll DOS header unreadable".to_string());
            return (None, Vec::new(), targets.clone(), notes);
        }
    };
    let pe = base + e_lfanew;
    let opt = pe + 24;
    let image_size = match rd_u32(opt + 56) {
        Some(v) => v,
        None => {
            notes.push("QQNT.dll optional header unreadable".to_string());
            return (None, Vec::new(), targets.clone(), notes);
        }
    };
    let in_image = |va: usize| va >= base && va < base + image_size as usize;

    let glm_va = match export_addr(
        base,
        "?get_linked_module@binding@node@@YAPEAUnode_module@2@PEBD@Z",
    ) {
        Some(v) => v,
        None => {
            notes.push("get_linked_module export not found".to_string());
            return (None, Vec::new(), targets.clone(), notes);
        }
    };
    let magic_rva = match export_addr(base, "qq_magic_napi_register") {
        Some(v) => (v - base) as u32,
        None => {
            notes.push("qq_magic_napi_register export not found".to_string());
            0
        }
    };

    // 模式扫描窗口字节。
    let mut window = [0u8; limits::HEAD_SCAN_WINDOW];
    let mut ok = true;
    for (i, slot) in window.iter_mut().enumerate() {
        match rd_u8(glm_va + i) {
            Some(b) => *slot = b,
            None => {
                ok = false;
                break;
            }
        }
    }
    if !ok {
        notes.push("get_linked_module bytes unreadable".to_string());
        return (None, Vec::new(), targets.clone(), notes);
    }
    let Some(insn) = scan_head_pattern(&window) else {
        notes.push("linked-head pattern not found (binary changed?)".to_string());
        return (None, Vec::new(), targets.clone(), notes);
    };
    let disp = rd_i32_bytes(glm_va + insn + 3).unwrap_or(0) as isize;
    let head_va = glm_va + insn + 7;
    let head = (head_va as isize + disp) as usize;
    if !in_image(head) {
        notes.push(format!(
            "linked head {head:#x} outside QQNT image — refusing to walk"
        ));
        return (None, Vec::new(), targets.clone(), notes);
    }

    let mut modules = Vec::new();
    let mut visited: Vec<usize> = Vec::new();
    let mut node = match rd_usize(head) {
        Some(v) => v,
        None => {
            notes.push("linked head unreadable".to_string());
            return (
                Some(QqntObs {
                    base,
                    image_size,
                    linked_head_va: head,
                    linked_head_rva: (head - base) as u32,
                    get_linked_module_rva: (glm_va - base) as u32,
                    qq_magic_napi_register_rva: magic_rva,
                }),
                modules,
                targets,
                notes,
            );
        }
    };
    while node != 0 {
        if visited.len() >= limits::MAX_NODES {
            notes.push(format!("node cap {} reached", limits::MAX_NODES));
            break;
        }
        if visited.contains(&node) {
            notes.push(format!("cycle detected at {node:#x}"));
            break;
        }
        visited.push(node);

        let version = rd_i32(node + node_module_offset::VERSION).unwrap_or(0);
        let flags = rd_u32(node + node_module_offset::FLAGS).unwrap_or(0);
        let modname_ptr = rd_usize(node + node_module_offset::MODNAME).unwrap_or(0);
        let filename_ptr = rd_usize(node + node_module_offset::FILENAME).unwrap_or(0);
        let name = if modname_ptr != 0 {
            read_cstring(modname_ptr).unwrap_or_else(|| "<unreadable>".to_string())
        } else {
            "<null>".to_string()
        };
        let filename = if filename_ptr != 0 {
            read_cstring(filename_ptr)
        } else {
            None
        };
        if CAPTURE_NAMES.contains(&name.as_str()) {
            targets.push((name.clone(), node));
        }
        modules.push(ObservedNodeModule {
            name,
            filename,
            flags,
            version,
            linked_binding: flags & nm_flags::LINKED != 0,
            napi_registered: flags & nm_flags::QQ_NAPI != 0,
        });

        match rd_usize(node + node_module_offset::LINK) {
            Some(next) => node = next,
            None => {
                notes.push(format!("link pointer unreadable at {node:#x}"));
                break;
            }
        }
    }

    (
        Some(QqntObs {
            base,
            image_size,
            linked_head_va: head,
            linked_head_rva: (head - base) as u32,
            get_linked_module_rva: (glm_va - base) as u32,
            qq_magic_napi_register_rva: magic_rva,
        }),
        modules,
        targets,
        notes,
    )
}

/// 对捕获目标的 node_module 结构读取注册回调指针,解析所属映像并转储
/// context_register_func 的代码字节。全部只读。
///
/// 供 envrun 等模块使用的两个只读助手:
/// - [`linked_targets`]:返回捕获目标 (name, node_va) 列表;
/// - [`read_node_field`]:读取 node 结构中偏移处的指针字段。
pub fn linked_targets() -> Vec<(String, usize)> {
    // SAFETY: 只读遍历,详见 observe_qqnt_at。基址经 GetModuleHandleW 解析
    //(此助手仅供进程内 envrun/测试路径使用,不在远程线程调用)。
    unsafe {
        let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
        let h = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr());
        let base = if h.is_null() { 0 } else { h as usize };
        let (_, _, targets, _) = observe_qqnt_at(base);
        targets
    }
}

/// 读取 node 结构中指定偏移处的 8 字节字段(只读)。
pub fn read_node_field(node_va: usize, offset: usize) -> Option<usize> {
    // SAFETY: node_va 来自运行时链表数据,偏移为已实证的布局偏移。
    unsafe { rd_usize(node_va + offset) }
}

unsafe fn capture_callbacks(
    targets: &[(String, usize)],
    images: &[ImageRange],
    notes: &mut Vec<String>,
) -> Vec<CallbackCapture> {
    let mut out = Vec::new();
    for (name, node) in targets {
        let register_func_va = rd_usize(node + node_module_offset::REGISTER_FUNC);
        let context_register_func_va = rd_usize(node + node_module_offset::CONTEXT_REGISTER_FUNC);
        let reg_img = register_func_va.and_then(|va| image_of(images, va).map(short_image_name));
        let ctx_img =
            context_register_func_va.and_then(|va| image_of(images, va).map(short_image_name));
        let node_img = image_of(images, *node).map(short_image_name);
        let code_hex = match context_register_func_va {
            Some(va) if va != 0 => match read_code(va, limits::CALLBACK_DUMP_BYTES) {
                Some(bytes) => Some(to_hex(&bytes)),
                None => {
                    notes.push(format!("{name}: context_register_func code unreadable"));
                    None
                }
            },
            _ => None,
        };

        // nm_priv(node+0x30)= qq_magic 注册路径存放的 napi_module*。
        // napi_module 布局(napi.h):version@0,flags@4,filename@8,
        // register_func@0x10,modname@0x18。
        let napi_module_va = rd_usize(node + node_module_offset::PRIV);
        let (
            napi_module_image,
            napi_module_hex,
            napi_register_func_va,
            napi_register_func_image,
            napi_register_code_hex,
            napi_modname,
        ) = match napi_module_va {
            Some(nva) if nva != 0 => {
                let img = image_of(images, nva).map(short_image_name);
                let raw = read_code(nva, 0x40).map(|b| to_hex(&b));
                let reg_va = rd_usize(nva + 0x10);
                let reg_img = reg_va.and_then(|va| image_of(images, va).map(short_image_name));
                let reg_code = match reg_va {
                    Some(va) if va != 0 => {
                        read_code(va, limits::CALLBACK_DUMP_BYTES).map(|b| to_hex(&b))
                    }
                    _ => None,
                };
                let modname =
                    rd_usize(nva + 0x18).and_then(|p| if p != 0 { read_cstring(p) } else { None });
                (img, raw, reg_va, reg_img, reg_code, modname)
            }
            _ => (None, None, None, None, None, None),
        };

        out.push(CallbackCapture {
            module_name: name.clone(),
            register_func_va,
            context_register_func_va,
            register_func_image: reg_img,
            context_register_func_image: ctx_img,
            context_register_code_hex: code_hex,
            image_of_module_struct: node_img,
            napi_module_va,
            napi_module_image,
            napi_module_hex,
            napi_register_func_va,
            napi_register_func_image,
            napi_register_code_hex,
            napi_modname,
        });
    }
    out
}

/// wrapper.node 分发表槽位(F1-R4;由 0x3E8AA/0x3E8C0/0x3E906/0x3E89A 转发器实证)。
const DISPATCH_SLOT_RVAS: [u32; 4] = [0x4910_0088, 0x4910_00A8, 0x4910_00B8, 0x4910_00C0];

/// obs v2 上下文:由加载器侧解析并远程传入。远程线程内**不调用任何加载器锁
/// 敏感 API**(GetModuleHandleW/Toolhelp),只做裸内存读 + 写文件。
/// (缺陷记录:EXP-F1-R4 中远程线程内 GetModuleHandleW 与宿主模块加载竞争,
/// 观测线程挂起且实例 38648 死亡——见 identity-invalidation-observations。)
#[repr(C)]
pub struct ObsCtx {
    pub wrapper_base: usize,
    pub qqnt_base: usize,
    pub major_base: usize,
    /// 远程已写入的 UTF-16 报告路径缓冲地址。
    pub report_path: usize,
}

/// 从模块基址裸读 PE 头取得 SizeOfImage(只读;失败返回 0)。
unsafe fn read_image_size(base: usize) -> usize {
    let Some(e_lfanew) = rd_u32(base + 0x3C) else {
        return 0;
    };
    rd_u32(base + e_lfanew as usize + 24 + 56).unwrap_or(0) as usize
}

/// 读取 wrapper.node 分发表槽位的实时指针并解析目标归属(只读裸读)。
unsafe fn probe_dispatch_slots(
    wrapper_base: usize,
    images: &[ImageRange],
    notes: &mut Vec<String>,
) -> Vec<DispatchSlot> {
    let mut out = Vec::new();
    if wrapper_base == 0 {
        notes.push("wrapper base not provided (loader-side lookup failed)".to_string());
        return out;
    }
    // 诊断 v3.1:活进程 PE 头核对 + 槽位页属性,定位"文件态 RVA 合法但运行时未提交"的矛盾。
    // SAFETY: 均为裸读(页校验在 rd_* 内)。
    unsafe {
        match rd_u32(wrapper_base + 0x3C) {
            Some(elf) => notes.push(format!("wrapper live e_lfanew={elf:#x}")),
            None => notes.push("wrapper live DOS header unreadable".to_string()),
        }
        notes.push(format!(
            "wrapper live SizeOfImage={:#x} (file expects 0x7419000)",
            read_image_size(wrapper_base)
        ));
        match page_state(wrapper_base + 0x4910_0088) {
            Some((state, protect)) => {
                notes.push(format!("slot page state={state:#x} protect={protect:#x}"))
            }
            None => notes.push("slot page VirtualQuery failed".to_string()),
        }
        // 活代码核对:0x3E8AA 处应为 48 85 D2(test rdx,rdx;与文件一致)。
        let mut code = [0u8; 8];
        if checked_read(wrapper_base + 0x3E8AA, code.as_mut_ptr(), 8) {
            notes.push(format!(
                "live code@0x3E8AA={code:02X?} (file: [48,85,D2,49,89,D0,31,D2])"
            ));
        } else {
            notes.push("live code@0x3E8AA unreadable".to_string());
        }
    }
    for slot_rva in DISPATCH_SLOT_RVAS {
        let live = match rd_usize(wrapper_base + slot_rva as usize) {
            Some(v) => v,
            None => {
                notes.push(format!("slot {slot_rva:#x} unreadable"));
                0
            }
        };
        let (target_module, target_rva) = if live == 0 {
            ("unfilled".to_string(), 0)
        } else {
            match image_of(images, live) {
                Some(img) => (short_image_name(img), (live - img.base) as u32),
                None => ("heap/unknown".to_string(), 0),
            }
        };
        out.push(DispatchSlot {
            slot_rva,
            live_ptr: live,
            target_module,
            target_rva,
        });
    }
    out
}

/// F1-R5 转储目标:(RVA, 标签)。来源:F-1 三轮的文件态接口表/方法清单。
/// 文件与内存可能不一致——本转储就是为取得内存域真相。
const LIVE_DUMP_TARGETS: [(u32, &str); 12] = [
    (0x3BE64, "tbl1_slot0_fwd"),
    (0x3BE6E, "tbl1_slot8_method"),
    (0x3BECC, "tbl1_4arg"),
    (0x3C0C4, "tbl1_check_act"),
    (0x3C110, "tbl1_string_ret"),
    (0x3E89A, "tbl2_m0"),
    (0x3E8AA, "tbl2_m1"),
    (0x3E8C0, "tbl2_m2"),
    (0x3E906, "tbl2_m3"),
    (0x3E916, "tbl2_m4"),
    (0x3EBE2B8, "base_vtable_88b"),
    (0x3EBD728, "derived_vtable_88b"),
];

/// 转储活内存中上述目标的字节(每方法 192 字节,vtable 88 字节;只读)。
unsafe fn dump_live_code(wrapper_base: usize, notes: &mut Vec<String>) -> Vec<CodeDump> {
    let mut out = Vec::new();
    if wrapper_base == 0 {
        return out;
    }
    for (rva, label) in LIVE_DUMP_TARGETS {
        let is_vtable = label.contains("vtable");
        let len = if is_vtable { 88 } else { 192 };
        let mut buf = vec![0u8; len];
        // SAFETY: 页校验读,坏页返回 None。
        let ok = unsafe { checked_read(wrapper_base + rva as usize, buf.as_mut_ptr(), len) };
        if !ok {
            notes.push(format!("live dump {label}@{rva:#x} unreadable"));
            out.push(CodeDump {
                rva,
                label: label.to_string(),
                len: 0,
                hex: String::new(),
            });
            continue;
        }
        out.push(CodeDump {
            rva,
            label: label.to_string(),
            len,
            hex: to_hex(&buf),
        });
    }
    out
}

/// 构建完整观测报告(错误都降级为 note,不 panic)。
/// v2 观测:全部模块基址由加载器侧经 [`ObsCtx`] 传入,本函数与被调链
/// **只做裸内存读**,不触碰加载器锁。报告写盘由调用方完成。
///
/// # Safety
///
/// ctx 内基址必须来自同进程内真实加载的模块;report_path 须指向本进程内
/// 已写入的 NUL 结尾 UTF-16 缓冲。
pub unsafe fn observe_ctx(ctx: &ObsCtx) -> ObsReport {
    let mut report = ObsReport {
        protocol_version: crate::PROTOCOL_VERSION,
        ..Default::default()
    };
    // SAFETY: 只读裸读;基址由加载器侧核对。
    let (qqnt, modules, targets, mut notes) = unsafe { observe_qqnt_at(ctx.qqnt_base) };
    report.qqnt = qqnt;
    report.modules = modules;
    // 由已知基址构造映像表(大小经裸读 PE 头取得),用于指针归属。
    let mut images: Vec<ImageRange> = Vec::new();
    for (base, name) in [
        (ctx.qqnt_base, "QQNT.dll"),
        (ctx.major_base, "major.node"),
        (ctx.wrapper_base, "wrapper.node"),
    ] {
        if base != 0 {
            // SAFETY: 裸读 PE 头。
            let size = unsafe { read_image_size(base) };
            images.push(ImageRange {
                base,
                size: size as u32,
                path: name.to_string(),
            });
        }
    }
    report.dispatch_slots = unsafe { probe_dispatch_slots(ctx.wrapper_base, &images, &mut notes) };
    // F1-R5:活内存方法字节转储(文件不可信,以内存域为准)。
    report.code_dumps = unsafe { dump_live_code(ctx.wrapper_base, &mut notes) };
    if !targets.is_empty() {
        // SAFETY: 回调指针读取 + 映像内代码转储均为只读。
        let captures = unsafe { capture_callbacks(&targets, &images, &mut notes) };
        report.callback_captures = captures;
    }
    let (registered, fired, env_hint) = crate::register::entry_state();
    report.entry_registered = registered;
    report.entry_fired = fired;
    report.entry_env_hint = env_hint;
    report.notes = notes;
    report
}

/// 兼容入口(进程内自解析)。**已弃用于远程线程**:内部 GetModuleHandleW
/// 存在加载器锁竞争(见 ObsCtx 文档),仅供进程内测试使用。
pub fn observe() -> ObsReport {
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    // SAFETY: GetModuleHandleW 只读查询(仅本进程内测试路径)。
    let ctx = unsafe {
        let qqnt =
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide("QQNT.dll").as_ptr());
        let wrapper = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
            wide("wrapper.node").as_ptr(),
        );
        ObsCtx {
            qqnt_base: if qqnt.is_null() { 0 } else { qqnt as usize },
            wrapper_base: if wrapper.is_null() {
                0
            } else {
                wrapper as usize
            },
            major_base: 0,
            report_path: 0,
        }
    };
    // SAFETY: 基址来自本进程真实模块。
    unsafe { observe_ctx(&ctx) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_pattern_scan_finds_mov_rip() {
        // 伪造 get_linked_module 开头:push; push; mov rsi,[rip+0x11111111]。
        let code = [0x56u8, 0x57, 0x48, 0x8B, 0x35, 0x11, 0x11, 0x11, 0x11, 0xC3];
        assert_eq!(scan_head_pattern(&code), Some(2));
        // rax 变体。
        let code2 = [0x48u8, 0x8B, 0x05, 0x22, 0x22, 0x22, 0x22, 0xC3];
        assert_eq!(scan_head_pattern(&code2), Some(0));
        // 无模式。
        assert_eq!(scan_head_pattern(&[0x90u8; 16]), None);
        // 窗口过短。
        assert_eq!(scan_head_pattern(&[0x48u8, 0x8B]), None);
    }

    #[test]
    fn obs_report_json_shape() {
        let report = ObsReport {
            protocol_version: 1,
            entry_registered: true,
            entry_fired: true,
            entry_env_hint: 0x7FF00000,
            qqnt: Some(QqntObs {
                base: 0x180000000,
                image_size: 0xCC8C2B0,
                linked_head_va: 0x180000000 + 0x0C7092F2usize,
                linked_head_rva: 0x0C7092F2,
                get_linked_module_rva: 0x01C911B0,
                qq_magic_napi_register_rva: 0x01C8B450,
            }),
            modules: vec![ObservedNodeModule {
                name: "NodeQQNT".into(),
                filename: Some("D:/x/major.node".into()),
                flags: 8,
                version: -1,
                linked_binding: false,
                napi_registered: true,
            }],
            callback_captures: vec![CallbackCapture {
                module_name: "major".into(),
                register_func_va: None,
                context_register_func_va: Some(0x180012340),
                register_func_image: None,
                context_register_func_image: Some("QQNT.dll".into()),
                context_register_code_hex: Some("4883EC28".into()),
                image_of_module_struct: Some("major.node".into()),
                napi_module_va: Some(0x180045000),
                napi_module_image: Some("major.node".into()),
                napi_module_hex: Some("FFFF0000".into()),
                napi_register_func_va: Some(0x180050000),
                napi_register_func_image: Some("major.node".into()),
                napi_register_code_hex: None,
                napi_modname: Some("major".into()),
            }],
            dispatch_slots: vec![DispatchSlot {
                slot_rva: 0x4910_0088,
                live_ptr: 0x1804123A0,
                target_module: "QQNT.dll".into(),
                target_rva: 0x4123A0,
            }],
            code_dumps: vec![CodeDump {
                rva: 0x3C110,
                label: "tbl1_string_ret".into(),
                len: 4,
                hex: "4883EC28".into(),
            }],
            notes: vec!["sample".into()],
        };
        let back: serde_json::Value =
            serde_json::from_str(&build_report_json(&report).unwrap()).unwrap();
        assert_eq!(back["qqnt"]["linked_head_rva"], 0x0C7092F2u64);
        assert_eq!(back["modules"][0]["name"], "NodeQQNT");
        assert_eq!(back["modules"][0]["napi_registered"], true);
        assert_eq!(back["callback_captures"][0]["module_name"], "major");
        assert_eq!(
            back["callback_captures"][0]["context_register_func_image"],
            "QQNT.dll"
        );
        assert_eq!(back["entry_registered"], true);
        assert_eq!(back["entry_fired"], true);
        assert_eq!(back["entry_env_hint"], 0x7FF00000u64);
        assert_eq!(back["dispatch_slots"][0]["slot_rva"], 0x4910_0088u64);
        assert_eq!(back["dispatch_slots"][0]["target_module"], "QQNT.dll");
        assert_eq!(back["dispatch_slots"][0]["target_rva"], 0x4123A0u64);
        assert_eq!(back["code_dumps"][0]["rva"], 0x3C110u64);
        assert_eq!(back["code_dumps"][0]["label"], "tbl1_string_ret");
        assert_eq!(back["notes"][0], "sample");
    }

    #[test]
    fn empty_report_is_serializable() {
        let report = ObsReport::default();
        let back: serde_json::Value =
            serde_json::from_str(&build_report_json(&report).unwrap()).unwrap();
        assert_eq!(back["qqnt"], serde_json::Value::Null);
        assert_eq!(back["modules"].as_array().unwrap().len(), 0);
    }
}
