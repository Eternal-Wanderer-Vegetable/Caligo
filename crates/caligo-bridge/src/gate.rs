//! K4-D0 研究门控:旧现场路线默认拒绝。
//!
//! 计划 §7-D0:CLI 的 inject/async/exec 业务入口默认拒绝;bridge 导出独立拒绝,
//! 不能只封 CLI;研究 feature 默认关闭,只在自建宿主保留需要的研究能力。
//!
//! 规则:
//! - 普通构建(无 `research` feature):所有会触碰 QQ 运行时的导出
//!   ([`crate::caligo_probe_run`]、`caligo_register_entry`、`caligo_env_start`、
//!   `caligo_obs_run*`、`caligo_interrupt_run`、`caligo_async_run`、
//!   `caligo_exec_run`)以及 CLI `inject`、旧 onebotd runner 一律拒绝;
//!   仅 `caligo_bridge_abi_version` / `caligo_bridge_protocol_version` /
//!   `DllMain` 保持可用(无 QQ 副作用,供握手与加载器核对)。
//! - `--features research` 构建:行为与 K3 冻结版一致,仅供自建宿主/离线
//!   研究使用;**研究 feature 不是恢复 QQ 实验的许可**(计划 §7-D0 op4),
//!   现场准入仍以 `docs/acceptance/k4-field-entry.md`(D6)为准。
//! - 门控检查位于每个入口的第一条语句,先于任何参数校验与进程/内存访问。

/// 普通构建下,被门控入口的统一返回码。
///
/// 取 0xE0(K3 各结果码均 < 16,不冲突);CLI 侧以退出码 3 + 明确文案呈现。
pub const ERR_RESEARCH_DISABLED: u32 = 0xE0;

/// 研究路线是否被本次构建启用。
pub const RESEARCH_ENABLED: bool = cfg!(feature = "research");

/// 研究路线是否启用。
pub fn research_enabled() -> bool {
    RESEARCH_ENABLED
}

/// 普通构建下返回 [`ERR_RESEARCH_DISABLED`],研究构建返回 `None`(放行)。
///
/// 每个被门控入口的第一条语句调用;返回 `Some(code)` 时入口必须立即返回,
/// 不得再触碰任何参数指针、进程句柄或宿主 API。
pub fn reject_legacy() -> Option<u32> {
    if RESEARCH_ENABLED {
        None
    } else {
        Some(ERR_RESEARCH_DISABLED)
    }
}

/// CLI/runner 侧的拒绝文案(说明当前真实层级与恢复路径)。
pub const DISABLED_NOTICE: &str = "\
[K4-D0] 已停用:注入式现场路线(inject/async/exec/onebotd 轮询)在普通构建中默认拒绝。\
背景:local-evidence/k3-g/final-boundary.md 记录六次实例损失后,现场注入进入暂停;\
K4 起生产路线为宿主合法初始化 + 常驻 bridge + 命名管道 + 常驻 core(docs/plans/2026-10-07-*)。\
本命令不打开任何进程、不注入任何 DLL、不调用任何 QQ 接口。\
如需离线研究构建:cargo build --features research(仅限自建宿主,不构成现场准入)。";

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "research"))]
    #[test]
    fn research_feature_is_off_by_default() {
        // K4-D0 验收:普通构建研究路线关闭。research 构建下本测试编译不进。
        assert!(!research_enabled());
        assert_eq!(reject_legacy(), Some(ERR_RESEARCH_DISABLED));
    }

    #[test]
    fn disabled_code_does_not_collide_with_known_result_codes() {
        // K1/K2/K3 的 probe/async/exec 结果码全部 < 16;0xE0 保持可区分。
        assert!(ERR_RESEARCH_DISABLED > 0x10);
    }
}
