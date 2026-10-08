# K4 原生路线冻结决定（k4-native-route-decision）

> 日期：2026-10-08（Asia/Shanghai）。依据：R2/R3 静态证据（`docs/research/qq-native-thread-contract.md`、`qq-native-lifetime-contract.md`，含 §10 现场闭合）+ 实例 47524 现场观测（`docs/execution/2026-10-08-native-contract-recovery/evidence/p1-observe-field-47524-t{1,2,3}.json`）。
> 本文是计划 §7-P2 的交付物。决定只基于下列证据；"更少 JS"与"SnowLuma 能运行"不作为选择依据。变更须留方案变更记录。
> 配套产物：`docs/contracts/qq-9.9.33-52230.capability-profile.json`（逐能力准入/拒绝）。

## 1. 决定

**选定：原生 MSF/SSO 路线（自有 Rust adapter + 固定 9.9.33-52230 capability profile），进入 P4。**

- 实际服务族：**MSFService**（getter `72DE38`）。运行时分支证据：实例 47524 上 `MSFCoreService` getter（`74203C`）从未被调用（`+0x674D3E8` 槽为空），MSFService 单例活跃（strong 149→501）。MSFCoreService 族标记 **unsupported**（无合同，不准入）。
- V8 owner 路线：降为**备选研究资产**，保留全部 D7–D9 现场记录；不得隐式 fallback，切换须走本文件的变更记录 + 补齐其独立合同。
- 禁用范围（详见 profile）：MSFCoreService 族；`0x750088` 执行器单例路径（现场未构造，无运行证据）；stop/cancel/drain 相关的主动关闭原语（合同未闭合）；一切多版本/地址猜测适配。

## 2. 路线对照（按计划 §7-P2 要求逐项填证据）

| 维度 | 原生 MSF/SSO | V8 owner |
|---|---|---|
| **线程** | 提交拓扑已闭合（R3.1 §10.3）：发送 `732A80 → 0x1B4E4EC → 0xB7CE8A → D32138 → 全局 dispatcher`；接收 `1B3F740 → TLS dispatcher`；回调=QQ worker 池、与提交者并发。732A80 函数体无显式线程 guard；下游全为带跨线程唤醒的 dispatcher 入队 → **有条件准入**（写调用前由 G1 受控观测终验）。调用方输入须活到同步复制完成（R2 §2 已证）。 | 首入=RequestInterrupt 链（D7 现场已证）；所有操作绑定 JS owner 线程 + uv 轮转点（D7/D9 证据）；跨线程面仅 `uv_async_send`。合同成熟但被锁面限制（C5 类竞争须修）。 |
| **寿命** | 控制块 0x28 布局/强弱的获取位点/析构尾全部闭合（lifetime contract §1–§5）；从 QQ 取回的 pair 用 QQ 侧释放家族释放，禁止跨 allocator——**有可核对合同**。 | 依赖 JS 对象句柄与 isolate 寿命；D8 的监听器保持语义已现场验证，但 Close 分类缺陷（C7/C10）未修。 |
| **取消** | **未闭合**（stop/cancel/drain 无合同）→ 主动取消原语不准入；结果分类维持 three-tier（Success/Failure/Unknown），delivery_unknown 不重发。 | 无独立取消证据；同样依赖 three-tier 分类。 |
| **身份** | 服务 getter pair 语义闭合；运行时 switch 分支有现场证据（本决定 §1）；transport 未连接时同步失败（token=0）可作 not-ready 探测。 | 绑定 host/env/current 检查（`host_adapter.rs`）；身份=JS 上下文有效性，不产生原生代次。 |
| **消息层成本** | P5 待做：protobuf 最小编解码 + 业务回执关联（独立于本决定；两条路线成本相同）。 | 同左；另需修 C6（空 native ID 不得算 Success）。 |
| **既有缺陷** | 无既有实现包袱（P4 从零建 adapter，unsafe 集中）。 | C6/C7/C8/C10 未修；选 V8 须先修这些 + 补等价线程证据。 |
| **加载 vs 原生引导的区别** | 加载=K1 注入器（不变，零修补）；**原生引导**=在准入线程上调 `72DE38` 取服务 pair + 观测 transport 状态——不需要 V8 isolate/context，与 bootstrap（JS 首入）彻底解耦。 | 引导=JS 首入 + env 获取 + 监听安装（D7/D8 链）。 |

## 3. 选择理由（证据句柄）

1. 原生路线的两大 P1 未决（发送实际目标、dispatcher 身份）已在实例 47524 上**现场+静态交叉闭合**（thread contract §10）；V8 路线的遗留缺陷（C6–C8/C10）仍是代码事实。
2. 现场证据表明运行时实际走 MSFService 族、CoreService 未实例化、`0x750088` 执行器单例不在运行路径——原生路线的必要面比静态预估更窄、更可控。
3. 线程模型上，原生路线的全部下游提交都经 dispatcher（含显式跨线程唤醒），对"自有线程提交"是结构性支持；V8 路线则要求一切操作挤进 JS owner 线程，与计划 §3 的"回调里只做校验/复制/有界入队"分工冲突更多。
4. 计划 §7-P2 通过门核对：G-ABI/G-THREAD/G-LIFE 文档合同齐备（两份 contract 文档 + §10 增补）；profile 的未知能力全部显式拒绝（capability-profile.json）；加载/接入/登录/收发分别标状态（profile `.gates`）。

## 4. 本决定的边界（防止越权解读）

- 本决定**不解封任何现场发送**：G3 仍锁，直到 P4 LAB（T11–T16）+ P5 编解码 + G1/G2 逐门通过。
- "有条件准入"的线程项不等于"已准入"：P4 的自有 adapter 必须先在**自建假 ABI 宿主**上验证 move/copy/disposer/回调语义（计划 §7-P4 第一步），再进研究构建；真实 QQ 上的首次调用是 P6 G1 的受控观测。
- 若 P4/P5 期间出现本决定未预见的合同违背（如 G1 观测显示 732A80 存在隐藏线程要求），按 §5 变更记录流程回退，不静默切换 V8。

## 5. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-08 | 初版：选定原生 MSF/SSO，服务族 MSFService | thread contract §10（47524 三次采样）、lifetime contract、R2 报告 |
