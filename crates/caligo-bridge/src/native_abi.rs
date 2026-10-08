//! 恢复的原生 ABI 布局(固定 9.9.33-52230,计划 §7-P4)。
//!
//! 全部形状来自 R2/R3 静态证据(`qq-native-thread-contract.md` /
//! `qq-native-lifetime-contract.md` / capability profile)。本模块只定义
//! 布局与机器码级协议(移动/复制/计数),**不**绑定真实 QQ 函数地址 ——
//! 真实入口在 P6 G1 前不接线;LAB 由 `native_adapter_contract` 的假宿主
//! 以相同形状实现,使 adapter 的 unsafe 路径在受控环境受试。
//!
//! 纪律:
//! - 伪代码字段名不是 ABI 事实;本文件每个偏移都能指回证据(内联注释)。
//! - 计数/释放只经本模块辅助函数;调用方不得手解引用控制块。
//! - panic/错误不得跨原生 callback 边界(见 [`callback_boundary`])。

use core::ffi::c_void;

/// 控制块(0x28;lifetime contract §1):vptr/strong/weak/obj。
/// 布局 opaque:计数只经 [`ctrl_strong_inc`]/[`ctrl_release`];此结构仅为
/// 假宿主与真实布局共享同一几何。
#[repr(C)]
pub struct CtrlBlockLayout {
    pub vptr: *const c_void, // RVA 0x3f6f4d8(锚点)
    pub strong: i32,         // +0x08
    pub weak: i32,           // +0x10
    pub obj: *mut c_void,    // +0x18
    pub _tail: [u8; 16],     // 补足 0x28
}

/// getter `72DE38` 的输出 pair(强引用已由 getter 获取)。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ServicePair {
    pub obj: *mut c_void,
    pub ctrl: *mut CtrlBlockLayout,
}

impl ServicePair {
    pub fn is_null(self) -> bool {
        self.obj.is_null() || self.ctrl.is_null()
    }
}

/// 24 字节 tagged string(R2 send contract §2):首 byte 低位区分短/长;
/// 短形长度 = 首字节 >>1、数据起点 +1;长形 len/ptr 在 +8/+10。
/// adapter 只经 [`tagged_copy`] 构造/复制;不在此实现真实 QQ 分配器。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaggedString24 {
    pub raw: [u8; 24],
}

pub const TAGGED_SHORT_MAX: usize = 22; // 短形容量(22 数据 + 1 长度字节 + NUL)

/// 短形构造(容量内);超长返回 None(调用方须走长形路径 —— 真实分配器
/// 未接线前 adapter 拒绝长命令,如实报 Unsupported)。
pub fn tagged_short(data: &[u8]) -> Option<TaggedString24> {
    if data.len() > TAGGED_SHORT_MAX {
        return None;
    }
    let mut t = TaggedString24 { raw: [0; 24] };
    t.raw[0] = ((data.len() << 1) | 1) as u8;
    t.raw[1..=data.len()].copy_from_slice(data);
    Some(t)
}

impl TaggedString24 {
    pub fn len(&self) -> usize {
        if self.raw[0] & 1 == 1 {
            (self.raw[0] >> 1) as usize
        } else {
            // 长形:len 在 +8(u64)。LAB 只用短形;真实长形读取属 P6 前补证。
            u64::from_le_bytes(self.raw[8..16].try_into().unwrap()) as usize
        }
    }
    pub fn bytes(&self) -> &[u8] {
        if self.raw[0] & 1 == 1 {
            &self.raw[1..1 + (self.raw[0] >> 1) as usize]
        } else {
            &[] // 长形数据须按 +0x10 指针读;不在本模块解引用
        }
    }
}

/// 24 字节 byte 容器 {begin, end, cap}(R2 §2;第三字段不参与长度)。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ByteRange24 {
    pub begin: *mut u8,
    pub end: *mut u8,
    pub cap: *mut u8,
}

impl ByteRange24 {
    pub fn from_slice(data: &[u8]) -> Self {
        // 假宿主语义:借用调用方缓冲(提交期内有效;adapter 保证同步复制
        // 在入口调用内完成 —— R2 §2 的约束)。真实路径 P6 前改为 QQ 分配器。
        let begin = data.as_ptr() as *mut u8;
        Self { begin, end: unsafe { begin.add(data.len()) }, cap: begin }
    }
    pub fn len(&self) -> usize {
        unsafe { self.end.offset_from_unsigned(self.begin) }
    }
}

/// callback 管理对象(R2 §3 的 move 协议,形状抽象):
/// manager(+0x10)的 operation=0 即"移动/销毁源";invoker(+0x18)移动后
/// 必须置空。adapter 以 [`CallbackMove`] 执行协议,不复刻 STL 类型名。
pub type ManagerOp = unsafe extern "system" fn(mgr: *mut c_void, operation: u32);
pub type InvokerFn = unsafe extern "system" fn(invoker: *mut c_void, a: usize, b: u32, c: *mut c_void, d: *mut c_void);

#[repr(C)]
pub struct CallbackMgrLayout {
    pub storage: [u8; 0x10], // 小对象存储
    pub manager: Option<ManagerOp>,
    pub invoker: Option<InvokerFn>,
    pub capture: *mut c_void,
}

