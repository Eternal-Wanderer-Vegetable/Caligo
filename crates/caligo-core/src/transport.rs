//! K4-D5 Windows 命名管道 transport(计划 §6.4/§7-D5)。
//!
//! 契约:
//! - core 为 server,bridge/控制客户端各自连接独立管道实例;
//! - 管道名含一次性后缀(`generate_token`),`self_id`/名字不构成认证 ——
//!   认证凭据为 OS 随机 32 字节 token,经启动配置交付,不进命令行/日志;
//! - 安全描述符:DACL 仅授予**当前用户 SID** 读写;`PIPE_REJECT_REMOTE_CLIENTS`;
//!   同用户恶意进程不在本阶段强隔离保证内(威胁边界见计划 §6.4);
//! - 全部 I/O 走 OVERLAPPED + 事件;读等待超时用 `CancelIoEx` 取消
//!   (等待失败 ≠ I/O 已终止,取消返回后才允许复用缓冲 —— K3 教训);
//! - `GetNamedPipeClientProcessId` 支持指定进程校验(LAB 由测试传入本进程 PID)。
//!
//! 仅 Windows;非 Windows 下编译为显式 `unsupported` 错误。

#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_BROKEN_PIPE, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED,
    GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{
    AddAccessAllowedAce, GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor,
    SetSecurityDescriptorOwner, TOKEN_QUERY, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, ACL,
    ACL_REVISION, SID, TokenUser,
};
use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED,
    FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

const IO_EVENT_TIMEOUT_MS: u32 = 100;

#[derive(Debug)]
pub enum TransportError {
    Win32 { op: &'static str, code: u32 },
    TimedOut,
    BrokenPipe,
    Unsupported,
    NameTooLong,
}

impl core::fmt::Display for TransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TransportError::Win32 { op, code } => write!(f, "{op} failed: Win32 error {code}"),
            TransportError::TimedOut => write!(f, "io timed out (cancelled)"),
            TransportError::BrokenPipe => write!(f, "pipe broken"),
            TransportError::Unsupported => write!(f, "not supported on this platform"),
            TransportError::NameTooLong => write!(f, "pipe name too long"),
        }
    }
}

impl std::error::Error for TransportError {}

fn w32(op: &'static str) -> TransportError {
    // SAFETY: GetLastError 无副作用。
    TransportError::Win32 { op, code: unsafe { GetLastError() } }
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// OS 随机 32 字节 token(BCryptGenRandom,系统首选 RNG)。
pub fn generate_token() -> Result<[u8; 32], TransportError> {
    let mut buf = [0u8; 32];
    // SAFETY: 缓冲由我们持有;BCRYPT_USE_SYSTEM_PREFERRED_RNG 时句柄为 null。
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(TransportError::Win32 { op: "BCryptGenRandom", code: status as u32 });
    }
    Ok(buf)
}

pub fn token_hex(tok: &[u8; 32]) -> String {
    tok.iter().map(|b| format!("{b:02x}")).collect()
}

/// 安全描述符及其依赖缓冲(SID 与 ACL 必须与 SD 同生存期 —— SD 内是
/// 指针,指向它们;先前的悬空 DACL 正是 CreateNamedPipeW 998 的根因)。
struct UserSecurityDescriptor {
    sd: SECURITY_DESCRIPTOR,
    _sid_buf: Vec<u8>,
    _acl_buf: Vec<u8>,
}

