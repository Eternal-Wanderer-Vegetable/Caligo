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

use serde::Serialize;

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QqntObs {
    pub base: usize,
    pub image_size: u32,
    pub linked_head_va: usize,
    pub linked_head_rva: u32,
    pub get_linked_module_rva: u32,
    pub qq_magic_napi_register_rva: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObservedNodeModule {
    pub name: String,
    pub filename: Option<String>,
    pub flags: u32,
    pub version: i32,
    pub linked_binding: bool,
    pub napi_registered: bool,
}

/// 对选定模块的注册回调捕获(只读):地址、所属映像、代码字节转储。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
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
    pub notes: Vec<String>,
}

pub fn build_report_json(report: &ObsReport) -> serde_json::Result<String> {
    serde_json::to_string_pretty(report)
}

// --- 自身进程内存的原始读取(只读) ---

unsafe fn rd_u8(p: usize) -> Option<u8> {
    (p as *const u8).as_ref().map(|v| *v)
}

unsafe fn rd_u16(p: usize) -> Option<u16> {
    (p as *const u16).as_ref().map(|v| v.to_le())
}

unsafe fn rd_u32(p: usize) -> Option<u32> {
    (p as *const u32).as_ref().map(|v| v.to_le())
}

unsafe fn rd_i32(p: usize) -> Option<i32> {
    rd_u32(p).map(|v| v as i32)
}

unsafe fn rd_usize(p: usize) -> Option<usize> {
    (p as *const usize).as_ref().copied()
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
unsafe fn export_addr(image_base: usize, name: &str) -> Option<usize> {
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

unsafe fn observe_qqnt() -> QqntObservation {
    let mut notes = Vec::new();
    let mut targets: Vec<(String, usize)> = Vec::new();
    let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
    let base = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr());
    if base.is_null() {
        notes.push("QQNT.dll not loaded in this process".to_string());
        return (None, Vec::new(), targets.clone(), notes);
    }
    let base = base as usize;
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
    // SAFETY: 只读遍历,详见 observe_qqnt。
    let (_, _, targets, _) = unsafe { observe_qqnt() };
    targets
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

/// 构建完整观测报告(错误都降级为 note,不 panic)。
pub fn observe() -> ObsReport {
    let mut report = ObsReport {
        protocol_version: crate::PROTOCOL_VERSION,
        ..Default::default()
    };
    // SAFETY: 全部为自身进程内只读读取;指针均来自已校验的映像/Node 注册链表。
    let (qqnt, modules, targets, mut notes) = unsafe { observe_qqnt() };
    report.qqnt = qqnt;
    report.modules = modules;
    if !targets.is_empty() {
        // SAFETY: 同上,回调指针读取 + 映像内代码转储均为只读。
        let images = unsafe { collect_images() };
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
