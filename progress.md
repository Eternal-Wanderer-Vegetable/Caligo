# Caligo 当前进度与验证

## Current State

**Last Updated:** 2026-10-14（Asia/Shanghai）
**Baseline commit:** `0f5b483`；本次静态追查起点 `59b8146`，保留既有未跟踪 harness 文件。
**Active Feature:** N4-R2。用户已授权使用指定测试 QQ 实例；同一固定 profile 已完成五次外部只读 `observe-msf`。五次全局 dispatcher 对象都命中一条线程 TLS 项，`+0x58` DWORD 位型归一后相同且不匹配目标线程；T4 可访问系统线程枚举亦无匹配，静态 IAT 证实该字段读取 `GetCurrentThreadId`。修正 collection 的 pointer-backed vector 布局后，T5 全可读映射扫描未找到匹配已知 worker-context 双 vptr 与同一 shared pair 的成员候选，故 membership 仍 unknown。callback consumer、manager 完整 ABI、退出/迁移与 stop/cancel/drain/quiescence、旧 AV 根因未闭合。R3 stable native identity 仍 unsupported；feat-005/006 仍 blocked，R4 发信门未满足。
**Scope:** 在 [N4 响应恢复计划](docs/plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md) 范围内，对指定实例完成五次固定 hash 的外部只读校准及受限只读内存取证；零注入、零目标写入、零 QQ 函数调用、零消息发送、零聊天打开，SnowLuma 未操作。R1–R3 完成门未满足，不执行 R4 native send。

## 当前产品验收边界

依据：[N 阶段台账](docs/execution/2026-10-09-snowluma-parity/ledger.md) 的
2026-10-11（三十九）至（四十一），以及当前源码；这里只核对仓库记录，未刷新运行现场。

| 能力 | 已有证据 | 尚未闭合 |
|---|---|---|
| 普通文本 wire/直调研究 | 共享 codec、golden、历史 r9/r18 送达与 errCode 通道 | 完整常驻链路、稳定关联与生产矩阵 |
| consumer 接线 | `61f9d19` 的 cookie→响应槽→session→resident；离线合同有记录 | resident 现场响应回程与稳定存活 |
| 常驻 bridge/core | `0f5b483` 的 Hello/attach、事件上行、gen 拒绝/幂等恢复、发送到 NativeStarted | 响应到达前实例损失，`consumer_result=0`，死因未定；现场硬停 |
| OneBot/消息索引 | 解析、事件转换、持久索引；普通 caligod loopback HTTP、HTTP POST events、正向/反向 WS 共用动作层；协议合同有本机合成测试 | 原生 receive projection、私聊 UID 映射、真实应用互通、稳定消息关联与持续运行 |
| @ 与其他消息段 | legacy-path @ 的 1006501/无 ack 负结果有记录 | Caligo 自有支持未通过；服务端/包/session 原因仍是假设 |

**项目状态：NOT READY（生产 MVP 未验收）。** 不沿用台账的“代码 100% / 现场 80%”
百分比作为完成依据；本仓库用工作块完成门与证据层级追踪。
工作块状态见 [feature_list](feature_list.json)，具体门见
[N 计划](docs/plans/2026-10-09-gitnexus-plan-caligo-snowluma-capability-parity.md)。

## Blockers / Risks