/// 仅当前用户可访问的安全描述符(DACL:当前用户 SID 读写)。
fn current_user_security_descriptor() -> Result<UserSecurityDescriptor, TransportError> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: 全部为文档化只读查询;句柄全程成对关闭。
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(w32("OpenProcessToken"));
        }
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let mut got = 0u32;
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut got);
        CloseHandle(token);
        if ok == 0 {
            return Err(w32("GetTokenInformation"));
        }
        // TOKEN_USER = { SID_AND_ATTRIBUTES { Sid: *mut SID, Attributes } }。
        let sid_ptr = *(buf.as_ptr() as *const *const SID);
        // 布局:ACL 头 + 单 ACE(SID 追加)。取足够大缓冲。
        let acl_len = std::mem::size_of::<ACL>() + 1024;
        let mut acl_buf = vec![0u8; acl_len];
        let acl = acl_buf.as_mut_ptr() as *mut ACL;
        if InitializeAcl(acl, acl_len as u32, ACL_REVISION) == 0 {
            return Err(w32("InitializeAcl"));
        }
        if AddAccessAllowedAce(
            acl,
            ACL_REVISION,
            GENERIC_READ | GENERIC_WRITE,
            sid_ptr as *mut core::ffi::c_void,
        ) == 0
        {
            return Err(w32("AddAccessAllowedAce"));
        }
        let mut sd: SECURITY_DESCRIPTOR = std::mem::zeroed();
        if InitializeSecurityDescriptor(&mut sd as *mut SECURITY_DESCRIPTOR as *mut core::ffi::c_void, 1 /* SECURITY_DESCRIPTOR_REVISION */) == 0 {
            return Err(w32("InitializeSecurityDescriptor"));
        }
        if SetSecurityDescriptorOwner(&mut sd as *mut SECURITY_DESCRIPTOR as *mut core::ffi::c_void, sid_ptr as *mut core::ffi::c_void, 0) == 0 {
            return Err(w32("SetSecurityDescriptorOwner"));
        }
        // DACL 以 PRESENT 标志内嵌 —— 简化:SetSecurityDescriptorDacl。
        use windows_sys::Win32::Security::SetSecurityDescriptorDacl;
        if SetSecurityDescriptorDacl(&mut sd as *mut SECURITY_DESCRIPTOR as *mut core::ffi::c_void, 1, acl, 0) == 0 {
            return Err(w32("SetSecurityDescriptorDacl"));
        }
        Ok(UserSecurityDescriptor { sd, _sid_buf: buf, _acl_buf: acl_buf })
    }
}

/// 一个 OVERLAPPED + 独立事件(每连接读写各自持有)。
struct Overlapped {
    ov: Box<OVERLAPPED>,
    event: HANDLE,
}
// SAFETY: Overlapped 只含裸句柄与 Box;句柄非线程亲和,移动安全。
unsafe impl Send for Overlapped {}

impl Overlapped {
    fn new() -> Result<Self, TransportError> {
        // SAFETY: 事件句柄由我们关闭。
        unsafe {
            let event = CreateEventW(std::ptr::null(), 1 /* manual reset */, 0, std::ptr::null());
            if event.is_null() || event == INVALID_HANDLE_VALUE {
                return Err(w32("CreateEventW"));
            }
            Ok(Self { ov: Box::new(std::mem::zeroed()), event })
        }
    }
    fn reset(&mut self) {
        // SAFETY: OVERLAPPED 字段均为 C 可写;逐字段清零在 Windows ABI 下成立。
        unsafe {
            std::ptr::write_bytes(self.ov.as_mut() as *mut OVERLAPPED, 0, 1);
        }
        self.ov.hEvent = self.event;
    }
}

impl Drop for Overlapped {
    fn drop(&mut self) {
        // SAFETY: event 由 CreateEventW 创建,唯一所有者在此。
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.event);
        }
    }
}

/// 双工管道连接(server 与 client 共用)。
pub struct PipeConnection {
    handle: HANDLE,
    /// server 侧连接与 PipeServer 共享同一句柄:Drop 只断开不关闭
    /// (由 server 显式 disconnect 后再 accept 下一客户端)。
    owns_handle: bool,
    rov: std::sync::Mutex<Overlapped>,
    wov: std::sync::Mutex<Overlapped>,
}
// SAFETY: 管道句柄非线程亲和;跨线程移动是受支持的用法(官方 I/O 模型)。
unsafe impl Send for PipeConnection {}

