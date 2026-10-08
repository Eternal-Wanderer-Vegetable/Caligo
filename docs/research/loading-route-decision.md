# 加载路线决定(loading-route-decision)

> K1 交付物。决定冻结于 2026-10-06。变更须按计划 §3.3 作方案变更记录。

## 1. 本轮决定

**采用:自有 Rust 加载器 + 自有 Rust bridge(cdylib),运行时注入,零文件修补,零第三方 loader。**

- 加载器:`crates/caligo-cli/src/winutil.rs::inject_and_probe`(自有实现)。
  流程:OpenProcess → VirtualAllocEx/WriteProcessMemory(bridge 路径)→ CreateRemoteThread(kernel32!LoadLibraryW)→ 快照定位基址 → ReadProcessMemory 解析远程导出表找 `caligo_probe_run` → 远程调用取回探测报告 → 释放。
- bridge:`crates/caligo-bridge`(cdylib)。DllMain 最小化(不建线程、不等待);探测由 loader 显式触发;报告含协议版本/自身路径/PID/模块快照,作为"已加载、可握手"证据。
- 门控(T01/T02 逻辑):manifest 摘要核对通过 + 执行者显式 `--confirm-designated-test-instance`,两者缺一不可;实现于 `caligo-cli` 的 inject 命令。

**状态:已验证(2026-10-06)。** 注入器先在自建牺牲进程完成干跑,随后按双门控加载进指定测试实例
(PID 27992):远程加载成功、远程导出解析定位 `caligo_probe_run`、探测报告回读、握手 PASS,
QQ 进程存活且响应正常(证据:identity-invalidation-observations §EXP-K1-01、local-evidence/k1-live/)。
会话入口层的调查继续按 §3 进行,probe 仍为只读。

## 2. 已评估的分支与拒绝理由

| 路线 | 决定 | 理由 |
|---|---|---|
| 文件修补 / preload 注入(LiteLoaderQQNT 式) | **拒绝** | 落盘修改增加恢复面(与 recovery-notes 冲突);污染官方安装;升级易碎 |
| 第三方 loader / 授权服务 | **拒绝** | 计划 §3.3 来源纪律;不可核对的二进制不能成为运行依赖 |
| 多版本自动适配 | **拒绝** | 计划 §6.1:先只维护一个已验证版本;歧义时拒绝 |
| bridge 热卸载 | **不承诺** | K1 默认以退出测试 QQ 实例收尾(recovery-notes §2) |

## 3. 会话入口层的路线修正(重要)

静态证据(见 native-entry-contract §2)表明:会话/消息入口不走 PE 导出表——major.node 零导出,
业务绑定在运行时注册进 Node 运行时(QQNT.dll)。因此:

- **probe/加载层**(K1 范围):纯 Rust 路线成立,维持上述决定;
- **会话入口层**(K2 起):预计需要"自有 Rust bridge 内嵌极薄 Node-API 适配"——在 bridge 内经
  QQNT.dll 导出的 napi_*/注册通道取得运行时上下文。这是计划 §3.3 预留的条件分支,本轮证据
  (qq_magic_napi_register、node::AddLinkedBinding、qq-proton.node 的标准 napi 注册形态)支持
  把它列为**待实测主线索**,但:
  - 仍未解决:外部注入 DLL 如何取得有效 `napi_env`(不经过 Node 的 dlopen 路径);
  - `qq_magic_napi_register` 语义未知,必须先做无副作用观测实验;
  - 在以上两点闭合前,**不改**本轮加载决定,也**不引入**任何 C++ shim;
  - 若实测后需要 C/C++ shim,先写方案变更记录(必要性、对象生命周期负责方、来源、构建、替代方案),不默认接受。

## 4. 变更控制

本文件是加载路线的唯一决定记录。任何偏离(引入 shim、更换注入原语、支持多版本)必须:
1) 在本文件追加"变更记录"小节;2) 更新 source-register;3) 更新 version-adapter-manifest(如涉及版本)。

## 4. R3 增补：原生路线冻结不影响本决定（2026-10-08）

K4 原生路线已冻结（`k4-native-route-decision.md`）：选定原生 MSF/SSO，服务族 MSFService（getter `72DE38`），配套 `docs/contracts/qq-9.9.33-52230.capability-profile.json`。

对本文件约束的影响：

- **加载决定不变**：仍是自有 Rust 加载器 + 自有 bridge（§1），原生路线同样经该注入器进入；capability profile 的 `gates.loading = admitted` 即指此层。
- **新增概念区分**："加载"（bridge 进 QQ 进程）与"原生引导"（在准入线程调 `72DE38` 取服务 pair + transport 状态观测）从此解耦——原生引导不需要 V8 isolate/context，不再受 §3 会话入口层 Node-API 线索的约束；§3 的 napi 线索降级为 V8 备选路线资产的一部分。
- 原生引导的准入判据 = observe-msf 锚点核对（profile `resolver.method`）；锚点不匹配 → unsupported-build，拒绝全部原生能力。
