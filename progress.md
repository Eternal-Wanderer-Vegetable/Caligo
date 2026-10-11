# Caligo 当前进度与验证

## Current State

**Last Updated:** 2026-10-12（Asia/Shanghai）
**Baseline commit:** `0f5b483`；本轮从 `b2b16cf` 继续，保留既有未跟踪文档 harness。
**Active Feature:** N4-R3 回执期限与未知结果语义已离线补齐；R5 普通 caligod loopback HTTP 动作入口现已接入并通过 fake-daemon 闭环和慢读并发测试。CPB 稳定身份仍 unsupported；R2 native ABI/线程/排空门、R4 现场、R5 私聊身份与原生 projection，以及 R6 event/WS transports 和应用验收仍未闭合，feat-005/006 仍 blocked。
**Scope:** 继续执行 [N4 响应恢复计划](docs/plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md)；完成可由离线证据支持的回执恢复行为，不越过 native 生命周期和现场准入门。

## 当前产品验收边界

依据：[N 阶段台账](docs/execution/2026-10-09-snowluma-parity/ledger.md) 的
2026-10-11（三十九）至（四十一），以及当前源码；这里只核对仓库记录，未刷新运行现场。

| 能力 | 已有证据 | 尚未闭合 |
|---|---|---|
| 普通文本 wire/直调研究 | 共享 codec、golden、历史 r9/r18 送达与 errCode 通道 | 完整常驻链路、稳定关联与生产矩阵 |
| consumer 接线 | `61f9d19` 的 cookie→响应槽→session→resident；离线合同有记录 | resident 现场响应回程与稳定存活 |
| 常驻 bridge/core | `0f5b483` 的 Hello/attach、事件上行、gen 拒绝/幂等恢复、发送到 NativeStarted | 响应到达前实例损失，`consumer_result=0`，死因未定；现场硬停 |
| OneBot/消息索引 | 解析、事件转换、持久索引与普通 caligod loopback HTTP 动作入口；合成群发送等待强回执和已持久 ID | 原生 receive projection、私聊 UID 映射、HTTP POST events/WS、真实应用互通、稳定消息关联与持续运行 |
| @ 与其他消息段 | legacy-path @ 的 1006501/无 ack 负结果有记录 | Caligo 自有支持未通过；服务端/包/session 原因仍是假设 |

**项目状态：NOT READY（生产 MVP 未验收）。** 不沿用台账的“代码 100% / 现场 80%”
百分比作为完成依据；本仓库用工作块完成门与证据层级追踪。
工作块状态见 [feature_list](feature_list.json)，具体门见
[N 计划](docs/plans/2026-10-09-gitnexus-plan-caligo-snowluma-capability-parity.md)。

## Blockers / Risks