impl PipeConnection {
    /// 读满 `buf`(阻塞直至读满/断开);`cancel_flag` 非 0 时以
    /// `CancelIoEx` 取消等待并返回 `TimedOut`(轮询粒度 100ms)。
    pub fn read_exact(
        &self,
        buf: &mut [u8],
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<(), TransportError> {
        let mut off = 0usize;
        while off < buf.len() {
            let n = self.read_some(&mut buf[off..], cancel)?;
            if n == 0 {
                return Err(TransportError::BrokenPipe);
            }
            off += n;
        }
        Ok(())
    }

    /// 读一段(阻塞单次 OVERLAPPED 读,可取消)。
    pub fn read_some(
        &self,
        buf: &mut [u8],
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<usize, TransportError> {
        // SAFETY: OVERLAPPED/事件由 Overlapped 独占管理(Mutex 保证同一
        // 时刻同一方向只有一个在途 I/O);缓冲在调用期间有效。
        unsafe {
            let mut guard = self.rov.lock().unwrap();
            guard.reset();
            let ov = guard.ov.as_mut() as *mut OVERLAPPED;
            let event = guard.event;
            let mut got = 0u32;
            let ok = ReadFile(self.handle, buf.as_mut_ptr().cast(), buf.len() as u32, &mut got, ov);
            if ok == 0 {
                let e = GetLastError();
                if e != ERROR_IO_PENDING {
                    return Err(self.map_io_err("ReadFile", e));
                }
            }
            loop {
                let mut transferred = 0u32;
                if GetOverlappedResult(self.handle, ov, &mut transferred, 0) != 0 {
                    return Ok(transferred as usize);
                }
                let e = GetLastError();
                if e != ERROR_IO_INCOMPLETE {
                    return Err(self.map_io_err("GetOverlappedResult", e));
                }
                let w = WaitForSingleObject(event, IO_EVENT_TIMEOUT_MS);
                if w == WAIT_OBJECT_0 {
                    continue;
                }
                if w == WAIT_TIMEOUT {
                    if let Some(flag) = cancel {
                        if flag.load(std::sync::atomic::Ordering::Relaxed) {
                            CancelIoEx(self.handle, ov);
                            // 取消后等待真正完成,回收 OVERLAPPED(计划 §5.3)。
                            let mut transferred = 0u32;
                            let _ = GetOverlappedResult(self.handle, ov, &mut transferred, 1);
                            return Err(TransportError::TimedOut);
                        }
                    }
                    continue;
                }
                return Err(w32("WaitForSingleObject"));
            }
        }
    }

    pub fn write_all(&self, bytes: &[u8]) -> Result<(), TransportError> {
        let mut off = 0usize;
        while off < bytes.len() {
            let n = self.write_some(&bytes[off..])?;
            if n == 0 {
                return Err(TransportError::BrokenPipe);
            }
            off += n;
        }
        Ok(())
    }

    fn write_some(&self, bytes: &[u8]) -> Result<usize, TransportError> {
        // SAFETY: 同 read_some。
        unsafe {
            let mut guard = self.wov.lock().unwrap();
            guard.reset();
            let ov = guard.ov.as_mut() as *mut OVERLAPPED;
            let mut wrote = 0u32;
            let ok = WriteFile(self.handle, bytes.as_ptr().cast(), bytes.len() as u32, &mut wrote, ov);
            if ok == 0 {
                let e = GetLastError();
                if e != ERROR_IO_PENDING {
                    return Err(self.map_io_err("WriteFile", e));
                }
            }
            let mut transferred = 0u32;
            if GetOverlappedResult(self.handle, ov, &mut transferred, 1) == 0 {
                return Err(self.map_io_err("GetOverlappedResult(write)", GetLastError()));
            }
            Ok(transferred as usize)
        }
    }

    fn map_io_err(&self, op: &'static str, code: u32) -> TransportError {
        if code == ERROR_BROKEN_PIPE {
            return TransportError::BrokenPipe;
        }
        TransportError::Win32 { op, code }
    }

    /// 客户端进程 PID(指定进程校验用)。
    pub fn client_pid(&self) -> Result<u32, TransportError> {
        // SAFETY: 只读查询。
        unsafe {
            let mut pid = 0u32;
            if GetNamedPipeClientProcessId(self.handle, &mut pid) == 0 {
                return Err(w32("GetNamedPipeClientProcessId"));
            }
            Ok(pid)
        }
    }
}

impl Drop for PipeConnection {
    fn drop(&mut self) {
        // SAFETY: 仅客户端连接拥有独立句柄;server 侧句柄由 PipeServer 关闭。
        if self.owns_handle {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.handle);
            }
        }
    }
}

