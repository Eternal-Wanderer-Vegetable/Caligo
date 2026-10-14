# Caligo 当前进度与验证

## Current State

**Last Updated:** 2026-10-14（Asia/Shanghai）
**Baseline commit:** `0f5b483`；本轮静态追查起点 `3444baa`，保留既有未跟踪 harness 文件。
**Active Feature:** N4-R2。用户已授权使用指定测试 QQ 实例；同一固定 profile 已完成五次外部只读 `observe-msf`。五次全局 dispatcher 对象都命中一条线程 TLS 项，`+0x58` DWORD 位型归一后相同且不匹配目标线程；T4 可访问系统线程枚举亦无匹配，静态 IAT 证实该字段读取 `GetCurrentThreadId`。修正 collection 的 pointer-backed vector 布局后，T5 全可读映射扫描未找到匹配已知 worker-context 双 vptr 与同一 shared pair 的成员候选，故 membership 仍 unknown。固定 hash 静态追查已定位 `HttpConnection+0xA0` 的构造期 empty→non-empty writer 与 `WeakWrapper<HttpConnection>` capture；本次进一步闭合被接受 `HttpServer` 路径：`HttpConnection+0x38` 为 `TCPClientSocket@xpng`，其 `+0x8` 是 `TCPSocketLibuv@xpng`；状态 6/8 的虚槽 `+0x10` 经过 `0xCAD3B8` 到内层 slot 8 `0x31F729E`。替代 callback wrapper 的 captured owner 已由有效 RTTI 收敛为 `WeakWrapper<HttpServer@httpx>`；factory 的 `R8D` 零/非零路由已拆开：零值进入 `0x3458B00`；非零值在 `HttpServer+0x20` tree-shaped header 中按 node `+0x20` DWORD 做下界搜索并验证精确键命中。新追查将该局部 key 来源收敛到 `HttpServer+0x5C` 自增值：注册侧把它插入 `+0x20` 并放入 `WeakWrapper<HttpServer>` callback capture，invoker 再用同一 key 回到 factory 删除匹配节点。该证据仅闭合通用 `HttpServer@httpx` 内部 key 回路；payload 类型/调用、key 与协议身份的关系仍 unknown，不等同 QQ response consumer。命中 node pointer 进入 `0x5D763A→0x1086E→0x108CC` RB-tree erase 路径并清理 node `+0x28` pair。全局 manager ABI、owner/consumer thread、退出/迁移与 stop/cancel/drain/quiescence、旧 AV 根因仍未闭合。新增 R2 静态增量见寿命合同 §7.14–§7.16：NTEngine parent 字段释放已定位；child dtor 路径也显示 run_loop_thread.cc stop 虚调用及对 child+0x10 waitable HANDLE 的 INFINITE wait/close，但 wait 返回值被忽略。child last-owner、manager 全任务停止及 QQ callback queue drain/quiescence 仍未证。§7.17 已将 manager vector 的 `TaskQueue` 项经 slot 1 投影出的 `TaskQueue+0x38` object RTTI 收敛到 `TaskRunner@TaskQueue@internal@xpng`；其 `+0x8/+0x10` pair pointee 为 `ThreadTaskManagerImpl`，slot0/slot2 经该 manager 方法比较 manager-owned `+0x58` 与当前 TID，slot3 也检查同一字段。此类型与字段归属证据不证明 task payload、全项 stop 或 drain/quiescence。§7.16 已将固定样本 `D31686→D33418→CreateThread` 的成功 HANDLE 写入点绑定到 `child+0x10`；wait 返回仍未检查。R3 stable native identity 仍 unsupported；feat-005/006 仍 blocked，R4 发信门未满足。
2026-10-14 §7.28 又定位 `MSFThread_xpng` slot 2 经 `D31686` 将 manager holder 安装到 inner payload `+0x110` 的静态路径；三个构造调用点均接到 slot 2，wrapper deleting destructor 进入 `D31356`。外部构造目标 `0x180D30FF0` 模块与语义未知，manager last owner、consumer/thread ABI 和 quiescence 仍未闭合，R4 不准入。

2026-10-14 §7.36 将固定-hash `MSFService@nt` vtable slot 3/4 分别连接到直接调用 `0x732A80` 的两个静态入口；slot 4 从 COL `+0x10` secondary subobject 调整 `this-0x10` 后提交。此项定位 service submission surface，不等于识别 `0x73BD6C` 实际 callback target/线程、owner teardown 或 quiescence；R2/R3 未闭合，R4 不准入。

**Scope:** 在 [N4 响应恢复计划](docs/plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md) 范围内，既往已对指定实例完成五次固定 hash 的外部只读校准及受限只读内存取证；2026-10-14 本次增量只读固定磁盘样本并更新静态研究文档，没有访问 QQ/SnowLuma。全程零注入、零目标写入、零 QQ 函数调用、零消息发送、零聊天打开。R1–R3 完成门未满足，不执行 R4 native send。

**最新静态更正：** 寿命合同 §7.18 / thread contract §13 重新分清 `ThreadTaskManagerImpl` 内部字段：`+0x30/+0x38` 是单独释放的 pair，`+0x40` 是保存新分配 16-byte pair holder 的 pointer，`+0x48` 是 task vector。slot2 TLS 安装来源是 `[manager+0x30]` pointee 的 `+0x38/+0x40` pair；`TaskRunner` TID mismatch 路径则直接把 manager 的 `+0x38/+0x40` 交给 pair-copy helper，属于跨越上述字段的 pair-shaped projection，不能称为已识别的 manager pair，也不能据此识别 nested `D32138` 目标。原有相反简写已由日期化证据更正；§7.19 又确认 manager `+0x30/+0x38` 由 `0x217D58C` 按 selector 条件调用 pair assignment helper `0x106E6` 写入，source pair 的类别以及 slot2/slot3 空值可达性仍待核验；运行时语义、TLS 目标 RTTI、历史 AV 根因仍未知，产品与 R4 门状态不变。