- 离线修复已贯通 request_id、NativeStarted 边界、AcceptedUnconfirmed 与旧 journal 保守恢复。2026-10-12 follow-up 使非零 callback code 保持 unknown 并保留原始 errCode/errStr 诊断；core 将剩余响应期限随 v3 Dispatch 下发，bridge 超时后发 DeliveryUnknown，并为同一请求保留最多 60 秒、最多 256 项的迟到强身份更正槽。无稳定 native identity 的成功仍 unsupported/unknown，不自动重发；实现与验证见执行记录。
- R2 已基于磁盘文件与冻结样本同 hash（`63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）静态确认 `1DB2(pair*)` release、`732A80` body 同步深复制及 `B75029` 输入释放；代码按代复用单个 service pair、body 调用后对称释放、去掉未使用的 self-string 副本。常驻与一次性 callback context 在发布前 PIN bridge image；resident 停止时本地关闭 callback 准入、等待已准入调用退出，隔离后迟到 callback 仅计数、不入槽。离线验证不证明 QQ consumer 停止、原生队列排空或最终业务回执持久化。callback manager 其它操作、owner/回调线程、QQ stop/cancel/drain/quiescence 和历史 AV 原因仍未证；产品仍 blocked。
- R3 使用 core journal 作为常驻候选 wire sequence authority，见 ADR；CPB 只有弱文本候选，stable identity unsupported，同步 send 不能返回成功。
- CPB identity follow-up 移除了 `cpbseq:{seq}` 伪 native ID。当前 decoder 不产生稳定 native ID 或 sender UID→UIN 映射；缺少这些证据时只计 `identity_blocked`，不向 resident 事件流发布。定向 gate 1/1 与 `init.ps1 -Mode all`（default 191/191、research 227/227）通过；详见 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。这关闭伪身份入口，不关闭 receive/OneBot 功能或 R4 门。
- R5 core/store 子块现已接入：数值 account 的 Daemon 打开 sibling MessageStore；AccountActor 在 journal 后、ACK 前持久化显式 OneBot projection，重启时幂等重建索引，Daemon `StoreReader` 与 OneBot `get_msg` 共用该索引。普通构建 `caligod` 现启动 loopback-only HTTP listener，共用动作层和 Daemon；标准群发送只在强 native 回执及匹配的持久 OneBot ID 均可读后成功，私聊映射未证时明确 unsupported。`daemon_client` 当前仍提供 `None` projection，不能声称 native receive 可查询；HTTP POST events、WS/反向 WS、私聊 peer UID 映射与真实应用互通未完成，feat-006 仍 blocked。
- R4 现场完成门不满足；保留最新现场硬停、钩子路线否决、旧实例不可复用。
- 部分 research/profile 文件仍有“实现未开始/阻塞 retCode”等旧描述。
  [docs 索引](docs/README.md) 指向后续台账；历史原文保留，能力 gate 未自动升级。
- 历史 research 全仓首次检查出现 bridge lib `STATUS_ACCESS_VIOLATION (0xc0000005)`；原始日志只显示前两项 `asyncrun` 测试通过，未留下故障线程/地址。本次单独复跑 research bridge lib 串行 69/69、默认并行 69/69 均通过，结果不能解释或关闭历史失败；未找到对应 WER、匹配 dump 或可用 `cdb`/`procdump`。本轮发现一份更早的 local-only QQ Crashpad dump，文件头为 `0xE06D7363`（C++ exception），无证据表明它对应 research AV，故不用于归因；`0xc0000005` 继续作为稳定性 blocker。

## 前次文档工作（What's Done / In Progress）

- 指定 `npx skills add walkinglabs/learn-harness-engineering --skill harness-creator` 已成功。
- 使用仓库内 skill 的 scaffold 建立初始文件，再按 Rust/Windows 实际合同替换占位内容。
- 创建短 AGENTS、四份按需专题、完整历史文档导航、feature 完成门与交接。
- 增加 PowerShell 验证入口、Git Bash 委托与轻量文档检查。
- 文档、状态、入口与差异检查通过；两种构建的实际结果及首次失败记录如下。

## Verification Evidence

| 检查 | 命令 | 本次结果 |
|---|---|---|
| skill 安装 | 用户指定的 npx 命令 | 成功；仓库 `.agents/skills/harness-creator` 与 skills-lock 已生成 |
| 上游结构审计基线 | `node .agents/skills/harness-creator/scripts/validate-harness.mjs --target . --json` | 改动前 20/100；只衡量五类模板结构，未衡量已有历史证据质量 |
| 文档/状态检查 | `.\init.ps1 -Mode docs` | 通过：9 份维护中文档、154 个本地链接、32 个历史 Markdown 入口、10 个工作块；AGENTS 63 行，四份专题 55–67 行 |
| 两种构建及 fail-fast 入口 | `.\init.ps1 -Mode all` | 普通构建 176 passed / 0 failed；research 首个 bridge lib 进程以 0xc0000005 终止，脚本返回 1，没有掩盖失败 |
| bridge 单二进制诊断 | `cargo test -p caligo-bridge --lib --features research --locked --offline --target-dir target-harness -- --test-threads=1` | 58 passed / 0 failed；不能据单次通过定案为并发原因 |
| research 入口复查 | `.\init.ps1 -Mode research` | 202 passed / 0 failed；首次崩溃仍未解决 |
| Bash 委托入口/语法 | `bash ./init.sh -Mode docs`、`bash -n ./init.sh` | 使用本机 Git Bash 执行，均通过；仓库内 LF 属性已固定 |
| 检查脚本语法 | `node --check scripts/check-docs.mjs` | 通过 |
| 上游结构审计终验 | `node .agents/skills/harness-creator/scripts/validate-harness.mjs --target .` | 100/100，五类结构各 5/5；未做 agent 成功率对照实验 |
| 差异检查 | `git diff --check` | 通过；Rust 和历史记录无差异 |

本机原始输出保存在被忽略的 `target-harness-verification.local.md`、
`target-harness-bridge-diagnostic.local.md` 与 `target-harness-research-recheck.local.md`。
它们属于本次本机验证证据，不作为干净 checkout 必须存在的文档依赖。

## 2026-10-11 N4 解阻调查

- 以下记录保留实施前调查结论，作为历史基线；实施状态与当前缺口见后文的 [N4 响应恢复执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- 源码确认：`HostOp::SendText` 不携带 daemon request_id，native adapter 另造 ID，响应将被 actor 作为未知请求审计；`NativeStarted` 当前在 submit 受理时提前上报。
- 源码确认：`BusinessAccepted` 仍跨 IPC 升格为无稳定 ID 的 ConfirmedSuccess；getter pair 未成对释放、seq 持久化为空且 wire seq=0、响应 terminal 未统一清理均需处理；不据此判定实例死因。
- 三个纯离线反例均复现：不同私聊 user_id 生成相同载荷；fake adapter 的内部响应 ID 被 resident 直接转发；HTTP 先收头后收 1024 字节正文在 onebot_http.rs:111 触发 debug 整数下溢。
- 现有定向测试仍通过：core OneBot 10 passed；research consumer 串行 2 passed。它们没有覆盖上述反例，也没有消除前次 research 访问冲突。
- GitNexus 已刷新到当前 HEAD（15,903 nodes / 43,263 edges）；Rust PDG 有数据，但图调用边存在遗漏及截断，结论已用源码核对。
- 本轮入口 `.\init.ps1 -Mode docs` 通过（9 文档 / 154 链接 / 32 历史入口 / 10 工作块）；交付后复核同样通过：9 文档 / 156 链接 / 33 历史入口 / 10 工作块；git diff --check 通过。
- 本机合成探针及原始输出：`E:/stella/_reference/caligo-n4-recovery-20261011/`，未复制进仓库；QQ 现场零动作。

## Next / 下一工作入口

按 [会话交接](session-handoff.md) 继续核对 callback manager/thread/drain 与历史 AV 证据，并从固定-hash CPB/native record 研究稳定 native ID 和 sender UID→UIN 映射。R5 core/store/普通 HTTP listener 已有离线证据；R6 event/WS transport 与真实客户端矩阵仍待办。强 identity 与现场生命周期门未满足前不开始 R4。旧实例、旧钩子和旧 unknown 请求均不能作为继续实验或自动重发的入口。

## 2026-10-11 N4 响应恢复执行

- 详细实现、资源所有权表、验证与未闭合门：[执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- R1 离线恢复合同覆盖原 request ID、真实调用边界、持久结果 ACK、协议语义 revision 与旧 `Success(None)` journal 保守恢复；默认与 research workspace 均已验证通过。
- R2 callback capture 采用固定代次上下文与有界槽，研究回调不写文件；固定-hash pair/body ownership 已先行补齐，manager ABI/线程/drain 合同仍开放。历史 AV 仍 unknown。
- R3 wire sequence authority 已写 [ADR](docs/decisions/2026-10-11-wire-sequence-authority.md) 并做离线重启测试；强 native identity 仍 unsupported。
- R2/R3 后续静态补齐：`1DB2` 与 `2632` 区分，service lease 在 probe→attach→adapter 间复用，body 用同一 QQ allocator pair 释放；`g3_session` 的空地址 `probe_ready(0)` 阻断已移除；同文记录重复/歧义 fixture 不消费发送登记。详细证据与验证见 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- R2 生命周期 follow-up：`consumer_probe::leak_ctx` 在共享入口 PIN bridge image，覆盖 `native_resident` 与 `g3_send_run` 两条 callback delegate 路径；resident stop 增加代次隔离/in-flight callback 准入栅栏，并有界 join Caligo 自有 pump、receive、daemon worker；未登记 generation 走 orphan 计数且不入共享槽。consumer quarantine 7 项测试与 owned-worker join helper 单测通过；QQ dispatcher 的 stop/cancel/drain/quiescence、完整 manager ABI 与历史 AV 仍开放，不作为 R4 准入。
- AV 后续复核：原始记录为 `caligo_bridge-c00aae2a8f83540a.exe` / `0xc0000005`，只看到前两项 `asyncrun` 测试通过；本次 research bridge lib 串行与默认并行各 69/69，通过但未复现，不能归因为已修复。无 WER 事件/dump 或调试器堆栈；忽略输出在 `target-harness/n4-av-serial-recheck.log`、`target-harness/n4-av-parallel-recheck.log`。
- R5 typed target、动作层、append-only durable MessageStore、分片安全 HTTP parser 与普通 caligod HTTP listener 已实现/离线测；core AccountActor/Daemon 的接收索引和 get_msg 已装配并有 pipe 集成证据。HTTP 使用 loopback bind、有限 worker/read deadline；慢头客户端不阻塞另一请求。bridge projection 仍为 None，私聊 peer UID 映射、HTTP POST event/WS/反向 WS 与真实应用互通未完成，feat-006 继续 blocked。
- 初次默认并行 full suite 的 allocator 资源断言受其它测试线程干扰；将断言计数改为当前测试线程局部后，`resident_lifecycle` 默认并行 18/18 通过。该结果不解释此前 0xc0000005。
- 当前项目仍 NOT READY；R4 未启动，R6 应用验收未完成；下一步见本页 Next 与 `session-handoff.md`。

## 2026-10-12 N4 R3 回执期限补齐

- `1006501` 等非零 callback code 不再直接解释为业务拒绝：结果为 `DeliveryUnknown`，v3 诊断保留原始 code/text，避免把本地网络观察误报成确定失败。
- core 依据原始受理时间下发剩余 timeout；bridge ledger 在期限到达时发出终态 unknown，不重派请求。未知记录在有界 60 秒宽限期内仅允许同 request 的强身份成功更正，容量上限 256；到期清理记录槽。
- `Unknown(None)` 与 v2 framing 保持兼容边界；v3 business protocol revision 提升为 2，旧 revision 明确拒绝。审计写 journal 失败时拒绝吞掉诊断错误并使 actor 降级。
- `.\init.ps1 -Mode all` 通过：docs 9 份 / 163 个本地链接；默认与 research 两套 workspace 的测试二进制均成功退出。该结果只证明离线合同，不证明 native callback 线程/寿命、强身份或现场响应。
- R2 callback manager 完整 ABI、owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence 与历史 AV 根因仍开放；R3 stable native identity 与 sender UID→UIN 未建立。R4 不准入，未启动 QQ 或改变 SnowLuma 连接。