/// core 侧管道 server(单实例,一次接受一个连接;重连需重新 create)。
pub struct PipeServer {
    handle: HANDLE,
    name: String,
    rov: std::sync::Mutex<Overlapped>,
}
// SAFETY: 同 PipeConnection。
unsafe impl Send for PipeServer {}
// SAFETY: 所有共享访问经 &self 的 Mutex/原子;句柄 API 本身线程安全。
unsafe impl Sync for PipeServer {}
unsafe impl Sync for PipeConnection {}
unsafe impl Sync for Overlapped {}

impl PipeServer {
    /// 创建命名管道实例(当前用户 DACL + 拒绝远程)。
    pub fn create(name: &str) -> Result<Self, TransportError> {
        if name.len() > 200 {
            return Err(TransportError::NameTooLong);
        }
        let sec = current_user_security_descriptor()?;
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: &sec.sd as *const SECURITY_DESCRIPTOR as *mut _,
            bInheritHandle: 0,
        };
        let wide = to_wide(name);
        // SAFETY: name/sa 缓冲在调用期间有效;返回句柄由 self 管理。
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1, // 单实例:一条连接一个身份;断开后由上层重建
                64 * 1024,
                64 * 1024,
                0,
                &mut sa,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(w32("CreateNamedPipeW"));
        }
        Ok(Self {
            handle,
            name: name.to_string(),
            rov: std::sync::Mutex::new(Overlapped::new()?),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// 等待客户端连接;`cancel` 置位时取消等待返回 `TimedOut`。
    pub fn accept(&self, cancel: &std::sync::atomic::AtomicBool) -> Result<PipeConnection, TransportError> {
        // SAFETY: OVERLAPPED 独占管理(Mutex);句柄为自身字段。
        unsafe {
            let mut guard = self.rov.lock().unwrap();
            guard.reset();
            let ov = guard.ov.as_mut() as *mut OVERLAPPED;
            let event = guard.event;
            let ok = ConnectNamedPipe(self.handle, ov);
            if ok == 0 {
                let e = GetLastError();
                if e != ERROR_IO_PENDING && e != ERROR_PIPE_CONNECTED {
                    return Err(w32("ConnectNamedPipe"));
                }
                if e == ERROR_PIPE_CONNECTED {
                    return Ok(self.connection());
                }
            }
            loop {
                let mut transferred = 0u32;
                if GetOverlappedResult(self.handle, ov, &mut transferred, 0) != 0 {
                    break;
                }
                let e = GetLastError();
                if e != ERROR_IO_INCOMPLETE {
                    return Err(w32("ConnectNamedPipe(result)"));
                }
                let w = WaitForSingleObject(event, IO_EVENT_TIMEOUT_MS);
                if w == WAIT_OBJECT_0 {
                    break;
                }
                if w == WAIT_TIMEOUT {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        CancelIoEx(self.handle, ov);
                        let mut t = 0u32;
                        let _ = GetOverlappedResult(self.handle, ov, &mut t, 1);
                        return Err(TransportError::TimedOut);
                    }
                    continue;
                }
                return Err(w32("WaitForSingleObject(accept)"));
            }
            drop(guard);
            Ok(self.connection())
        }
    }

    fn connection(&self) -> PipeConnection {
        PipeConnection {
            handle: self.handle,
            owns_handle: false,
            rov: std::sync::Mutex::new(Overlapped::new().expect("event")),
            wov: std::sync::Mutex::new(Overlapped::new().expect("event")),
        }
    }

    /// 连接结束后断开当前客户端实例,使本 server 可接受下一个连接。
    pub fn disconnect(&self) {
        // SAFETY: 句柄为自身字段。
        unsafe {
            DisconnectNamedPipe(self.handle);
        }
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        // SAFETY: 句柄唯一所有者在此。
        unsafe {
            DisconnectNamedPipe(self.handle);
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// 客户端连接(bridge/控制面)。管道为字节模式;认证/身份在语义层。
pub fn connect_client(name: &str) -> Result<PipeConnection, TransportError> {
    let wide = to_wide(name);
    // SAFETY: name 在调用期间有效;返回句柄由 PipeConnection 管理。
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_NONE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(w32("CreateFileW(pipe)"));
    }
    Ok(PipeConnection {
        handle,
        owns_handle: true,
        rov: std::sync::Mutex::new(Overlapped::new()?),
        wov: std::sync::Mutex::new(Overlapped::new()?),
    })
}