**最新静态配置覆盖：** 冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）的 30 个可识别直接 rel32 `E8` 调用 `0xD31686` 均已逐点分类：28 个经 `0x4E6E5A` 得 `{0,0,0,0,1,0}`，`0x01B63FF7` 经 `0xD324EC` 得相同模板，`0x009C0EE1` 经 `0xD32508/0xD32512/0xD3251C` 得 `{0,0,0,0,3,1}`；两种模板的 selector writer 在各静态 setup 代码路径可达。该结果不证明现场执行、间接调用或 T1–T5 instance mapping；剩余门见[寿命合同 §7.21](docs/research/qq-native-lifetime-contract.md#721-全部-30-个-d31686-直接调用点的-manager-配置来源2026-10-14固定样本静态)。

**最新 R2 ownership 增量：** 固定-hash 证据现已确认 `ThreadTaskManagerImpl+0x10`、`TaskQueue+0x48` 和 `TaskRunner+0x8/+0x10` 均为弱边；manager vector strong 持有 `TaskQueue`、TaskQueue strong 持有 `TaskRunner`，这条三边路径没有强引用环。NTEngine `+0xE0` 持有 child pair，`+0xE8` 另持 index-1 `TaskRunner` pair；`0x9C1928` 是带 TLS guard 的静态访问器，在冻结样本中有 796 个已识别直接调用者。候选析构体会调用 runner slot 0 后释放 parent 字段，但 dtor finalizer 注册/触发、child/manager last owner、所有任务 stop 与 callback quiescence 仍 unknown。脚本/输出 hash 与边界见[寿命合同 §7.22–§7.24](docs/research/qq-native-lifetime-contract.md#724-nteengine-静态访问器调用面与-finalizer-未知2026-10-14固定样本静态)及[thread contract §17–§18](docs/research/qq-native-thread-contract.md#18-2026-10-14-taskrunner-manager-弱引用与候选强环复核固定样本静态)。

**GitNexus 文档 gate：** 首次 no-overlap 来自未包含 Markdown Section ranges 的旧图；强制重建后 5 份 Markdown 文件已索引 Section（42/35/25/29/6 个），新 detect 映射 6 files / 64 Section symbols / 0 affected processes / low risk。`feature_list.json` 仍只有 File node，故已另行审阅其三处行级变更且 JSON 解析通过；此结果只界定本次文档影响，不声称项目全局零风险。当前完整记录见本次 execution/handoff。

**最新固定-hash finalizer 限界：** PE TLS callback `0x3E68820` 是 thread-attach 路径且固定 callback 区间为空；`0x3E67F70` 在 process/thread detach 遍历当前 TLS `+0xB0` callback list，注册者未找到，因此仍可能是间接 finalizer 通道。析构 body `0x9C15D6` 唯一对齐 RVA 命中经 `.pdata`/UNWIND_INFO 归类为自身 EH unwind map；没有 `atexit`/`_onexit`/`__cxa_atexit` 命名导入。NTEngine dtor 是否加入 TLS list 或由其他内部 finalizer 触发仍 unknown；R2/R3 gate 开放、R4 不准入，产品仍 NOT READY。

**本轮验证：** `init.ps1 -Mode docs` PASS（9 live / 202 local links / 33 historical / 10 features），`git diff --check` exit 0（仅 LF→CRLF 警告）。Docker GitNexus 1.6.12 / Node 22.23.2 PDG rebuild 后 status up-to-date、runner/content current（272 files）、无 incomplete reasons；最新 detect 为 4 files / 14 Section symbols / 0 affected processes / low risk，均是文档变更。索引日志仍有 callable cap、REACHING_DEF 丢 311/4311、一个 CDG skip 与 flow truncation；0 processes 不代表代码路径全局无影响。未运行 Cargo tests；本轮未访问 QQ/SnowLuma。

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
- R2 已基于冻结磁盘样本 hash（`63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）静态确认 `1DB2(pair*)` release、`732A80` body 同步深复制与 `B75029` 输入释放；代码按代复用单个 service pair、body 调用后对称释放、去掉未使用的 self-string 副本。常驻和一次性 callback context 在发布前 PIN bridge image；resident 停止时本地关闭 callback 准入、等待已准入调用退出，隔离后迟到 callback 只计数、不入槽。2026-10-13 Ghidra follow-up 已映射 `73BC5C/73C2AA` op 0–3、确认 heap-fallback closure 同尺寸分配/释放，并补强 `0x54CE00` assignment 路径中的 op=1 copy / op=3 dispose；`0x3BC545C` 所读字段 owner 是 `httpx::HttpConnection+0xA0`。固定 hash 下的构造链确认从 `this+0x158/+0x160` 创建并安装 `WeakWrapper<HttpConnection>` capture；inline/heap 局部 manager/invoker 不等同于两张全局 manager 表。2026-10-14 进一步闭合被接受 `HttpServer` 路径的 `HttpConnection+0x38` 类型为 `TCPClientSocket@xpng`，状态 6/8 的 `+0x10` 实际到 `0xCAD3B8`，然后转交给内层对象 `+0x40` 虚槽（其 RTTI/合同未知）；此为通用 HTTP socket 链，不能认作 QQ 响应消费者。2026-10-14 全局 manager 表 op=4 输出清零分支与 9 个 op=2 manager-ABI-shaped 调用点（7 个函数）亦已记录；动态函数指针仍未逐一绑定至 `73BC5C/73C2AA`。完整调用者、动态 table 身份、arena 分支寿命、owner/回调线程、QQ stop/cancel/drain/quiescence 和历史 AV 原因仍未证。`0x3FCE5E` 仅称 op=4 输出查询 helper，不声称 callback presence。静态与离线证据不证明 QQ consumer 停止、原生队列排空或最终业务回执持久化；产品仍 blocked。
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

## 2026-10-14 N4-R2 HttpServer 注册 key 静态追查

- 固定 hash `wrapper.node` 的 `0x3458736` 成功路径将 `HttpServer+0x5C` 自增 DWORD 经 `0x34589B8` 插入 `HttpServer+0x20`，payload 暂不能命名；同一 key 随 `WeakWrapper<HttpServer>` capture 位于 `+0x10`，两个 invoker 进入 `0x345927C` 后将 key 传回 factory。非零 gate 下 factory 在 `+0x20` 精确匹配并删除该节点；零 gate 使用独立 `+0x60` 连接表。局部 key 创建→capture→erase 回路见 [寿命合同 §7.13](docs/research/qq-native-lifetime-contract.md) 与 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。这属于 generic `HttpServer@httpx` 静态路径，不证明 payload 实际调用、协议身份、QQ callback consumer 或业务回执。
- 本轮只读冻结磁盘样本，不访问 QQ/SnowLuma、不调用 native、不发送消息；R2 全局 manager ABI、owner/consumer thread、outer teardown、QQ stop/cancel/drain/quiescence、历史 AV 与 R3 stable identity 仍开放。验证：`init.ps1 -Mode docs` PASS（9 live docs / 189 local links / 33 historical entries / 10 features）；`feature_list.json` 解析 PASS；`git diff --check` PASS（只有既有 LF→CRLF 提示）。Docker GitNexus 1.6.12 / Node 22.23.2 刷新 `/repo` 后为 18,424 nodes / 294,192 edges / 148 clusters / 459 flows；HEAD `ef7c6acf9b7affc7c8480f70741165672f3b683c`，status up-to-date、runner/content current、272 files、无 incomplete reasons。`detect-changes --scope all` 为 5 files / 8 section symbols / 0 affected processes / low risk；分析仍有 callable-flow、REACHING_DEF、CDG 与 process truncation 警告，缺边不作无路径证明。Rust diff 为空，未运行 Cargo tests。R4 不准入、feat-005/006 继续 blocked、产品 NOT READY。

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
- 该 owner-only 增量确认具体 C++ owner 和 manager-managed callback wrapper 字段；当时尚未定位 empty→non-empty writer。后续 writer 与该字段局部 manager/invoker 分支见本记录的 [HttpConnection capture writer](#2026-10-14-n4-r2-httpconnection-capture-writer-静态) 和 [寿命合同 §7.9](docs/research/qq-native-lifetime-contract.md#79-httpconnectiona0-callback-capture-安装路径-2026-10-14-静态)。这些结果仍不证明任一现场实例已构造或 callback 已执行。R2 全局 manager ABI、arena 生命周期、owner/consumer thread、outer teardown、stop/cancel/drain/quiescence、历史 AV 和 R3 identity 均开放；R4 仍不准入。
- 可复现脚本与原始文本输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`；脚本先验证样本 SHA-256。没有访问 QQ 进程、调用 native 函数、发送消息或操作 SnowLuma；未运行 Cargo 测试。
- 本次适用门：`init.ps1 -Mode docs` PASS（9 live docs / 181 local links / 33 historical entries / 10 features）；`git diff --check` PASS；Docker GitNexus `detect-changes --scope staged` 为 4 files / 11 section symbols / 0 affected processes / low risk。未运行 Cargo 测试。

## 2026-10-14 N4-R2 HttpConnection capture writer（静态）

- 固定 hash 样本的 `0x3BC2C3A` 构造路径先用 op=3 初始化 `this+0xA0`，随后通过 `0x3BC3008 → 0x3BC66B5 → 0x3BC66EF` 构造 RTTI `__shared_ptr_emplace<WeakWrapper<HttpConnection>>` capture；把 `this` 存在对象 `+0x18` 并生成 `{object+0x18, object}` pair。
- ctor 增加 control-block 强计数，再由 `0x3BC678A` 将 pair 装入 `+0xA0` wrapper。helper 通过 `0x40634` inline storage 路径或 `0x24C0(0x10)` heap fallback，分别安装局部 manager/invoker `0x3BC67F8/0x3BC6436` 或 `0x3BC692A/0x3BC6576`，复制 pair 并清空临时 wrapper。成功构造路径的 empty→non-empty writer 与该字段动态分支现已解析；这些局部 helper 不是 `0x73BC5C/0x73C2AA` 全局 manager 表。
- 状态 6/8 从该字段 clone callback wrapper，再经 `this+0x38` handler 虚表 slot `+0x10` 转交，具体 handler/consumer thread 未解析。脚本/输出在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-httpconnection-write-sites-20261014.py/.txt` 及同目录 `r2-httpconnection-*-20261014.py/.txt`。静态证据不证明某个现场实例已构造、callback 执行或结果交付；没有 QQ/SnowLuma 动作或 Cargo 测试，R4 不准入、feat-005/006 blocked、项目 NOT READY。
- 上游 rel32 caller 筛查找到 `0x3458736→0x3458A18` 与 `0x34592DC→0x3458A18`：第一处在父对象 `+0x18` 为空时把父对象 `+0x38` handler 转交给新连接；第二处位于 `0x345927C`，从 owner pair 经 `0x5CD68C` lock-shaped helper 取得非空对象，再传入 factory。该 returned object 和 alternative handler 的动态类仍 open；详见 [寿命合同 §7.9](docs/research/qq-native-lifetime-contract.md#79-httpconnectiona0-callback-capture-安装路径-2026-10-14-静态)。
- 验证：`init.ps1 -Mode docs` PASS（9 live docs / 184 local links / 33 historical entries / 10 features）；`git diff --check` exit 0（仅现有 LF→CRLF 提示）；Docker GitNexus `detect-changes --scope all` 为 4 files / 13 section symbols / 0 affected processes / low risk。未运行 Cargo 测试。

## 2026-10-14 N4-R2 accepted HttpServer callback handler（静态）

- 固定磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 下，`0x2A0690A → 0x2A08062 → 0xCADD64 → 0xCADD8A` 构造 listener，`0x2A080BD` 把它交给 `HttpServer` ctor `0x3457D10` 并写入 `HttpServer+0x10`。`0x3458614` 经该 listener vtable `+0x28` 接受连接，把 client pair 写入 `HttpServer+0x38`。RTTI/vtable 确认 listener 为 `TCPServerSocket@xpng`，其 `+0x8` acceptor 是 `TCPSocketLibuv@xpng` slot 4 (`0x31F6CF6`)；成功分支构造同类底层 pair，再由 `0xCAE0DC` 包入 `TCPClientSocket@xpng`，pair 经 `0x3458736 → 0x3458A18 → 0x3458B00 → 0x3BC2C3A` 成为 `HttpConnection+0x38`。
- 状态 6/8 将 `HttpConnection+0xA0` clone 交给 `+0x38` 的 vtable `+0x10`；该槽落到 `TCPClientSocket` 的 `0xCAD3B8`，再调用内层 `TCPSocketLibuv@xpng` slot 8 `0x31F729E`。该方法检查对象状态后向 `0x3EB5090` helper 传本地 callback entry addresses；helper 语义、实际 callback thread/结果合同未知。这只解析通用 HTTP accepted-socket path；`0x34592DC` (`0x345927C`) 的替代 factory 其 captured owner 已由 RTTI 确认为 `WeakWrapper<HttpServer@httpx>`，但不证明所有 state 的 handler class；QQ consumer/owner thread、外层 teardown 及 stop/cancel/drain/quiescence 仍未闭合，不能据此认定 QQ send response consumer。详见 [寿命合同 §7.10](docs/research/qq-native-lifetime-contract.md) 与 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- 本机可复现脚本与地址级导出在 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-httpconnection-tcpclient-chain-20261014.py/.txt`、同目录 `r2-tcpserver-inner-acceptor-rtti-20261014.py/.txt`、`r2-httpserver-listener-ctor-xrefs-20261014.py/.txt` 和 `r2-alternative-wrapper-xref-screen-20261014.py/.txt`；脚本先验固定 hash。只读样本，无 QQ/SnowLuma 操作、原生调用、消息发送或 Cargo 测试。
- 初次 direct `E8 rel32`/`.rdata` absolute-qword 屏幕漏掉 RIP-relative wrapper 引用；后续 `.pdata` 边界扫描找到 `0x3459220`/`0x3459426` 的三处引用，COL/RTTI 确认 captured owner 为 `WeakWrapper<HttpServer@httpx>`。进一步按固定 hash 有界解码确认 factory 的 `R8D` gate、`HttpServer+0x20` 树的 `+0x20` DWORD 下界搜索与精确键校验；键来自输入对象 `+0x10` DWORD，精确命中 node 进入 RB-tree erase。证据见 [寿命合同 §7.11–§7.12](docs/research/qq-native-lifetime-contract.md#711-httpserver-替代-callback-wrapper-的-弱-owner-2026-10-14-静态)；容器模板、pair pointee、键的产品意义及 callback/QQ 语义仍 unknown。
- 文档门 `init.ps1 -Mode docs` PASS（9 live docs / 187 local links / 33 historical entries / 10 features），`feature_list.json` JSON PASS，`git diff --check` exit 0（Git 给出既有 LF→CRLF 提示，没有 whitespace error）。Docker GitNexus 1.6.12 / Node 22.23.2 刷新后 status 为 HEAD `ef7c6ac`、runner/content current（272 files）、无 incomplete reasons、up-to-date。最新 `detect-changes --scope all` 说明 5 个文件有差异、但这些 hunks 未覆盖索引 symbol，并标记工作树非 clean；之前旧索引报告的 5 files / 8 symbols / 459 processes / critical 已被此刷新后的结果取代，原始前后输出分别保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-gitnexus-detect-followup-20261014.txt` 和 `r2-gitnexus-detect-final-20261014.txt`。当前 Rust diff 为空。索引保留 callable-flow、REACHING_DEF、CDG、scope-resolution 与 process truncation 告警；缺边不能证明无路径。未运行 Cargo 测试。

## 2026-10-14 N4-R2 NTEngine parent 字段释放边界（静态）

- 对固定 hash `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 确认 `NTEngine@nt` deleting destructor 会释放自身 pair/owner 字段；`0xD31686` 写入的 ThreadTaskManager holder 位于 child 基址 `+0x110`，与 NTEngine 自身同偏移字段分开记录。析构虚槽/helper 语义、child last-owner 与 manager 最终销毁/dispatcher drain 仍未证。slot 1 输出初始化及跨映像 direct target 不构成 teardown/QQNT 归属证据。
- 仅固定磁盘分析，无 QQ/SnowLuma 访问、native 调用、消息发送或 Cargo 测试。新增可复核脚本/输出和 hash 见 [寿命合同 §7.14](docs/research/qq-native-lifetime-contract.md) 与 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。R2/R3 门仍开放，R4 不准入，feat-005/006 blocked，产品 NOT READY。
- 文档验证：`.\init.ps1 -Mode docs` (PASS, 9 live docs / 191 local links / 33 historical entries / 10 features); `feature_list.json` JSON PASS；`git diff --check` PASS（仅 Git 现有 LF→CRLF 规范化提示）。`*.rs` diff 为空，未运行 Cargo 测试。Docker GitNexus 1.6.12 / Node 22.23.2 对 `/repo` 重建成功：18,412 nodes / 49,269 edges / 148 clusters / 459 flows；`status --json` 为 up-to-date、runner/content current、272 files、无 incomplete reasons。`detect-changes --scope all` 报 5 files / 31 symbols / 0 affected processes / low risk；输出所列 changed symbols 为执行记录/研究记录等文档 section；Rust diff 为空，这些命中表示文档与已索引流程名称相关，不代表原生 Rust 路径发生变化。分析仍有 callable-flow cap、REACHING_DEF cap、CDG skip、跨语言解析拒绝及 process truncation 告警；缺图不证明无路径。
- 2026-10-14 child stop/wait 静态补充的最终文档门：`init.ps1 -Mode docs` PASS（9 live docs / 191 local links / 33 historical entries / 10 features）；`feature_list.json` JSON PASS；`git diff --check` PASS（只有既有 LF→CRLF 规范化提示）。Docker GitNexus 1.6.12 / Node 22.23.2 对 `/repo` 强制索引成功，18,414 nodes / 49,271 edges / 148 clusters / 459 flows；`status --json` up-to-date、runner/content current、272 covered files、无 incomplete reasons；`detect-changes --scope all` 为 5 files / 33 symbols / 0 affected processes / low risk，均为文档 section 命中。索引仍报告 33 个 callable-flow 候选超 cap、REACHING_DEF cap 丢 311 条、CDG skip、30 个跨语言 property 解析拒绝、336/536 entry candidate 未排入 flow、902 个 callee skipped 与 14 次 trace budget cut；缺边不证明无路径。`*.rs` diff 为空，未运行 Cargo 测试；无 QQ/SnowLuma 操作或 native invocation。
## 2026-10-14 N4-R2 child thread HANDLE 创建来源（静态）

- 固定样本 `wrapper.node` `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 的 `D31686` 将 `&child+0x10` 传给 `D33418`；该 helper 调 `KERNEL32.dll!CreateThread`，成功时将返回 HANDLE 写回该 out-pointer。`D313D6→D3355C` 对同一槽执行 INFINITE wait 和 close；返回值未检查，线程完成与 manager 全量停止/回调排空均未证。线程 callback `D31958→D31B7E` 会写 `child+0x60` TID 并通过 `child+0x110` manager holder 虚调，但动态任务语义仍 unknown。详见寿命合同 §7.16 与执行记录。
- R1–R3 前置门、R3 stable identity、child last-owner、全局 callback quiescence、历史 AV 和 R4 现场验收仍未闭合；feat-005/006 保持 blocked，产品 NOT READY。无 QQ/SnowLuma 访问/native invocation/消息发送；无 Cargo 测试。
- 2026-10-14 TaskRunner owner follow-up：准确追踪 `manager+0x10 → 0x46448 clone → TaskRunner+0x8/+0x10`，结合 factory 的 `0x1687C→0x168CE` 初始化与 manager primary RTTI，将 inner pair pointee 识别为 `ThreadTaskManagerImpl` 自身。更正 `+0x58` 归属：TaskRunner slot0/slot2 通过 held pair 调 `0x31F88EA` 比较 manager-owned `+0x58` 与当前 TID；slot3 `0x31F8C7E` 也检查同一字段。此前记作 `TaskRunner+0x58` 的表述属 owner 误判，已修正。嵌套 `D32138` 的 object pointer 来自 `ThreadTaskManagerImpl+0x38/+0x40` pair；slot3 `0x217DA96` 又从 `manager+0x30` pointee 投影 `+0x38/+0x40` pair，但这两种 pointee、payload 语义、全项 stop 与 callback drain/quiescence 仍 unknown。Capstone 固定-hash 脚本/输出保存在本机 `_reference` 目录；Rust diff 为空，无 QQ/SnowLuma/native invocation/message/Cargo test。

## 2026-10-14 N4-R2 manager TLS pair field-owner correction（固定样本静态）

- 固定样本 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.6 复核 ctor `0x217D242`、dtor `0x217D4FA`、slot2 `0x217D8CC`、slot3 `0x217DA96` 与 pair helpers：`+0x30/+0x38` 由 ctor 成对清零并由 dtor `1DB2` 释放；`+0x40` 单独清零/替换/释放；task vector 在 `+0x48`。
- `0x217D8CC` 从 `[manager+0x30]` 加载 pointee，`0x63EC4` 复制该 pointee 自身 `+0x38/+0x40` pair；`0x217DA24` 分配 16-byte holder，`D32E5A` 增加强引用后安装到 TLS，且 slot2 把同一 holder pointer 保存在 manager `+0x40`。slot3 `0x217DA96` 从相同 pointee 投影 pair。之前把 TLS source 简写为 manager `+0x30` pair 首 qword，现由 [寿命合同 §7.18](docs/research/qq-native-lifetime-contract.md#718-threadtaskmanager-tls-pair-来源与字段边界更正-2026-10-14-固定样本静态) 和 [thread contract §13](docs/research/qq-native-thread-contract.md#13-2026-10-14-tls-pair-来源与-manager-字段边界更正-固定样本静态) 更正。
- `0x31F88EA` 的 mismatch route 把 manager 本身交给 `0x63EC4`，机械复制 manager `+0x38/+0x40` 并将首 qword交给 `D32138`。此处跨越 manager `+0x30` pair 的 control qword与 `+0x40` holder pointer；不认定为合法/有效 manager pair，不推断 nested RTTI、故障根因或业务语义。R2 owner/thread/drain、R3 identity 与历史 AV 继续开放；不访问 QQ/SnowLuma、不调用 native、不发消息、不运行 Cargo 测试，R4 仍不准入。
- 固定 hash 校验脚本/导出：本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-manager-field-owner-clarification-20261014.py/.txt`，SHA-256 `ED299ABF97A4351292ED7370A941421D8F794B30D59F3E8147CEA17EA2ABE2BA` / `134996C57B11C70B644F97CC5AC827688AA5CC6A22E2D5307304B27D9CC10C27`。

## 2026-10-14 N4-R2 ThreadTaskManager TLS pair 字段更正（固定样本静态）

- 2026-10-14 follow-up 修正前一条 TaskRunner owner 记录的 nested pair 表述。固定-hash ctor/dtor 证明 `ThreadTaskManagerImpl+0x30/+0x38` 是 pair，`+0x40` 是单独 holder pointer，`+0x48` 是 task vector。slot2 从 `[manager+0x30]` pointee 复制其 `+0x38/+0x40` pair，`D32E5A` 克隆并安装到 TLS，同时把 holder pointer 保存到 manager `+0x40`。详见寿命合同 §7.18 与 thread contract §13。
- `0x31F88EA` mismatch 分支直接将 manager 传给 `0x63EC4`，机械读取 manager `+0x38/+0x40`；因其跨过前述 pair/pointer 字段，不将结果称作已识别 manager pair，不给 `D32138` 目标类命名，也不推断 bug 或 AV 根因。slot3 `0x217DA96` 则从 manager `+0x30` pointee 投影其内部 pair，二者保持区分。
- 固定样本校验脚本/导出在本机 `_reference/n4-r2-callback-manager-static-20261013/nte-engine-owner-followup-20261014/`：`r2-nte-manager-field-owner-clarification-20261014.py/.txt`，SHA-256 `ED299ABF97A4351292ED7370A941421D8F794B30D59F3E8147CEA17EA2ABE2BA` / `134996C57B11C70B644F97CC5AC827688AA5CC6A22E2D5307304B27D9CC10C27`。仅固定磁盘静态分析，无 QQ/SnowLuma 访问、native invocation、消息发送或 Cargo 测试。R2/R3 准入门仍未闭合，R4 不准入，feat-005/006 blocked，产品 NOT READY。

- 验证：`.\init.ps1 -Mode docs` PASS（9 live docs / 195 local links / 33 historical entries / 10 features）；`feature_list.json` JSON PASS；`git diff --check` PASS，只有 Git 既有 LF→CRLF 提示。Docker GitNexus 1.6.12 / Node 22.23.2 当前索引为 HEAD `ef7c6acf9b7affc7c8480f70741165672f3b683c`、18,424 nodes / 52,508 edges / 148 clusters / 459 flows；status up-to-date，schema-4 runner identity current、content current（272 files）、无 incomplete reasons。`detect-changes --scope all` 为 6 files / 36 section symbols / 0 affected processes / low risk，均为本轮 docs/feature/state 修改。索引仍有 callable-flow cap、REACHING_DEF drop、CDG skip、跨语言 scope 与 processes truncation 告警；不以缺图证明无路径。当前 tracked Rust diff 为空；本轮无 QQ/SnowLuma 访问、native invocation、消息发送或 Cargo 测试。

## 2026-10-14 N4-R2 ThreadTaskManager `+0x30` 条件 writer（固定样本静态）

- 固定 `wrapper.node` SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。`0x217D58C` 在 `r15b < byte[this+0x2C]` 循环中，先向 `+0x48` vector 放入 task pair；匹配 `r15b == byte[this+0x2D]` 时调用 `0x106E6` 将 `0x217D6D8` 生成的 source pair 条件写到 `this+0x30/+0x38`。`0x106E6` 加强 source control 引用、复制两 qword 并释放旧 destination pair。此前 §7.18 的 writer unknown 已由寿命合同 §7.19 / thread contract §14 更新。
- 若 loop 不运行或 selector 未匹配，本方法不写 pair；slot2/slot3 后续投影的 `0x63EC4` 没有空 pointee guard。寿命合同 §7.20 已从两个固定构造点复原 `{bound=3,selector=1}` 和 `{bound=1,selector=0}`，两者都会命中 writer；shared control-block RTTI 指向 `TaskQueue`。这只覆盖 30 个 `0xD31686` 直接调用点中的两条，未映射到 T1–T5 的动态 manager，也未证明 slot2/slot3 全局调用顺序或 teardown 安全，不据此归因历史 AV。manager owner/thread/quiescence、R3 identity 继续开放；R4 不准入，feat-005/006 blocked，产品 NOT READY。
- 固定-hash Capstone 复核 `r2-manager-writer-source-context-20261014.py/.txt` SHA-256 `0F47D5E0E51DBA9432C4AF2AA4CF37EBFB25F48B3929112D84BDB36C9F989BC7` / `C4D82FE82F24900CE03EC003E024786694240FBF41C2D10A18C723614346877C`。本轮未访问 QQ/SnowLuma、未调用 native、未发送消息、未运行 Cargo 测试。
- 本轮最终文档门：`init.ps1 -Mode docs` PASS（9 live docs / 195 local links / 33 historical entries / 10 features）；`feature_list.json` JSON PASS（10 features）；`git diff --check` exit 0（仅既有 LF→CRLF 警告）。本轮没有 Rust diff，未运行 Cargo。
- 计划 receipt 保持 schema 2、39 cited paths、digest `sha256:7ffaf98b4b4a25f5083fa91feab3655a5dab84272dfc15a3529c467ade1dee3f`；HEAD `ef7c6acf9b7affc7c8480f70741165672f3b683c`。本次已引用路径复核只涉及文档/状态，没有改动计划范围、KTD 或实现决策。
- Docker GitNexus 1.6.12 / Node 22.23.2 刷新后 status `up-to-date`，runner identity schema 4 current、content current（272 files）、`incompleteReasons=[]`。最终 `detect-changes --scope all` 为低风险、0 个 affected processes；changed symbols 均为文档 Section nodes，Rust diff 为空。完整 index/status/detect 输出留在 ignored `target-harness/n4-r2-manager-writer-index.log`、`target-harness/n4-r2-manager-writer-status.log`、`target-harness/n4-r2-manager-writer-detect.log`。分析仍有 callable-flow cap、REACHING_DEF 丢边、CDG skip、跨语言解析拒绝与流程截断，缺图不证明无路径。
- 仅固定磁盘静态分析；无 QQ/SnowLuma 操作、native invocation 或消息发送，R1–R3 未闭合，R4 不准入；feat-005/006 保持 blocked，产品 NOT READY。

## 2026-10-14 N4-R2 manager 配置与 TaskQueue pair 来源（固定样本静态）

- 新的固定-hash 有界反汇编沿 `0x9C0C68 → 0xD31686 → 0x217D16C → 0x217D242 → 0x217D58C` 追出 6-byte config：`{0,0,0,0,3,1}`，对应 `+0x2C=3`、`+0x2D=1`；另一直接 caller `0xB81758 → 0xD31686` 传 `{0,0,0,0,1,0}`，对应 bound 1 / selector 0。两条路径都会在 `0x217D58C` 命中 `+0x30` pair writer。生成 pair 的 control-block RTTI 为 `std::__shared_ptr_emplace<TaskQueue,...>`。
- `0xD31686` 在 manager init 前先安装 holder 到外层 `+0x110`，init 返回后才走 `D33418` 线程创建调用。该顺序只证实这条静态 setup；30 个直接调用点中只有两条输入已复原，也未将任一 manager 与 T1–T5 的动态 dispatcher 对应。slot2/slot3 全局调用次序、manager last-owner/teardown、callback drain/quiescence、历史 AV 和 R3 stable identity 仍 open；R4 不准入，feat-005/006 blocked，产品 NOT READY。
- 固定-hash 脚本与输出：本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\r2-manager-config-producer-20261014.py/.txt` SHA-256 `FF9E4C0D8EEACCB05A6E5079E00592495DE53426BC8E5A5A782D519FEFD04FBF` / `0056F41CCFC68695173E6C051E1B08CB7EB80EF7AC7B993DC0FBE2649B66DBB1`；RTTI/caller 脚本及输出 `r2-manager-init-dispatch-rtti-20261014.py/.txt` `AD02A313D5336241E0012DCAE643E0CF6C738A0CDAA7D422C16405167B8F908A` / `DC964B06DDF2237C9CD6207ABC3DA95D021B56923DCD78A2A3301FCEA78F1930`。报告均先验证冻结样本 SHA-256。
- 本次仅固定磁盘静态分析，无 QQ/SnowLuma 操作、native invocation、消息发送或 Cargo 测试。最终 docs/JSON/diff/GitNexus 验证结果见本节收尾。

## 2026-10-14 N4-R2 全部 `D31686` 直接 caller 配置覆盖（固定样本静态）

- 对冻结 `wrapper.node` 的 30 个可识别直接 E8 调用点逐个核对配置来源：28 个通过 `0x4E6E5A` 为 `{0,0,0,0,1,0}`，`0x01B63FF7` 直接经 `0xD324EC` 产生同一模板，`0x009C0EE1` 形成 `{0,0,0,0,3,1}`。两类配置均令 `0x217D58C` selector writer 静态可达；本结果替代此前“只复原两条输入”的覆盖状态，不证明 runtime setup 被调用、任一现场 dispatcher 身份或间接调用覆盖。
- 逐点导出与 helper bounded decode 及其校验 hash 见寿命合同 §7.21。R2 owner/consumer/lifetime、slot2/slot3 全局顺序、teardown/quiescence、R3 identity、历史 AV 与 R4 现场回程仍未闭合；feat-005/006 blocked、产品 NOT READY。仅固定磁盘静态分析，无 QQ/SnowLuma 操作、native invocation、消息发送或 Cargo 测试。
### 2026-10-14 本轮验证与 GitNexus 影响门

- `init.ps1 -Mode docs` PASS：9 live docs / 196 local links / 33 historical entries / 10 features；`feature_list.json` JSON PASS（feat-005 仍 blocked）；`git diff --check` PASS，仅有既存 LF→CRLF 提示。没有 Rust source diff，未运行 Cargo tests。
- 提交前旧索引查询曾报告 hunk no-overlap，且另一索引状态出现 46 Section/401 process/critical；二者均在本次强制重建后 superseded。fresh staged detect 为 6 files / 64 Markdown Section symbols / 0 affected processes / low risk；JSON hunk 另行人工审阅并通过解析。提交门与 post-commit clean index 结果见下方“弱引用闭合与提交后验证”。图仍告警 callable-flow candidate cap、REACHING_DEF drop、CDG skip、跨语言拒链与流程截断，缺图不证明无路径。
- 本轮仅固定磁盘静态分析；未操作 QQ/SnowLuma、未调用 native、未发送消息、未运行 Cargo tests；保留 `.agents/` 用户文件。

## 2026-10-14 N4-R2 弱引用闭合与提交后验证

- 固定-hash 静态证据新增到寿命合同 §7.22/thread contract §17：`ThreadTaskManagerImpl+0x10` 是 weak self-reference，`TaskQueue+0x48` 是 weak manager backreference；manager 保存 TaskQueue strong pair，但不能由此确定 manager last owner 或析构时序。可排除的是该特定强引用回边，R2 callback consumer、manager last-owner、child teardown、stop/drain/quiescence 与 R3 identity 仍未闭合；feat-005/006 blocked，产品 NOT READY。
- 文档检查：`init.ps1 -Mode docs` PASS（9 live docs / 200 local links / 33 historical entries / 10 features）；`feature_list.json` JSON PASS；`git diff --check` PASS（Git 仅提示 LF→CRLF）。GitNexus 强制重建后，Markdown Sections 数量为 execution/lifetime/thread/progress/handoff = 42/35/25/29/6；staged detect 映射 6 files / 64 symbols / 0 affected processes / low risk。JSON 没有 line-bearing Section node；已手动审阅只涉及 `updated`、feat-005 evidence 和 evidence_paths，状态仍 blocked。
- 六个 tracked 状态/证据文件已提交为本次文档快照；未跟踪 `.agents/` harness 保留。提交后 Docker GitNexus 1.6.12 / Node 22.23.2 的 schema-4 status 与当前 HEAD 匹配，272 covered files、runner/content current、无 incomplete reasons；`detect-changes --scope all` 返回 `No changes detected`。忽略日志位于 `target-harness/n4-r2-weakref-{index,status,detect,detect-staged,postcommit-index,postcommit-status,postcommit-detect,plan-snapshot}.log`；未运行 Cargo、未操作 QQ/SnowLuma、未调用 native、未发送消息。
- N4 计划 receipt 原文未改：canonical path `docs/plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md`，75,673 bytes，SHA-256 `7ffaf98b4b4a25f5083fa91feab3655a5dab84272dfc15a3529c467ade1dee3f`，39 cited paths。GitNexus 分析仍有 callable-flow、REACHING_DEF、CDG、跨语言字段拒链及 process trace 截断；不以 missing graph edges 证明路径不存在。

## 2026-10-14 N4-R2 TLS detach callback producer follow-up

- 固定样本静态追踪补足 TLS detach 链：`0x3E67F70` 从 `TLS+0xB0` 链表逆序调用 callback，调用槽 `0x4909B18` 指向 `0x3EB1DE0 (jmp rax)`；`0x3E94F90` 跳转至调用 `HeapFree` 的释放函数。
- TLS writer hunt 扩展至 1499 个 `.pdata` index-reference 候选、37 条首层参数边、46 条 helper 转发边和 50 个函数/参数状态，仍未在此覆盖内发现 producer。NTEngine destructor registration/执行仍未知；R2/R3 blockers 与 R4 hard gate 保持。
- 新证据与扫描边界见[寿命合同 §7.26](docs/research/qq-native-lifetime-contract.md#726-tls-detach-callback-dispatch-与-producer-有界追踪-2026-10-14固定样本静态)和[N4 execution log](docs/execution/2026-10-11-n4-response-recovery.md)。本轮没有 QQ/SnowLuma/native/message/Cargo 行为；user-owned `.agents/` harness 保留。
- N4 schema-2 计划按 `gitnexus-work` re-anchor：descriptor-safe receipt digest `7ffaf98b...1dee3f`，pinned HEAD `0f5b483`，当前 `db5c8a1`；39 个 cited entries 中 35 个摘要随已提交工作变化，当前未引用 `.agents/` user harness 保留。R1–R3/R4 gate 与 NOT READY 未变；详见执行记录计划重锚段。

## 2026-10-14 N4-R2 NTEngine parent-to-child destructor edge

- 固定 hash 静态路径现闭合为：NTEngine `+0xE0` raw child owner 清理时，非空旧指针会经 `0x4E91BE` 调 `0xD31356`，再由 `0xB75029` 释放；child dtor 先进入既有 `0xD313D6` stop/wait，再对 child `+0x110` 释放 manager shared holder。NTEngine `+0xE8` 在另一处单独释放 pair。详细地址、偏移边界与脚本/output hash 见[寿命合同 §7.27](docs/research/qq-native-lifetime-contract.md#727-nteengine-0xe0-raw-owner-到-child-destructor-与-manager-holder-release2026-10-14固定样本静态)和[执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- 这是以 NTEngine dtor 执行且 `+0xE0` 非空为条件的静态 teardown edge，不证明 finalizer 注册/触发、manager last strong owner、wait 成功、完整 task stop、callback drain/quiescence、R2 consumer ABI/thread、R3 identity 或旧 AV 根因。构造 helper 对 `0x180D3111E2` 的绝对调用仍在冻结映像外，归属/语义未知。
- 验证：`.\init.ps1 -Mode docs` PASS（9 live docs / 207 local links / 33 historical entries / 10 features）；`feature_list.json` JSON PASS；`git diff --check` PASS（仅 Git 的 LF→CRLF 规范化提示）。Docker GitNexus staged gate 在本行最后修改前报 5 files / 16 symbols / 0 affected processes / low risk；CLI 展示 15 个 Section nodes 并省略 1 项，结合其既有 File-node 映射，已单独人工审阅 `feature_list.json` 状态/evidence/path diff。该行修改后重新运行 gate。工作仅固定磁盘静态证据，无 QQ/SnowLuma/native/message/Cargo 行为；feat-005/006 继续 blocked，产品 NOT READY，R4 不准入。
## 2026-10-14 N4-R2 alternate `MSFThread_xpng` manager-holder path

- 固定 wrapper hash 追查发现，`MSFThread_xpng` vtable slot 2 `0x1B63FC0` 读取 `this+0x8` payload，先调用 `0xD31686` 安装 manager holder 到 payload `+0x110`，随后调用 `0xD31A9A`。其外层 shared pair 有两条全局构造调用和一条 owner `+0x8` 成员构造调用，三条都在周边代码走 vtable `+0x10`；deleting destructor `0x1B643C0` 在 payload 非空时进入 `D31356`。证据与边界见寿命合同 §7.28、thread contract §19、execution log。
- 外层 payload `0x120` bytes 构造通过映像外绝对目标 `0x180D30FF0`；模块/hash/函数 ABI 未绑定，因此不能外推构造合同或以此认定所有 manager owner。manager last strong owner、NTEngine finalizer、全任务 stop、成功 wait、callback queue drain/quiescence、consumer ABI/thread、R3 identity、旧 AV 均未闭合；R1–R3 gates open、R4 not admitted、feat-005/006 blocked、产品 NOT READY。
- 计划在 Docker GitNexus Node 容器以 descriptor-safe reader 重新锚定：receipt canonical path 和 schema-2 provenance 一致，plan digest `7ffaf98b4b4a25f5083fa91feab3655a5dab84272dfc15a3529c467ade1dee3f`，pinned HEAD `0f5b483`，本轮开始 HEAD `afa5719`；此前 global dirty digest `efb41f833e65026d4c2e7dc141d71b59b348ce8d44e0e229d13fa9dd4bd19f0a`。计划本体未改；`.agents/` 用户 harness 保留。结束检查：`.\\init.ps1 -Mode docs` PASS（9 live docs / 207 local links / 33 historical entries / 10 features）；`feature_list.json` JSON PASS；`git diff --check` PASS（无 whitespace error，Git 仅提示 LF→CRLF）；14 个本机证据脚本/报告 SHA-256 PASS。Docker GitNexus 1.6.12/Node 22.23.2 重建后 index commit `afa5719abfe5cd1947084b56b504a2f1f93a43c9`、schema-4 runner/current content（272 files）、`incompleteReasons=[]`、status up-to-date；首轮 staged detect 为 6 files / 13 Section symbols / 0 affected processes / low risk。`feature_list.json` 仅有 File node，人工审阅唯一 feat-005 evidence 字段变化，仍 blocked；该段更新后再跑 staged gate。索引日志仍有 callable/PDG/process truncation 警告，不能把 0 affected processes 扩成代码全局无影响。未运行 Cargo tests；无 QQ/SnowLuma/native/message 行为。

## 2026-10-14 N4-R2 `ThreadPoolImpl` manager owner-stop/release (fixed-sample static)

- 固定 wrapper hash RTTI vtable 将 `ThreadPoolImpl@internal@xpng` slot 2 定为 `0x290C746`。该方法把 pair-shaped 值放入 `this+0xA0`，取首 qword 后调用 `D31686`；同表 slot 0 的 teardown 链 `0x290DA46 → 0x290C59C → 0x290C612` 在 `this+0x20` 状态检查通过时，先对该 qword 调 `D313D6`，再清空并释放 `+0xA0` 的 16-byte pair。详细边界与可复核脚本/报告 hash 见寿命合同 §7.29。
- 这是新增的条件性静态 owner teardown edge，不证明 runtime 实例、manager 最后 strong owner、wait 成功、全局 callback drain/quiescence、response consumer ABI/thread、R3 identity 或旧 AV 根因。脚本与报告位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`，hash 为 `C5D69CE9ABCC3BD1EC657FB815F401478B105A60EDF1F92C029128EC9202F8EE` / `A0AE6A23EB7117F3A35F09443E96E22AB4A4F1477BC853FE148F86F86DFCC9BE`。仅固定磁盘静态分析，无 QQ/SnowLuma/native/message/Cargo 行为；R1–R3 gates open、R4 not admitted、feat-005/006 blocked、产品 NOT READY。

## 2026-10-14 N4-R2 `FileDragManagerWin` owner-to-child teardown (fixed-sample static)

- 固定 wrapper hash RTTI 将 `FileDragManagerWin@nt` slot 2 `0x9C4852` 识别为 `D31686` 调用路径：child pointer 暂存到 owner `+0xA8`，其 manager holder 在 child `+0x110`。slot-0 `0x9C5462` 到 owner 清理后，经 `0x4E6872 → 0x4E91A6` 清空 owner slot；旧 payload 非空则经 `0x4E91BE → D31356 → B75029` 停止并释放。
- 这是固定样本静态的 owner→child teardown edge，不证明现场构造/析构、manager last strong owner、wait 成功、全任务停止、callback drain/quiescence、consumer ABI/thread 或历史 AV 根因。可复核脚本/report 在 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`，SHA-256 `EAE21E4CE8660D1C3852DF941D8C6517AE222F86DF27924A76450183E7B8A66A` / `FBF5CD676007342D1E0A3D2D97170A0D1BD56BFD262CA8D050E37DD85C7B837C`。无 QQ/SnowLuma/native/message/Cargo 行为；R1–R3 open、R4 不准入、feat-005/006 blocked、产品 NOT READY。
### 本轮文档与 GitNexus 验证（2026-10-14）

- `.\init.ps1 -Mode docs` PASS：9 live docs / 207 local links / 33 historical entries / 10 features；`feature_list.json` JSON PASS（feat-005 仍 blocked）；`git diff --check` PASS（Git 仅提示 LF→CRLF 规范化）。ThreadPoolImpl 与 FileDragManagerWin 两份固定-hash 脚本均复跑成功，报告与脚本 SHA-256 分别见寿命合同 §7.29、§7.30。
- Docker GitNexus 1.6.12 / Node 22.23.2 强制索引成功：18,469 nodes / 49,326 edges / 148 clusters / 459 flows；status up-to-date、runner/content current（272 files），staged detect 为 6 files / 24 Section symbols / 0 affected processes / low risk。索引保留 callable-flow/REACHING_DEF/CDG/跨语言/process truncation 警告；0 processes 只代表本次文档 Section 命中，不证明源码全局零影响。
- 本轮只做固定-hash 磁盘静态分析和文档；未跑 Cargo、未操作 QQ/SnowLuma、未调用 native、未发消息。feat-005/006 继续 blocked，产品 NOT READY，R4 不准入。

## 2026-10-14 N4-R2 four manager fields with matching setup/stop paths

- 固定 wrapper hash 下，`0x18584E8 / 0x18585E2 / 0x18586DC / 0x1858E2C` 分别设置 owner `+0x1D0 / +0x188 / +0x1B8 / +0x1A0` 的 manager pair，并以首 qword 调 `D31686`；`0x1852D42` 在字段非空时对同四个位置调 `D313D6`。按证据称为 stop/update method，不外推为 destructor 或特定动态类。
- `0x1852D42` 有 22 个直接 E8 调用点/16 个 caller functions；全映像 `D313D6` 直接 census 为 18 点/12 个 caller functions。两条另行定位的 `0x33D89AC`、`0x33D9304` 路径同时含 manager `D31686` setup 与同一局部 pair 的条件性 `D313D6` 清理。所有结论限固定 wrapper，间接/out-of-image caller 未覆盖。
- 可复核脚本、报告及 SHA-256 见寿命合同 §7.31，位于本机 `_reference/n4-r2-callback-manager-static-20261013/manager-method-scope-20261014/`。这仍不证明现场实例、触发时机、last strong owner、wait 成功、全 task stop、callback drain/quiescence、consumer ABI/thread、R3 identity 或 AV 根因；R1–R3 open、R4 不准入、feat-005/006 blocked、产品 NOT READY。
- 验证：`.\init.ps1 -Mode docs` PASS（9 live docs / 207 local links / 33 historical entries / 10 features），feature JSON 与三个复跑脚本 SHA-256 PASS，`git diff --check` PASS（仅 LF→CRLF 提示）。Docker GitNexus 1.6.12 / Node 22.23.2 schema-4 runner 当前、272 files、无 incomplete reasons；强制索引 18,473 nodes / 49,330 edges / 148 clusters / 459 flows，staged detect 5 files / 18 Section symbols / 0 processes / low risk，完整符号均为文档 Section。索引的 callable-flow/REACHING_DEF/CDG/跨语言/process truncation 限制保留；未运行 Cargo 或操作 QQ/SnowLuma。

## 2026-10-14 N4-R2 FileBridgeHostServiceImpl four-slot stop/update caller

- 固定 `wrapper.node` RTTI 将 `0xA18CB4` 识别为 `FileBridgeHostServiceImpl@wifiphoto@nt` secondary-vtable slot 4；COL offset `+0x20` 意味着该方法的 subobject `this+0xD8` 是 complete-object `+0xF8`。该 pair 非空时取首 qword 调 `0x1852D42`，随后 `0x504AEC` reset/release。相同 secondary vtable slot 13 的 `0xA19830` 是该 stop/update 方法的另一个直接 caller。证据仅把 typed outer field 连到四槽 stop/update，不命名 pair pointee，也不把它定性为 destructor/finalizer。
- 固定样本及脚本/输出 hash 见寿命合同 §§7.32–7.33。本项不证明运行时触发、last strong owner、成功 wait、全 task stop、callback drain/quiescence、consumer ABI/thread、R3 identity 或 AV 根因。R2/R3 未完成、R4 未准入、feat-005/006 blocked、产品 NOT READY。
- 验证：`.\init.ps1 -Mode docs` PASS（9 live docs / 207 local links / 33 historical entries / 10 features），`feature_list.json` JSON PASS，`git diff --check` PASS（Git 提示 LF→CRLF 规范化）。Docker GitNexus 1.6.12 / Node 22.23.2 schema-4 runner/content current（272 files，无 incomplete reasons）；强制 clean index 为 18,477 nodes / 49,334 edges / 148 clusters / 459 flows；最终 staged detect 映射 5 files / 15 Section symbols / 0 affected processes / low risk，含新增 §7.32 与 §23。一次增量重索引的暂时结果将文档根 Section 映射到 459 条全仓流程并误报 critical；结构化结果确认变更节点均为 Markdown Section，强制重建后影响门恢复为 0/low。保留 callable-flow、REACHING_DEF、CDG、跨语言与 process-truncation 警告；未运行 Cargo tests。仅静态磁盘分析，未访问 QQ/SnowLuma、调用 native 或发消息。

### 2026-10-14 N4-R2 FileBridge pair offset and stop-status correction

- 寿命合同 §7.32–§7.33 将 `FileBridgeHostServiceImpl` 次级 vtable COL offset `+0x20` 纳入布局：slot-4 subobject `this+0xD8` 对应完整对象 `+0xF8`；writer setup 与 `[this+0xF8]` pair cleanup 提供静态交叉核对。旧偏移按 complete-object 写法已纠正，未扩大 pair pointee 类型结论。
- 固定样本 §7.31 的 `0x1852D42` 反汇编显示四处 manager `D313D6` call 的 `RAX` 均未被消费，下一步控制判断只检查另一个 manager 字段或保存的 `BL`。父级路径没有显式 stop-success 状态；深层 `WaitForSingleObject(INFINITE)` 返回仍按 §7.15–§7.16 忽略，不证明线程未退出，也不证明 global callback drain/quiescence。
- 静态证据与 artifact SHA-256 见寿命合同 §7.33–§7.34。R2 last-owner/完整 teardown、consumer ABI/thread、R3 identity、历史 AV 均 open；R4 不准入，feat-005/006 blocked，产品 NOT READY。仅固定磁盘静态分析，无 QQ/SnowLuma、native invocation、目标写入、发信或 Cargo tests。
- 验证：`./init.ps1 -Mode docs` PASS（9 live docs / 207 local links / 33 historical entries / 10 features）；`feature_list.json` JSON parse PASS；`git diff --check` exit 0（Git 仅提示 LF→CRLF 规范化）。本轮没有运行 Cargo tests。

### 2026-10-14 N4-R2 four-slot setup caller graph

- 固定 wrapper hash 的 `.pdata`/Capstone 反向清点将 `0x185806A` 连到三个既有 setup 方法：它保存入口 `RCX` 到 `RDI` 后，以同一对象指针调用 `0x18584E8`、`0x18585E2`、`0x18586DC`。独立 helper `0x1858D66` 以保存的入口 `this` 调 `0x18585E2` 和 `0x1858E2C`；两个 helper 的直接调用者分别已计数。详见寿命合同 §7.35 与本次 execution 记录。
- 本轮只细化静态 caller graph；没有把 helper 命名为 RTTI 类或生命周期方法，也没有证明两 helper 指向同一实例。manager last strong owner、finalizer 触发、wait 成功、全局 task stop/drain/quiescence、consumer ABI/thread、R3 identity 与历史 AV 仍 open。用户要求如需 QQ 则暂停说明；本轮无 QQ/SnowLuma/native/message 操作，R4 不准入，产品 NOT READY。
- 验证：`.\init.ps1 -Mode docs` PASS（9 live docs / 207 local links / 33 historical entries / 10 features）；`feature_list.json` JSON PASS；`git diff --check` exit 0（仅 LF→CRLF 规范化提示）。Docker GitNexus 1.6.12 / Node 22.23.2 强制重建为 18,485 nodes / 49,342 edges / 148 clusters / 459 flows，status up-to-date、runner/content current（272 files）、无 incomplete reasons；staged detect 为 4 files / 13 Section symbols / 0 affected processes / low risk。仍有 callable-flow cap、REACHING_DEF 丢 311/4311、CDG skip 和 flow truncation 警告；低风险结论仅限文档 Section 命中。未运行 Cargo tests，未访问 QQ/SnowLuma。
