//! G3 原生调用封装为 [`NativeOps`](crate::native_session::NativeOps)(N3 现场验证;
//! 计划 §7-N3:把已证 g3 FFI 序列封装为 NativeSession 的派发后端)。
//!
//! 安全边界:与 g3_send_run 同纪律 —— getter 租约 + 锚点核对 +
//! transport 在位检查先于任何原生调用;body data 经 QQ operator new 分配
//! 后移转(本侧永不释放);每次 submit 均为完整独立调用(零常驻修改)。

use crate::native_session::{OpsError, NativeOps};
use crate::native_response::Cookie;
use core::ffi::c_void;

/// g3 原生调用所需 wrapper 内 RVA(与 g3.rs 同源)。
mod rva {
    pub const GETTER: usize = 0x72DE38;
    pub const ANCHOR_SVC_VTBL: usize = 0x3F6DED8;
    pub const SEND_SSO: usize = 0x732A80;
    pub const OPERATOR_NEW: usize = 0xB75000;
    pub const EMPTY_FN_CALLABLE: usize = 0x72D596;
    pub const EMPTY_FN_DTOR: usize = 0x72D5C8;
    pub const SVC_TRANSPORT: usize = 0x60;
}

type GetterFn = unsafe extern "system" fn(out: *mut ServicePair) -> *mut ServicePair;
type SendSsoFn = unsafe extern "system" fn(usize, usize, usize, u8, usize);
type OperatorNewFn = unsafe extern "system" fn(usize) -> *mut c_void;

#[repr(C)]
struct ServicePair {
    obj: *mut c_void,
    ctrl: *mut c_void,
}

/// g3 原生调用 ops(每个实例绑定一个 wrapper 基址)。
pub struct G3NativeOps {
    wrapper_base: usize,
}

impl G3NativeOps {
    pub fn new(wrapper_base: usize) -> Self {
        Self { wrapper_base }
    }

    /// 就绪探针(零副作用):getter 租约 + 锚点核对;成功返回**真实服务
    /// 对象地址**(诚实证据,供 `AttachProof.service_addr` 登记;ops.submit
    /// 每次调用自行重新租约,本地址不用于派发)。
    pub fn probe_service(&mut self) -> Result<usize, OpsError> {
        let wb = self.wrapper_base;
        // SAFETY: 同 submit ①②;只读,零写入。
        unsafe {
            let getter: GetterFn = core::mem::transmute(wb + rva::GETTER);
            let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
            let ret = getter(&mut pair as *mut ServicePair);
            if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
                return Err(OpsError::NotReady("probe: getter failed".into()));
            }
            let svc = pair.obj as usize;
            let vptr = core::ptr::read_volatile(svc as *const u64);
            if (vptr as usize).wrapping_sub(wb) != rva::ANCHOR_SVC_VTBL {
                return Err(OpsError::NotReady("probe: anchor mismatch".into()));
            }
            Ok(svc)
        }
    }

    /// 完整 g3 发送序列:adopt → transport → alloc → self str → send。
    /// 返回 Ok = 已提交(异步;结果需经 CPB 差分确认)。
    fn native_submit(&self, body: &[u8]) -> Result<(), OpsError> {
        let wb = self.wrapper_base;
        // SAFETY: 与 g3_send_run 同序列;全部地址经锚点核对。
        unsafe {
            // ① getter 租约。
            let getter: GetterFn = core::mem::transmute(wb + rva::GETTER);
            let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
            let ret = getter(&mut pair as *mut ServicePair);
            if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
                return Err(OpsError::PreCall("g3 adopt: getter failed"));
            }
            let svc = pair.obj as usize;
            let vptr = core::ptr::read_volatile(svc as *const u64);
            if (vptr as usize).wrapping_sub(wb) != rva::ANCHOR_SVC_VTBL {
                return Err(OpsError::PreCall("g3 adopt: anchor mismatch"));
            }

            // ② transport 在位。
            let transport = match read_qword(svc + rva::SVC_TRANSPORT) {
                Some(t) if t > 0x10000 => t as usize,
                _ => return Err(OpsError::NotReady("transport 未安装".into())),
            };

            // ③ body 分配(QQ operator new;QQ 侧释放)。
            let new_fn: OperatorNewFn = core::mem::transmute(wb + rva::OPERATOR_NEW);
            let buf = new_fn(body.len().max(1));
            if buf.is_null() {
                return Err(OpsError::PreCall("g3 alloc: operator new null"));
            }
            core::ptr::copy_nonoverlapping(body.as_ptr(), buf as *mut u8, body.len());
            let begin = buf as u64;
            let mut vec_hdr: [u64; 3] = [begin, begin + body.len() as u64, begin + body.len() as u64];

            // ④ self string(svc+0x1f0 → 24B)。
            let mut self_str = [0u8; 24];
            if !copy_self_string(svc + 0x1F0, new_fn, &mut self_str) {
                return Err(OpsError::PreCall("g3 selfstr: unreadable"));
            }

            // ⑤ cmd + 空回调。
            let mut cmd = [0u8; 24];
            let cmd_name = b"MessageSvc.PbSendMsg";
            cmd[0] = (cmd_name.len() as u8) << 1;
            cmd[1..1 + cmd_name.len()].copy_from_slice(cmd_name);
            let mut cb = [0u64; 4];
            cb[2] = (wb + rva::EMPTY_FN_CALLABLE) as u64;
            cb[3] = (wb + rva::EMPTY_FN_DTOR) as u64;

            // ⑥ 提交(异步;QQ dispatcher 执行)。
            let send: SendSsoFn = core::mem::transmute(wb + rva::SEND_SSO);
            send(
                svc,
                cmd.as_mut_ptr() as usize,
                vec_hdr.as_mut_ptr() as usize,
                1,
                cb.as_mut_ptr() as usize,
            );
        }
        // 移转后 buf/cb 所有权归 QQ;本侧不再触碰。
        let _ = Cookie([0u8; 16]); // cookie 语义由 NativeSession 层管理
        Ok(())
    }
}