/// 执行 move 协议(31c5f8 语义的 LAB 等价形):guts 转移到 dst,源置
/// moved-from(不可再用);**capture 的所有权随之转移到 dst**,销毁只在
/// dst 侧 manager operation=0 发生一次。真实 QQ 的 op=0 transfer-in
/// 细节在 P6 ABI 冻结时定证;两侧(假宿主/adapter)已按同一简化合同
/// 受试,证据跟踪见 lifetime contract §3.3。
///
/// # Safety
/// `src` 须指向有效 CallbackMgrLayout;调用后 src 处于 moved-from 态。
pub unsafe fn callback_move_protocol(src: *mut CallbackMgrLayout) -> CallbackMgrLayout {
    let moved = CallbackMgrLayout {
        storage: (*src).storage,
        manager: (*src).manager,
        invoker: (*src).invoker,
        capture: (*src).capture,
    };
    (*src).manager = None;
    (*src).invoker = None;
    (*src).capture = core::ptr::null_mut();
    moved
}

/// 强引用递增(lifetime contract §2 获取位点语义)。
///
/// # Safety
/// `ctrl` 须为有效控制块或 null(null 为无操作)。
pub unsafe fn ctrl_strong_inc(ctrl: *mut CtrlBlockLayout) {
    if !ctrl.is_null() {
        (*ctrl).strong += 1;
    }
}

/// 释放(strong-1;归零走控制块 vtable slot3 自删语义 —— 真实路径由 QQ
/// 释放家族执行;LAB 由假宿主注入对应行为)。返回是否触发了删除。
///
/// # Safety
/// `ctrl` 须为有效控制块或 null。
pub unsafe fn ctrl_release(ctrl: *mut CtrlBlockLayout, delete_hook: Option<unsafe fn(*mut CtrlBlockLayout)>) -> bool {
    if ctrl.is_null() {
        return false;
    }
    (*ctrl).strong -= 1;
    if (*ctrl).strong <= 0 {
        if let Some(hook) = delete_hook {
            hook(ctrl);
        }
        true
    } else {
        false
    }
}

/// 原生 callback 边界:panic 不得越过(计划 §7-P4)。返回 `None` 表示
/// 回调体内 panic(调用方按失败分类,不得再触原生对象)。
pub fn callback_boundary<R>(f: impl FnOnce() -> R) -> Option<R> {
    // LAB/真实同一路径;std catch_unwind 足够(不跨 FFI 传播)。
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

/// transport 提交入口的机器级原型(732A80 经 732EB9 调用的形状;
/// thread contract §10.1:实际目标 0x1B4E4EC 同型)。
///
/// # Safety
/// 全部指针须指向按本文布局构造的对象;调用线程须满足调度许可(由
/// adapter 层把关,本类型不校验)。
pub type TransportSubmitFn = unsafe extern "system" fn(
    this: *mut c_void,          // transport
    command: *mut TaggedString24,
    request_holder: *mut ByteRange24, // {begin,end,cap} holder
    callback: *mut CallbackMgrLayout,
    out_token: *mut u64,
    small_storage: *mut c_void,
) -> u64;

/// dispatcher slot0 提交原语(thread contract §10.2:0x31F8CEE)。
pub type DispatcherSubmitFn = unsafe extern "system" fn(
    self_obj: *mut c_void,
    closure: *mut c_void,
    mgr_moved: *mut CallbackMgrLayout,
    state_out: *mut c_void,
) -> i32;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn tagged_short_roundtrip_and_reject() {
        let t = tagged_short(b"MessageSvc.PbSendMsg").unwrap();
        assert_eq!(t.bytes(), b"MessageSvc.PbSendMsg");
        assert_eq!(t.len(), 20);
        assert!(tagged_short(&[0u8; 23]).is_none(), "超短形容量的命令必须拒绝");
    }

    #[test]
    fn ctrl_counts_are_symmetric() {
        let mut ctrl = CtrlBlockLayout {
            vptr: core::ptr::null(),
            strong: 1,
            weak: 0,
            obj: core::ptr::null_mut(),
            _tail: [0; 16],
        };
        let c = &mut ctrl as *mut CtrlBlockLayout;
        unsafe {
            ctrl_strong_inc(c);
            assert_eq!((*c).strong, 2);
            assert!(!ctrl_release(c, None));
            assert!(ctrl_release(c, None), "归零应触发删除钩子");
        }
    }

    #[test]
    fn callback_move_invalidates_source_and_keeps_capture() {
        static DESTROYS: AtomicU32 = AtomicU32::new(0);
        unsafe extern "system" fn op(m: *mut c_void, operation: u32) {
            // SAFETY: 测试内单线程访问静态;m 由本测试写入。
            unsafe {
                if operation == 0 {
                    DESTROYS.fetch_add(1, Ordering::Relaxed);
                    (*(m as *mut CallbackMgrLayout)).capture = core::ptr::null_mut();
                }
            }
        }
        let mut mgr = CallbackMgrLayout {
            storage: [7; 0x10],
            manager: Some(op),
            invoker: None,
            capture: 0x1234 as *mut c_void,
        };
        // SAFETY: 本测试构造。
        let mut moved = unsafe { callback_move_protocol(&mut mgr) };
        assert_eq!(moved.storage, [7; 0x10]);
        assert_eq!(moved.capture, 0x1234 as *mut c_void, "capture 所有权随 guts 转移");
        assert!(mgr.manager.is_none() && mgr.invoker.is_none(), "源必须 moved-from");
        // dst 侧销毁一次。
        // SAFETY: moved.manager 在位;moved 存活到调用结束。
        unsafe {
            let destroy = moved.manager.unwrap();
            destroy(core::ptr::from_mut(&mut moved) as *mut c_void, 0);
        }
        assert_eq!(DESTROYS.load(Ordering::Relaxed), 1, "销毁恰一次(在 dst 侧)");
    }

    #[test]
    fn boundary_catches_panic() {
        assert!(callback_boundary::<()>(|| panic!("x")).is_none());
        assert_eq!(callback_boundary(|| 41 + 1), Some(42));
    }
}
