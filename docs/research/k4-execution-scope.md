# K4 执行范围与遗留路线冻结(k4-execution-scope)

> D0 交付物(计划 §7-D0)。执行:2026-10-07。锚点提交见文末基线。
> 标签约定沿用 K4 计划:`[verified]` 源码/构建核实;`[inferred]` 工程判断。

## 1. 本轮落地的冻结(计划 §7-D0 op1–op5)

| op | 要求 | 落地 | 状态 |
|---|---|---|---|
| op1 | 保存 HEAD/工作树/哈希基线,不覆盖旧证据;旧代码标记 research legacy | §4 基线;`winutil.rs`/`asyncrun.rs`/`exec.rs` 文档头标注 research legacy;旧 JSONL/PDB/DLL 均未改动 | [verified] |
| op2 | CLI inject/async/exec 业务入口默认拒绝;bridge 导出独立拒绝 | CLI `cmd_inject` 第一条语句拒绝(退出码 3);bridge 七个宿主入口导出(`caligo_probe_run`/`register_entry`/`env_start`/`obs_run`/`obs_run2`/`interrupt_run`/`async_run`/`exec_run`)首条语句拒绝(结果码 0xE0);`async_run`/`exec_run` 函数体内部再次独立拒绝 | [verified] |
| op3 | 旧 onebotd 退出并提示停用;不调用 cli_inject;不自动加确认参数 | `onebotd::main` 首条语句拒绝(退出码 3),ARM/poll/STOP 流程不可达;`build_send_args` 纯函数保留(仅测试引用,不构成执行入口) | [verified] |
| op4 | 核查所有导出与脚本启动边界,避免绕过入口 | 门控覆盖面见 §2;剩余已知旁路面见 §3(如实列出,不含糊) | [verified] |
| op5 | 修正"生产层定型"等结论;README 说明当前真实层级 | README 状态段改写为 K4 层级;历史验收记录(acceptance/k3-*)保留原文不改,其"生产化"表述以本文件为准 | [verified] |

## 2. 门控设计(实现事实)

- **feature 开关**:`caligo-bridge` 新增 `research` feature,`default = []`。
  普通构建 `cfg!(feature = "research") == false`;`--features research` 恢复
  K3 冻结行为,**仅限自建宿主离线研究**。
- **bridge 侧**:`crates/caligo-bridge/src/gate.rs` 集中实现
  (`reject_legacy()` / `ERR_RESEARCH_DISABLED = 0xE0`)。`DllMain`、
  `caligo_bridge_abi_version`、`caligo_bridge_protocol_version`、
  `caligo_entry_registered/fired`(纯读原子量)不在门控内——无 QQ 副作用。
- **CLI 侧**:`cmd_inject` 拒绝先于门 1(manifest)/门 2(实例确认);退出码 3,
  文案含 `--features research` 与现场准入文档指针。
- **onebotd 侧**:同上,退出码 3。
- **loader 资源修复(K4-D1 预合并)**:`winutil.rs` 等待超时改为**留存账本**
  (`RetainedRemoteAlloc`,七处等待点全部改序:先判 wait,超时不释放、登记
  `retain_remote()`;env 链路线程的缓冲因内部 worker 生命周期不可证明,一律
  留存)。这直接修复计划 §5.3 指出的"超时后释放仍被引用内存"缺陷面;
  K4 生产链路不再逐次远程调用,账本仅服务研究构建取证。

## 3. 覆盖面与剩余旁路面(op4 如实清单)

已封入口:CLI `inject`(全部 async/exec/send 载荷经它);bridge 七个动作导出;
旧 onebotd runner。

如实声明的剩余边界(均不构成"可执行 QQ 调用"的旁路):

1. `research` 构建本身:feature 即钥匙。任何持有本仓库构建能力的人可自行
   打开——这是研究工具集的既定定位(K3 收官结论),不是安全边界。
2. 直接 `LoadLibrary` 本 DLL 并调用导出:普通构建返回 0xE0,已封;
   research 构建不设防(同上条)。
3. `caligo-cli` 其余只读子命令(`process`/`exports`/`bytes`/`disasm`/
   `identifiers`/`rtti`/`envscan`/`verify-manifest`):保持可用——
   final-boundary 铁律明确"只读 envscan/process 允许",它们不写目标进程。
4. 历史构建产物(target/、旧 DLL):不在本仓库门控范围;按 op1 未覆盖、
   未删除;执行者按 local-evidence 纪律处置。
5. `js/` 下的研究脚本与 K3 脚本常量:普通构建下无任何入口可达
   (async/exec 全部拒绝),仅作历史保留。

## 4. 基线记录

- HEAD:`6fd83294774d01e1627ad8c5492de539f41c52ac`(main,2026-10-07)。
- 旧构建哈希基线:QQ 模块摘要见 `docs/research/environment.json`;
  manifest 判据见 `docs/contracts/version-adapter-manifest.json`。
  旧 bridge 构建产物(K3 各 DLL)只存在于本机 target/local-evidence,未入库。
- 测试基线:D0 前 32 通过 / 0 失败(local-evidence/reviews/2026-10-07 §5);
  D0 后新增:gate 单测(asyncrun/exec/gate)、CLI legacy_gate 4 项、
  onebotd_gate 2 项;原有 32 项全部保留。

## 5. 恢复条件(何时可以解除冻结)

本文件不授权任何现场动作。解除顺序(计划 §7):

1. D2 完成 bootstrap 候选契约(离线);
2. D3–D5 K4-LAB 通过(runtime/journal/resident/管道全部接线并有测试);
3. D6 逐项 PASS 落入 `docs/acceptance/k4-field-entry.md`;
4. 执行者按 test-scope 指定新测试实例,D7 才恢复现场(且只走新路线,
   旧 probe/通用 exec/onebotd 不复用)。

研究 feature 的启用不改变上述任何一步。