impl NativeOps for G3NativeOps {
    fn probe_ready(&mut self, _service_addr: usize) -> Result<(), OpsError> {
        // 最小就绪探针:getter 租约 + 锚点核对(零副作用)。
        self.probe_service().map(|_| ())
    }

    fn submit(&mut self, _service_addr: usize, _cookie: &Cookie, body: &[u8]) -> Result<(), OpsError> {
        self.native_submit(body)
    }
}

/// RPM-self 单 qword 读取(g2 同款内核校验读)。
fn read_qword(addr: usize) -> Option<u64> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut out = 0u64;
    let mut got = 0usize;
    // SAFETY: 自进程 RPM;栈缓冲。
    unsafe {
        ReadProcessMemory(GetCurrentProcess(), addr as *const c_void, (&mut out as *mut u64).cast(), 8, &mut got)
    };
    if got == 8 { Some(out) } else { None }
}

/// self string 拷贝(短/长形态;长形态 data 换 QQ 分配副本)。
unsafe fn copy_self_string(addr: usize, new_fn: OperatorNewFn, dst: &mut [u8; 24]) -> bool {
    let mut raw = [0u64; 3];
    for (i, slot) in raw.iter_mut().enumerate() {
        match read_qword(addr + i * 8) {
            Some(v) => *slot = v,
            None => return false,
        }
    }
    let q0 = raw[0].to_le_bytes();
    if q0[0] & 1 == 0 {
        // 短形态:24B 原样。
        for (i, q) in raw.iter().enumerate() {
            dst[i * 8..i * 8 + 8].copy_from_slice(&q.to_le_bytes());
        }
        true
    } else {
        // 长形态:头部原样 + data 换 QQ 分配副本。
        let size = raw[1] as usize;
        let data_ptr = raw[2] as usize;
        if size == 0 || size > 256 || data_ptr < 0x10000 {
            return false;
        }
        let copy = new_fn(size + 1);
        if copy.is_null() {
            return false;
        }
        let mut tmp = [0u8; 256];
        let src = data_ptr as *const u8;
        core::ptr::copy_nonoverlapping(src, tmp.as_mut_ptr(), size);
        core::ptr::write(copy as *mut u8, 0);
        // 写 NUL 后重拷正文(顺序:先 copy 再 NUL)。
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), copy as *mut u8, size);
        dst[0..8].copy_from_slice(&raw[0].to_le_bytes());
        dst[8..16].copy_from_slice(&(size as u64).to_le_bytes());
        dst[16..24].copy_from_slice(&(copy as u64).to_le_bytes());
        true
    }
}
