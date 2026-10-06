//! 自有 linked binding 注册机制(EXP-K2-00,方案 b/a 验证)。
//!
//! 机制(见 docs/research/js-context-acquisition.md):
//! 1. 构造静态 `napi_module`(镜像本机 major 的实测模式:nm_version=1、
//!    nm_flags=2=NM_F_LINKED;qq_magic 会在 node.flags 上再 |8,语义未知、如实记录);
//! 2. 调用 QQNT.dll 导出的 `qq_magic_napi_register` 把它挂入 node_module 注册链表;
//! 3. 回调 [`entry_register`] 一旦被 Node 以 (env, exports) 调用,只原子记录 env 指针
//!    并原样返回 exports——**不做任何 napi 调用、不做 IO**,把"急切 or 惰性"的问题
//!    留给观测数据回答。
//!
//! 生命周期:注册不可逆(node_module_register 只有插入),随测试 QQ 进程退出回收。
//! 状态字段(REGISTER_*/ENTRY_*)属于调用注册的那份 DLL 实例;多份 bridge 共驻时
//! 各自独立,链表里会出现多个同名节点,obs 如实呈现。

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// 注册结果码。
pub mod register_code {
    pub const OK: u32 = 0;
    pub const ERR_ALREADY: u32 = 1;
    pub const ERR_NO_QQNT: u32 = 2;
    pub const ERR_NO_MAGIC: u32 = 3;
}

// SAFETY: NapiModule 仅作为不可变静态数据使用;指针指向静态字节串与函数,
// 无内部可变性,跨线程共享安全。
unsafe impl Sync for NapiModule {}
unsafe impl Send for NapiModule {}

/// napi.h `napi_module` 布局(本机实测:version=1,flags,0x08 filename,
/// 0x10 register_func,0x18 modname,0x20 priv,0x28 reserved[4])。
#[repr(C)]
struct NapiModule {
    nm_version: i32,
    nm_flags: u32,
    nm_filename: *const u8,
    nm_register_func:
        Option<unsafe extern "C" fn(env: *mut c_void, exports: *mut c_void) -> *mut c_void>,
    nm_modname: *const u8,
    nm_priv: *mut c_void,
    reserved: [*mut c_void; 4],
}

static ENTRY_FILENAME: &[u8] = b"caligo-bridge-entry\0";
static ENTRY_MODNAME: &[u8] = b"caligo_bridge\0";

static ENTRY_FIRED_ENV: AtomicUsize = AtomicUsize::new(0);
static REGISTER_DONE: AtomicBool = AtomicBool::new(false);

/// Node 请求本绑定时调用(经共享 thunk)。最小实现:记录 env,原样返回 exports。
unsafe extern "C" fn entry_register(env: *mut c_void, exports: *mut c_void) -> *mut c_void {
    ENTRY_FIRED_ENV.store((env as usize) | 1, Ordering::Release);
    exports
}

static ENTRY_NAPI_MODULE: NapiModule = NapiModule {
    nm_version: 1,
    nm_flags: 2, // NM_F_LINKED(get_linked_module 的判定位,本机实证)
    nm_filename: ENTRY_FILENAME.as_ptr(),
    nm_register_func: Some(entry_register),
    nm_modname: ENTRY_MODNAME.as_ptr(),
    nm_priv: std::ptr::null_mut(),
    reserved: [std::ptr::null_mut(); 4],
};

/// 注册自有绑定。重复调用幂等拒绝。
pub fn register_entry() -> u32 {
    if REGISTER_DONE.swap(true, Ordering::AcqRel) {
        return register_code::ERR_ALREADY;
    }
    let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
    // SAFETY: GetModuleHandleW/GetProcAddress 为只读查询;随后调用的是
    // 已完整解码的 qq_magic_napi_register(链表插入,见方案文档)。
    let qqnt =
        unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr()) };
    if qqnt.is_null() {
        return register_code::ERR_NO_QQNT;
    }
    let magic = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            qqnt,
            c"qq_magic_napi_register".as_ptr() as *const u8,
        )
    };
    let Some(magic) = magic else {
        return register_code::ERR_NO_MAGIC;
    };
    let magic: unsafe extern "system" fn(*mut NapiModule) =
        unsafe { std::mem::transmute(magic as usize) };
    unsafe {
        magic(&ENTRY_NAPI_MODULE as *const NapiModule as *mut NapiModule);
    }
    register_code::OK
}

/// (是否已执行注册, 回调是否已被调用, 回调收到的 env 指针值, 0 表示未触发)
pub fn entry_state() -> (bool, bool, usize) {
    let fired = ENTRY_FIRED_ENV.load(Ordering::Acquire);
    (
        REGISTER_DONE.load(Ordering::Acquire),
        fired != 0,
        fired & !1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn napi_module_layout_matches_observed_pattern() {
        // 字段偏移按 napi.h 实测:0x00 version,0x04 flags,0x08 filename,
        // 0x10 register_func,0x18 modname,0x20 priv。
        let base = &ENTRY_NAPI_MODULE as *const NapiModule as usize;
        let fptr = base + 0x10;
        // SAFETY: 读取自有静态结构的字段指针,仅做偏移校验。
        unsafe {
            let reg = fptr as *const Option<
                unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void,
            >;
            assert!((*reg).is_some());
        }
        // modname 指针指向静态字节串,内容校验。
        assert_eq!(ENTRY_NAPI_MODULE.nm_modname, ENTRY_MODNAME.as_ptr());
        let name = unsafe { std::ffi::CStr::from_ptr(ENTRY_NAPI_MODULE.nm_modname as *const i8) };
        assert_eq!(name.to_bytes(), b"caligo_bridge");
    }

    #[test]
    fn entry_callback_records_env_and_returns_exports() {
        let exports = 0x1234usize as *mut c_void;
        // env 指针按对齐约定最低位为 0(低位用作"已触发"标记)。
        let env = 0xABC0usize as *mut c_void;
        let ret = unsafe { entry_register(env, exports) };
        assert_eq!(ret, exports);
        let (_, fired, env_hint) = entry_state();
        assert!(fired);
        assert_eq!(env_hint, 0xABC0);
        // 复位以便其它测试不受污染(仅测试进程)。
        ENTRY_FIRED_ENV.store(0, Ordering::Release);
    }
}
