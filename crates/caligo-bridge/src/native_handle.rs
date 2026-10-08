//! 拥有式服务句柄(计划 §7-P4:引用句柄 + 代次 + 释放对称性)。
//!
//! `ServiceHandle` 持有 getter `72DE38` 返回的 {obj, ctrl} pair:获取即
//! 强引用(lifetime contract §2 位点),Drop 走释放家族。合同:
//! - **代次绑定**:句柄携带 Host/Session 代次;任何操作前核对,不匹配即
//!   拒绝(T16);
//! - **释放对称**:每次获取恰一次释放;计数不可回复到基线 = 泄漏(LAB
//!   的 100 次 lifecycle 判据);
//! - **危险释放拒绝**:释放钩子未接线(真实 QQ 释放家族未定证)时,
//!   Drop 只登记泄漏并转入隔离计数 —— 不猜地址、不跨 allocator。

use super::native_abi::{self, CtrlBlockLayout, ServicePair};

/// 释放家族抽象:真实=QQ 侧函数(P6 前定证);LAB=假宿主注入。
pub type ReleaseFn = unsafe fn(ctrl: *mut CtrlBlockLayout);

/// 句柄统计(泄漏/隔离审计;LAB 断言的数据源)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HandleStats {
    pub acquired: u64,
    pub released: u64,
    /// 释放家族缺失而登记的隔离句柄(不释放,如实计数)。
    pub quarantined: u64,
    /// 代次不匹配被拒绝的操作次数。
    pub stale_rejected: u64,
}

pub struct ServiceHandle {
    pair: ServicePair,
    host_generation: u64,
    session_generation: u64,
    release: Option<ReleaseFn>,
    stats: *mut HandleStats,
}

// SAFETY: pair 指针只在所属(假/真)宿主存续期内使用;宿主以代次失效
// 保证句柄不再被真实解引用后释放。跨线程移动是受支持用法(计数为原子
// 语义的宿主侧责任;LAB 假宿主单线程访问)。
unsafe impl Send for ServiceHandle {}

impl ServiceHandle {
    /// 包装 getter 返回的 pair(已含一次强引用)。
    ///
    /// # Safety
    /// `pair` 须来自真实 getter 或形状一致的假宿主。
    pub unsafe fn adopt(
        pair: ServicePair,
        host_generation: u64,
        session_generation: u64,
        release: Option<ReleaseFn>,
        stats: *mut HandleStats,
    ) -> Option<Self> {
        if pair.is_null() {
            return None;
        }
        // SAFETY: 计数经由布局辅助;null 已排除。
        unsafe { (*stats).acquired += 1 };
        Some(Self { pair, host_generation, session_generation, release, stats })
    }

    pub fn generations(&self) -> (u64, u64) {
        (self.host_generation, self.session_generation)
    }

    /// 操作前代次核对(T16;不匹配计一次拒绝并返回 false)。
    pub fn check_generations(&self, host: u64, session: u64) -> bool {
        let ok = self.host_generation == host && self.session_generation == session;
        if !ok {
            // SAFETY: stats 由宿主保证存续。
            unsafe { (*self.stats).stale_rejected += 1 };
        }
        ok
    }

    pub fn raw(&self) -> ServicePair {
        self.pair
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        // SAFETY: 析构内指针有效性由宿主/代次合同保证(失效代次的句柄
        // 在宿主侧已解除映射,release 为 None)。
        unsafe {
            match self.release {
                Some(release) => {
                    native_abi::ctrl_release(self.pair.ctrl, Some(release));
                    (*self.stats).released += 1;
                }
                None => {
                    // 释放家族未接线:不猜地址,登记隔离(Quarantined 计数)。
                    (*self.stats).quarantined += 1;
                }
            }
        }
    }
}