- 离线修复已贯通 request_id、NativeStarted 边界、AcceptedUnconfirmed 与旧 journal 保守恢复。2026-10-12 follow-up 使非零 callback code 保持 unknown 并保留原始 errCode/errStr 诊断；core 将剩余响应期限随 v3 Dispatch 下发，bridge 超时后发 DeliveryUnknown，并为同一请求保留最多 60 秒、最多 256 项的迟到强身份更正槽。无稳定 native identity 的成功仍 unsupported/unknown，不自动重发；实现与验证见执行记录。
- R2 已基于冻结磁盘样本 hash（`63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）静态确认 `1DB2(pair*)` release、`732A80` body 同步深复制与 `B75029` 输入释放；代码按代复用单个 service pair、body 调用后对称释放、去掉未使用的 self-string 副本。常驻和一次性 callback context 在发布前 PIN bridge image；resident 停止时本地关闭 callback 准入、等待已准入调用退出，隔离后迟到 callback 只计数、不入槽。2026-10-13 Ghidra follow-up 已映射 `73BC5C/73C2AA` op 0–3、确认 heap-fallback closure 同尺寸分配/释放，并补强 `0x54CE00` assignment 路径中的 op=1 copy / op=3 dispose；`0x3BC545C` 所读 `httpx::HttpConnection+0xA0` 是 manager-managed callback wrapper；owner 由 RTTI/ctor/dtor/state-dispatch 交叉确定，实例动态 manager 值与 empty→non-empty 赋值 writer 尚未解析。2026-10-14 后续静态复核确认两张 manager 表另有 op=4 输出清零分支，并在固定 hash 上筛出 9 个同 manager-slot/16-byte 参数形状的 op=2 调用点（7 个函数）；其中两个还先在同一 slot 调用 op=4 helper。它们是高置信 manager-ABI-shaped 候选，动态函数指针值尚未逐一绑定到 `73BC5C/73C2AA`。此前 bounded `[reg+0x10]` 筛查未命中这些编码形状，不再表述为“未找到可归属 caller”；完整调用者、动态 table 身份、arena 分支寿命、owner/回调线程、QQ stop/cancel/drain/quiescence 和历史 AV 原因仍未证。`0x3FCE5E` 仅能称 op=4 输出查询 helper，不能据返回值声称 callback presence。静态与离线证据不证明 QQ consumer 停止、原生队列排空或最终业务回执持久化；产品仍 blocked。
- R3 使用 core journal 作为常驻候选 wire sequence authority，见 ADR；CPB 只有弱文本候选，stable identity unsupported，同步 send 不能返回成功。
- CPB identity follow-up 移除了 `cpbseq:{seq}` 伪 native ID。当前 decoder 不产生稳定 native ID 或 sender UID→UIN 映射；缺少这些证据时只计 `identity_blocked`，不向 resident 事件流发布。定向 gate 1/1 与 `init.ps1 -Mode all`（default 191/191、research 227/227）通过；详见 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。这关闭伪身份入口，不关闭 receive/OneBot 功能或 R4 门。
- R5 core/store 子块现已接入：数值 account 的 Daemon 打开 sibling MessageStore；AccountActor 在 journal 后、ACK 前持久化显式 OneBot projection，重启时幂等重建索引，Daemon `StoreReader` 与 OneBot `get_msg` 共用该索引。普通构建 `caligod` 的 loopback HTTP、HTTP POST events、正向/反向 WS 共用动作层和 Daemon；HTTP POST quick operation 完成后才扇出到 WS，订阅者队列有界。标准群发送只在强 native 回执及匹配的持久 OneBot ID 均可读后成功，私聊映射未证时明确 unsupported。`daemon_client` 当前仍提供 `None` projection，不能声称 native receive 可查询；真实应用互通、私聊 peer UID 映射与稳定身份未完成，feat-006 仍 blocked。
- T1–T4 于 2026-10-13 在用户指定测试实例完成四次 `caligo-cli observe-msf`；observer 仅 `QUERY_INFORMATION | VM_READ`，原始报告与日志保存在 `local-evidence/n4-r2-field-20261013-224510/`。四次均为同一进程代次、固定 `wrapper.node` SHA-256 匹配、MSFService 锚点匹配、TLS index 有效、145 条线程中 41 条装有同一 dispatcher vtable、manager 链各观察到 2 次 visit；global dispatcher 地址四次均匹配一个线程 TLS dispatcher 项。`field_58_tid` 改为 `u32` 后复采，DWORD 位型四次一致且均不匹配目标线程，T4 的 8,874 个可访问系统线程 ID 枚举也无匹配。T5 于 2026-10-14 在同一代次/profile 再次确认 global dispatcher 与一条 TLS 项相同、`+0x58` DWORD 相同且不匹配目标线程；T5 原始文件位于 `local-evidence/n4-r2-field-20261014-000619/`。静态 writer `0x31F90B0` 从 `GetCurrentThreadId` 写入 `ThreadTaskManagerImpl` dispatcher `+0x58`；RTTI 后续确认 `+0x48` 是该类的 pointer-backed task vector，`0x217D58C` 填充、`0x217D8CC` 遍历、析构释放。尚未证明 global dispatcher 属于该 vector、线程退出清理/迁移或 callback drain。没有 callback invocation 或 native send；consumer 执行线程、stop/cancel/drain/quiescence、R3 identity 与历史 AV 仍开放。
- R4 现场完成门不满足；本轮只完成外部只读校准，没有注入/发送。保留最新现场硬停、钩子路线否决、旧实例不可复用。
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
| N4-R2 observer DWORD 读取 | `cargo build -p caligo-cli --locked --offline --target-dir target-harness` | 通过；`field_58_tid` 改用 `u32`，T4 只读复采成功；保留已有 warnings，未运行测试 |
| 2026-10-13 N4 文档门 | `.\init.ps1 -Mode docs` | PASS：9 live docs、172 本地链接、33 个历史入口、10 个 feature |
| 2026-10-13 差异门 | `git diff --check` | PASS：observer unsigned DWORD 与本次维护文档改动 |
| 2026-10-13 Docker GitNexus | `gitnexus analyze --force --index-only --pdg`; `status --json`; `detect-changes --scope all` in `caligo-gitnexus` | final status up-to-date，runner/content current，无 incomplete reasons；detect 命中 5 个预期文件与 2 条 observer flow，risk medium 来自全工作树范围；原始状态与检测输出在忽略日志 |

本轮 GitNexus 仍报告 callable-flow 候选超 cap、536 个入口候选中 336 个未入流程、902 个 callee 分支跳过、14 次 walk 达预算，以及局部 REACHING_DEF/CDG 缺口；因此缺失边或 flow 不作为“没有调用”的证据。

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

2026-10-14 bounded static follow-up：有效 COL/vtable 链把 `0x9C509C`、`0x345763E`、`0x3456C1C` 分别归属 `FileDragManagerWin` slot 5、`IgnoreBodyHttpRequestHandler` slot 3、`SaveToStringHttpRequestHandler` slot 3；也找到相应 vptr writer。`0x345745A` 暴露一处 op=1/16-byte 初始化形状。enclosing class 已识别，但这些方法内的 nested manager slot 动态值仍未绑定到 `73BC5C/73C2AA`；`0x3BC545C` 的 `httpx::HttpConnection+0xA0` owner 已解析，但字段赋值 writer/实例动态 manager 与外层析构等仍 open。证据与边界见 [寿命合同 §7.8](docs/research/qq-native-lifetime-contract.md) 和 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。

## Next / 下一工作入口

按 [会话交接](session-handoff.md) 继续固定-hash callback manager op=1/op=2 调用合同与新发现的 op=2 ABI-shaped caller、manager 外层析构 owner，以及 global dispatcher 对 worker 集合归属与 `field_58_tid` 生命周期证据；op=2 与 op=4 增量见 [寿命合同 §7.6](docs/research/qq-native-lifetime-contract.md#76-callback-manager-op4-与-op2-abi-调用点-补充-2026-10-14-静态)。另从 CPB/native record 研究稳定 native ID 和 sender UID→UIN。R5/R6 离线接线已有证据，真实应用矩阵仍待办。R1–R3 与 stop/cancel/drain/quiescence 门未满足前不进行 R4 发信；unknown 不重发，SnowLuma 保持不触碰。

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

## 2026-10-12 N4-R6 transport 补齐（离线）

- 已将 HTTP POST events、正向 WebSocket、反向 WebSocket Universal client 接到普通 `caligod`，三种 transport 与 HTTP API 共用单一 `action_handler`。事件必须先由 AccountActor journal/MessageStore 接受，且 message event projection 字段齐全，才进入 OneBot fanout。
- HTTP POST 签名与 bounded response、quick-operation 回复、正向 WS `/api`/`/event`/`/`、反向 WS V11 headers/reconnect 均有合成协议测试。HTTP POST 配置时，caligod 等待 post subscriber ready；快速操作响应完成后再将该事件扇出至 WS。event queues 容量 256，forward WS clients 上限 16。
- `cargo test -p caligo-core --locked --offline --target-dir target-harness` PASS；transport 6/6、pipe/journal event roundtrip 及 store/query 测试通过。最终 `.\init.ps1 -Mode all` 第三次运行 PASS：docs 9 / 163 links / 33 history entries / 10 features；普通与 research workspace 测试二进制均通过。
- 首次全仓运行中 bridge lib 测试进程曾以 `0xc0000005` 退出；默认构建串行诊断 58/58 通过，但旧 AV 根因未解。第二次运行暴露 `daemon_chain` 的共享静态 native ID counter 并行污染；已改为每个 fake Host 自有计数，定向/并行 `daemon_chain` 5/5 通过，随后全仓 all 通过。保留两次失败记录，第三次绿色不覆盖首次 AV。
- 本次变更 Rust 路径 rustfmt 检查、`git diff --check` 与 feature JSON 解析通过。全仓 `cargo fmt --all -- --check` 报出既有多路径格式差异；未格式化 scope 外文件。
- 现场边界没有变化：native `daemon_client` projection 仍为 `None`，稳定身份与私聊 UID→UIN 未证；真实 OneBot 应用收发/get_msg/慢连接矩阵未跑。R2 ABI/thread/quiescence 与 R3 identity 门未闭合，R4 不准入；feat-005/006 blocked，产品 NOT READY。无 QQ/注入/真实发送/SnowLuma 动作。

## 2026-10-13 N4-R2 callback manager 静态分支补充

- 从固定 hash `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 的磁盘样本定向导出 `73BC5C` inline 与 `73C2AA` heap manager；两张相对跳转表的 op 0–3 都有对应分支入口。op=1 的两种布局及 op=2/3 cleanup 差异记入 [寿命研究增量](docs/research/qq-native-lifetime-contract.md#72-callback-manager-operation-dispatch-静态补充-2026-10-13) 和 [N4 执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。本地原始 Ghidra 导出保存在 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-dispatch\`，未入库。
- 这是固定样本代码形状证据；op=1/op=2 的调用者与合同、owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence 仍未闭合，旧 `0xC0000005` 仍无关联 dump/故障栈。R4 不准入，feat-005/006 保持 blocked，产品保持 NOT READY。未启动、读取或操作 QQ 实例，未改变 SnowLuma 连接。
- 文档检查与 `git diff --check` 为本轮适用门；本轮未改 Rust、未运行 Cargo 测试。

## 2026-10-13 N4-R2 response invoker ABI 修正（离线）

- 针对冻结 `wrapper.node` hash `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 的 `73BD6C` call site，静态反汇编确认 Windows x64 下是 5 参调用：`inner, 0x10, errCode, errStr, responseUid`；只设置第五参 `[RSP+0x20]`，无第六参写入。证据与 [寿命合同 §7.3](docs/research/qq-native-lifetime-contract.md#73-73bd6c-用户-invoker-五参-abi-与-rust-对齐-2026-10-13) 和本机 Ghidra 导出一致。
- 将 `consumer_invoke` 对齐为五参 ABI。第五参 UID 指针暂作 opaque、不读取；移除错误地当作第六参 span 指针的解析与 `span_shape` 日志，并保留回调错误码/错误文本捕获。单测以函数指针类型锁定 ABI，并传不可读第五参验证它不被解引用。
- 该证据只闭合一个 callback ABI 子项；manager op 1/2 调用合同、当前 owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence、历史 AV 根因、R3 stable native ID 与 sender UID→UIN 仍未闭合。R4 未执行；本轮未对 QQ 调用 native API 或发送消息，SnowLuma 仅作静态参考，feat-005/006 继续 blocked，产品 NOT READY。
- `cargo test -p caligo-bridge --lib --features research consumer_probe::tests --locked --offline --target-dir target-harness -- --test-threads=1` PASS（7/7）；本次 `rustfmt --edition 2021 --config skip_children=true --check crates/caligo-bridge/src/consumer_probe.rs crates/caligo-bridge/src/g3.rs` 在格式化后 PASS；`.\init.ps1 -Mode all` PASS，docs 9 live docs / 168 local links，普通与 research workspace 全部完成。构建存在既有 warnings。
- `git diff --check` PASS。GitNexus 1.6.12 在 7 个 tracked dirty files 下重建索引，commit `80d6d95b0cbf652812170d53c833da0d7934221c`、runner/current-content 皆 current、无 incomplete reasons；调用流候选/REACHING_DEF/流程截断警告仍按不完备图处理。post-commit final detect 输出记录于最终答复；本轮未对 QQ 调用 native API 或发送消息。

## 2026-10-13 N4-R2 heap-fallback closure 分配/释放配对（离线）

- 再次核对冻结磁盘 `wrapper.node` SHA-256，仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Ghidra 12.1.4 `-noanalysis` 定向导出 `0x24C0`、`0x1FD0`、`0x1FE0`、`0xB75000`、`0xB75029`；原始静态输出位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\allocator-helper-analysis\followup-20261013\`，不入库。
- `73BBF8` 的 heap fallback 以 `24C0(0x40)` 分配，`24C0` 跳到 `B75000`；heap manager op=2/3 清理 closure 后以 `(closure, 0x40)` 调 `1FD0`，其经 `1FE0` 跳到 `B75029`。这只闭合固定样本 heap-fallback closure 的同尺寸 QQ wrapper 分配/释放配对，不扩展到 arena branch 或 wrapper 内部 allocator 语义。
- 仍未闭合 op=1/op=2 完整调用者与合同、op=3 重复调用安全、manager 输入边界、owner/consumer thread、QQ dispatcher quiescence、历史 AV 或 R3 stable identity。R4 不准入；本轮未操作 QQ 或 SnowLuma。
- 本轮门：`init.ps1 -Mode docs` PASS（9 live docs、168 links、33 historical entries、10 features）；`git diff --check` PASS。GitNexus 1.6.12 对四份文档做增量重索引，status 为 up-to-date、runner/content current、incompleteReasons 为空；图仍有 callable-flow cap 与 flow truncation 警告，不能以缺失关系证明无调用。`detect-changes --scope staged` 标为 CRITICAL/449 flows；暂存区仅四份 Markdown，唯一 changed symbol 是旧执行记录 section 标题，影响路径与实际源码 diff 无关，复核为文档图扩散，不据此声称源代码调用链改变。

## 2026-10-13 N4-R2 callback-manager consumer callers follow-up（静态）

- 固定 hash 上确认共享 16 字节 wrapper assignment helper 的 op=3 destination cleanup、op=1 source copy 和调用方 source disposal；另确认 `0x3BC545C` 从 request object `+0xA0` 复制 callback-field 候选后传入下游。详见 [寿命合同 §7.4](docs/research/qq-native-lifetime-contract.md#74-callback-manager-consumer-callers-follow-up2026-10-13静态) 与 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。候选对象具体 manager table 未解析，op=2 完整调用者/合同仍 open；有限 direct-call scan 不构成完整 xref。
- GitNexus 已改在专用 Docker 容器中运行：CLI 1.6.12 / Node 22.23.2 Linux x64。提交前的工作树索引在 HEAD `4da7167` 上 status up-to-date、runner/content current、无 incomplete reasons；文档提交后再次刷新，最终索引也匹配提交后的 HEAD，`status --json` up-to-date、final `detect-changes --scope all` 为 No changes detected。`drain_responses` 仓库调用者定位为 4 个测试 + 1 个生产侧 adapter 调用，已与 `rg` 核对；图本身有流程截断，不对缺失边作否定推断。
- 仅更新研究/状态文档，未运行 Cargo 测试、未调用或读取 QQ，没有更改 SnowLuma 连接。R2 原生 manager / thread / quiescence 和历史 AV 仍未闭合；R4 不准入、feat-005/006 保持 blocked、产品 NOT READY。

## 2026-10-14 N4-R2 dispatcher owner T5 只读复核

- 第五次 `observe-msf` 绑定既有测试进程代次和 profile hash；145 个线程、41 个 TLS dispatcher，全局 dispatcher 指针仍命中一条 TLS 项。五次 `+0x58` 原始 DWORD 位型相同且均不匹配目标线程；T4 对 8,874 个可访问系统线程枚举也无匹配。
- 固定 profile 静态核验：`0x7879D0` IAT → `GetCurrentThreadId`；`.pdata` 起点解码覆盖 525,291 个函数范围 / 18,889,360 条指令；后续 RTTI 将旧日志的 `worker+0x48` 收敛为 `ThreadTaskManagerImpl+0x48` pointer-backed 16-byte pair collection，`0x217D8CC` 遍历并设置当前 TID；`0xB7CE8A` 通过 singleton shared pair 提交 outbound task。
- T5 只读扫描 2,274 个可读提交 region、每轮约 901 MB，出现 3 次局部读取不足。纠正内联数组假设后，未找到符合固定 worker-context 双 vptr、vector 项、同一 global object/control block 的精确成员候选；归属与生命周期继续 unknown。原始证据在 `local-evidence/n4-r2-field-20261014-000619/`，脱敏摘要 `summary-t5-redacted.json`；详情见 [thread contract §12](docs/research/qq-native-thread-contract.md#12-2026-10-14-r2-global-dispatcher-owner-只读复核) 与 [执行记录 T5](docs/execution/2026-10-11-n4-response-recovery.md)。
- 没有注入、目标进程写入、QQ 函数调用、callback invocation、发送、打开聊天或操作 SnowLuma。R2 owner/consumer/manager/quiescence 与历史 AV、R3 stable identity 均未闭合；R4 不准入，feat-005/006 blocked，产品 NOT READY。未运行 Cargo 测试。
- 新增固定-hash RTTI/vtable 证据：`.rdata` RVA `0x4260448` 的 `ThreadTaskManagerImpl` vtable 含 `0x217DD98` 析构项；析构链最终遍历 shared-pair vector 并释放缓冲。仅确认 manager 类存在清理实现；owner、析构触发、线程退出顺序与 callback drain 仍未知。
- `.pdata` xref 映出 `0x217D242` 构造函数由 factory `0x217D16C` 调用；factory 创建 0xF0 字节 manager 并包入 shared holder。静态调用链 `0xB81758 → 0xD31686 → 0x217D16C` 中，`0xD31686` 将 holder 安装至外层对象 `+0x110` 后调 `0x217D58C`；外层对象销毁 owner/退出触发/drain 仍未知。
- 后续反汇编确认 `0x217D58C` 填入 `ThreadTaskManagerImpl+0x48` task vector、`0x217D8CC` 遍历并设置各项 TID、析构路径释放该 vector；此前使用的 `worker+offset` 是暂定变量名。manager 内部 task vector 合同已静态闭合，global dispatcher 是否为其成员、外层析构触发与 QQ callback drain 仍 unknown。
- 精简静态分析摘要写入忽略的本机证据 `local-evidence/n4-r2-field-20261014-000619/summary-r2-manager-static-redacted.json`；wrapper 哈希固定，真实会话/QQ 二进制仍不入库。
- 本轮文档门通过：`init.ps1 -Mode docs` 为 9 live docs / 174 local links / 33 historical entries / 10 features；`git diff --check` exit 0（仅有仓库既有 LF→CRLF 提示）。提交前 Docker GitNexus 1.6.12 / Node 22.23.2 对 272 个文件强制重建为 18,386 nodes / 49,242 edges / 148 clusters / 459 flows；`detect-changes --scope staged` 为 5 files / 31 symbols / 2 observer flows、medium risk。提交后 Docker 索引匹配 HEAD 且 `status --json` up-to-date、runner/content current、无 incomplete reasons；final `detect-changes --scope all` 为 No changes detected。索引仍报告 33 个 callable-flow 候选超 cap、336/536 entrypoint candidates 未入 flow、902 callees 被跳过、14 次 walk 达预算及局部 REACHING_DEF/CDG 缺口；缺失关系不作为无调用证明。未运行 Cargo tests。
- 在 T5 文档追补前通过 Docker Linux descriptor helper 重读 schema-2 计划：plan digest 仍为 `sha256:7ffaf98b4b4a25f5083fa91feab3655a5dab84272dfc15a3529c467ade1dee3f`、39 个 cited paths；当时 HEAD 为 `1b7f6147a7ee213ec070f7b795502a2e9eddab69`。所有引用路径均重新哈希并与计划 manifest 比对；已落地的 R1–R6 离线实现差异已核对，新增 `.agents/` harness 识别为用户未跟踪工作并保留。该记录为该时点快照，后续 T5 文档改动列在本节。
- 续作时再次用 Docker Linux descriptor helper 读取同一计划的精确字节并重算 schema-2 snapshot：digest `sha256:7ffaf98b4b4a25f5083fa91feab3655a5dab84272dfc15a3529c467ade1dee3f` 不变；从计划基线 HEAD `0f5b483` 到当时 HEAD `7a998db`，39 个 cited paths 中 36 个的对象层/状态/digest 不同，差异与此前落地的计划切片及 harness 文档入库一致；当时 global dirty digest 为 `cd8fda719e873b6bd580aa8bf89130c6c7afee28e816dd0e6955951b7e6262e2`，用户 `.agents/` 仍为唯一未跟踪内容并保留。该快照早于本节本次交接文字修订。

## 2026-10-14 N4-R2 op=4 与 op=2 caller 形状扩展（离线静态）

- 固定 `wrapper.node` hash `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 的两张 callback manager 跳表，除 op=0–3 外，`table+0x10` op=4 目标也已闭合：inline `0x73BCB0` 与 heap `0x73C309` 都将第五参数输出 qword 清零。`0x3FCE5E` 调 manager op=4 后返回 out!=0；因此旧“presence helper”标签过强，现仅称 op=4 输出查询 helper，不推断 callback presence。
- 扩展精确 `mov edx,2` 的 `.pdata` 静态筛选，要求 RCX 与间接目标 slot 地址归一相等、R9D=`0x10`，且短近邻内不消费返回值；识别 9 个 ABI-shaped 调用点，分布于 7 个函数。`0x35140D5` 与 `0x9E13A7` 还先对同一 manager slot 执行 op=4 查询 helper。具体地址、限制和纠正记录见 [寿命合同 §7.6](docs/research/qq-native-lifetime-contract.md#76-callback-manager-op4-与-op2-abi-调用点-补充-2026-10-14-静态) 与 [N4 执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。筛选未解析各对象的动态 manager pointer，不能当作完整 xref 或全调用者枚举。
- 本轮仅静态读取本机固定样本；未启动、读取或操作 QQ，没有 native 函数调用、callback invocation、消息发送或 SnowLuma 操作。op=1/op=2 完整 ABI、动态 table/field owner、consumer/owner thread、QQ dispatcher stop/cancel/drain/quiescence、旧 AV 根因与 R3 stable identity 仍开放；R4 不准入、feat-005/006 blocked、产品 NOT READY。验证：`init.ps1 -Mode docs` PASS（9 live docs / 178 local links / 33 historical entries / 10 features）；`git diff --check` exit 0（仅既有 LF→CRLF 提示）；Docker GitNexus `detect-changes --scope staged` 为 4 files / 14 section symbols / 0 affected processes / low risk。静态任务未运行 Cargo 测试。

## 2026-10-14 N4-R2 HttpConnection callback field owner（静态）

- 固定 hash `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 的 ctor/dtor vptr 都指向 RVA `0x458D3B8`，有效 COL RVA `0x458D4C0` 解析为 `.?AVHttpConnection@httpx@@`；vtable slot 0 destructor thunk `0x3BC550C` 调 `0x3BC3082`。`0x3BC545C` 的 owner 因此从 request-object 候选改为 `httpx::HttpConnection`。
- `0x3BC2C3A` 初始化 `this+0xB0/+0xB8` 的 empty manager/invoker sentinel，并以 manager op=3、`R9=0x10` 处理 `this+0xA0`。对应析构主体对 `this+0xA0` 调 `0x2632` dispose。异步状态分发 `0x3BC456A` 进入状态 6/8 的 `0x3BC4C18/0x3BC4D84`；二者经 `0x3BC545C` 对同一字段执行 op=1、16-byte copy 后传递给下游 request/callback helper。
- 这确认的是具体 C++ owner 和 manager-managed callback wrapper 字段；并不说明当前实例含非空 callback。尚未定位 empty→non-empty 的赋值 writer，也未把实例动态 manager slot 绑定到 `0x73BC5C/0x73C2AA`。R2 caller/ABI、arena 生命周期、owner/consumer thread、outer teardown、stop/cancel/drain/quiescence、历史 AV 和 R3 identity 均开放；R4 仍不准入。
- 可复现脚本与原始文本输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`；脚本先验证样本 SHA-256。没有访问 QQ 进程、调用 native 函数、发送消息或操作 SnowLuma；未运行 Cargo 测试。
- 本次适用门：`init.ps1 -Mode docs` PASS（9 live docs / 181 local links / 33 historical entries / 10 features）；`git diff --check` PASS；Docker GitNexus `detect-changes --scope staged` 为 4 files / 11 section symbols / 0 affected processes / low risk。未运行 Cargo 测试。
