# 2026-10-11 N4 响应恢复执行记录

## 范围与结论

- 执行入口：[N4 响应恢复计划](../plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md)。
- 固定源基线：`0f5b483`；计划提交：`d637901`。开始时 harness 文件已有未提交改动；本次保留并继续修改必要状态文档，没有提交或推送。
- 本次先完成 R1 离线恢复合同，推进 R2 callback capture、R3 本地 durable wire sequence 与 R5 文本/HTTP LAB；后续增补完成了 R5 core/store 接收与查询接线。R4 准入、R3 强身份、native projection 与 R6 transport/应用验收仍未完成。
- 产品状态仍是 **NOT READY**；feat-005 与 feat-006 继续 blocked。没有启动 QQ、注入、发送真实消息、复用旧 PID 或自动重发 unknown。

## 已交付的离线代码与证据

### R1：请求身份与旧账本恢复

- 原始 daemon `request_id` 贯通 IPC Dispatch、resident、HostOp 与 response；假 adapter 返回内部/未知 ID 时只审计，不将其当作原请求结果。
- `NativeStarted` 由 resident 的实际 HostOp 调用边界产生；内联回调路径保证 started notice 排在 outcome 之前。队列受理不再等于 native started。
- `AcceptedUnconfirmed` 已跨 model、IPC business revision、bridge、daemon、actor 和 journal 传播。只有带非空 native identity 的强成功可进入 `ConfirmedSuccess`；接受回执不释放终态保留槽、不触发重派。
- journal 恢复遇到旧 `ConfirmedSuccess(None)` / 空身份记录时保留旧行，按未确认事实恢复；不编造 ID、不改写历史、不自动重发。协议业务 revision 与 IPC v3 framing 分开握手，旧语义 bridge 明确拒绝。
- 测试入口：`daemon_chain`、`daemon_client_lab`、`ipc_contract`、`recovery_contract`、`runtime_contract`、`resident_lifecycle`。本次全量离线运行通过，命令与结果见“验证”。

### R2：回调最小捕获与寿命边界

- `consumer_probe` 改为固定容量 16 个不可变 session contexts 和 256 个 CAS 管理的 response slots。回调只做 bounded capture；日志格式化与文件 I/O 在 resident worker drain 阶段执行。注册容量耗尽会阻止 attach。
- old generation delegate 可找到仍保留的隔离上下文；满槽计数 overflow，不覆盖 ready slot。callback 中的常规 Rust panic 有边界，但 `catch_unwind` 不捕获 Windows SEH/访问冲突，也不证明任意指针安全。
- 本次只读复核本机磁盘文件 `D:\Program Files\Tencent\QQNT\versions\9.9.33-52230\resources\app\wrapper.node` 的 SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`，与冻结静态样本一致；这不能证明某个运行中进程实际加载了该文件。对固定文件的 RVA 字节静态解码确认 `1DB2` 接收 pair 地址、读取 ctrl、递减 ctrl+8，并在前值为零时走 ctrl vtable+8 与 QQ deallocator；其 counter/对象销毁整体模型仍未闭合。`2632` 仍是 manager `+0x10` 的 op=3，和 pair release 分离。
- 资源所有权与清理状态：

| 资源 | 当前证据/动作 | 未闭合点 |
|---|---|---|
| getter `{obj, ctrl}` | `1DB2(pair*)` 与 ctrl+8 的固定 hash 静态证据已复核；单个 `G3NativeOps` lease 从 probe→attach→adapter 复用；attach 失败可对该未提交 pair 调用 `1DB2` | 已尝试 native submit 后不释放 lease；完整 counter/对象销毁、transport teardown 与 callback quiescence 未闭合 |
| QQ `operator new` 的 body input | `732A80` 同步复制 `[begin,end)` 到 QQ 自有 request；本侧在调用返回后以配对 `B75029` 释放临时 body buffer | 静态结论仅绑定该 wrapper hash；不代表 transport 已发送或业务成功 |
| service self string | 移除原来未传入参数的长字符串副本；`732A80` 从 service `+0x1F0` 自行复制字符串 | callee 行为仍绑定固定 hash；没有现场结论 |
| callback delegate | `31C5F8` 静态调用 manager op=0 clone 并清空源；`2632` 调 manager op=3；`7328EA` 销毁 nested callback 并递减 metadata strong count；`73BD6C` 同步调用用户 invoker。bridge image 在 context 发布前 PIN；resident 停止时关闭本地 callback 准入并等待已准入回调退出 | op=1/其它 manager 操作的调用者与语义未穷尽；当前宿主线程身份、QQ stop/cancel/drain/join/quiescence 与 QQ 侧卸载寿命未证 |
| capture slots | 固定 256 项；worker 消费后释放槽，overflow 只计数；resident generation 隔离后，迟到 callback 仅计 `late_callbacks_rejected`，不再占槽 | 此本地栅栏不排空 QQ 队列或证明 native quiescence；无独立进程 AV dump/堆栈，既往 `0xc0000005` 原因仍 unknown |
| report context | 固定 16 项且进程内保留，避免迟回调 UAF；第 17 项 admission fail | 生命周期内不复用，长期运行容量与进程重启合同需要进一步定义 |

因此 R2 只交付 callback capture 的离线收敛，不满足计划的所有权、AV 定因和驻留准入完成门。

### R3：序号与身份

- 已采用 core AccountActor journal 作为常驻候选路径唯一 wire sequence authority，见 [ADR](../decisions/2026-10-11-wire-sequence-authority.md)。同一序号从 Dispatch DTO 贯通到 wire encode/ledger；落盘失败先于 native 拒绝，恢复从最大已用值之后继续，容许空洞。
- response terminal 走统一消费并保留有界 notice capacity；CPB 同文/peer/time matcher 只保留诊断用途，不再作为强回执。
- 增加重复相同 CPB peer/text/time 记录 fixture：记录顺序/来源不可辨时，每条都继续得到多个候选，不消费任一 send 登记。
- 没有找到 callback 与 native record 间的强身份桥接键。当前明确 **stable_identity unsupported**；同步标准 send 不返回 `ok`，AcceptedUnconfirmed 最终可变为 unknown，但永不自动重发。R3 的序号子块和歧义安全子块已有离线证据，强身份门仍 blocked。

### R2/R3 先行补齐（固定 hash 静态 + 离线，不含现场动作）

- `G3NativeOps` 现在持有完整 `{obj,ctrl}` pair；常驻路径将同一实例从 getter 探测传到 `NativeSession::attach` 再移入 `NativeHostAdapter`。重复环境探针和每次 submit 复用一份 lease，避免每次 getter 都泄漏一份强引用。
- 固定 hash 下 `1DB2` 是 pair release；不再把 manager disposer `2632` 当作 pair release。attach 失败或首次探针锚点/transport 不符时释放尚未提交的 pair；尝试过任何 native submit 后保留该 pair，不在 Drop 中释放。
- callback 生命周期静态边界进一步收窄：已知的 op=0 clone、move 源清空、op=3 dispose、nested callback 销毁与直接 invoke 顺序写入上表；thread contract §10 的历史样本显示 QQ worker/dispatcher 回调，但当前会话未复核，断链/停止后的 drain/quiescence 仍未知。
- `g3_session` 曾在取得服务对象之前调用 `probe_ready(0)`，该函数必然拒绝空 service address。移除该不可达前置调用，让 getter 获得的同一 lease 经过 attach 与 send。
- 删除未被 `732A80` 调用参数使用的 self-string QQ 副本，消除无意义分配和泄漏。body 使用 QQ `B75000` 分配，callee 同步深复制后由 `B75029` 对称释放。
- 以上是固定文件静态分析加本地源码/合同证据；没有 attach、调用、发送或探测运行中的 QQ 进程，也未改变 SnowLuma/QQ 的连接状态。

### R2 剩余寿命风险收窄：bridge callback module pin

- 两条实际 callback 构造路线（常驻 `consumer_probe::init` 与一次性 `g3_send_run → leak_ctx`）现在共用 `leak_ctx` 的 module-lifetime gate。它在发布 context 前以 `GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN, consumer_invoke)` 固定 bridge image；Win32 API 失败时 context 不发布，两个调用方都在 native submit 前返回失败。
- PIN 使 bridge 函数页在 QQ 进程退出前保持映射，避免迟到 delegate 调用已卸载的 Rust 函数；context 与 capture slots 仍保留旧代次。实现不卸载 QQ 对象，不改变 SnowLuma/QQ 连接，也未启动现场动作。
- PIN 使 bridge 函数页在 QQ 进程退出前保持映射，避免迟到 delegate 调用已卸载的 Rust 函数；实现不卸载 QQ 对象，不改变 SnowLuma/QQ 连接，也未启动现场动作。
- resident 停止时 `GenerationGuard` 先标记该代次已隔离，再等待已经通过本地准入的回调退出。隔离后的迟到响应 callback 只增加原子审计数，不解引用错误/跨度输入、不入共享槽；worker summary 可将其写入 report。它保留未知请求语义，不将迟到结果变成成功或自动重发。
- `native_resident_run` 现在保留 pump、receive、daemon worker 三个 `JoinHandle`。正常 stop 与 spawn 失败清理都会通知/汇合已启动的本地 worker；join 使用有界 deadline，超时或 worker 退出异常时写失败阶段并返回非零。offline helper test 覆盖 joined 与 timeout 两种结果。此证据不覆盖 QQ dispatcher 自己的队列/线程寿命，也不允许提前释放已提交的 QQ service pair。
- `cargo test -p caligo-bridge --lib --features research consumer_probe::tests --locked --offline --target-dir target-harness -- --test-threads=1` PASS：7/7，含 PIN context gate、活动回调正常捕获、隔离等待在途调用、隔离后不读无效诊断指针、未登记 generation orphan 不占槽。测试进程结果不等于 DLL 在 QQ 内加载/回调后生命周期现场验证，也不证明 dispatcher quiescence。
- 本地准入栅栏只闭合 bridge image 保留和停止后的持续迟到 callback 槽占用；不闭合 QQ dispatcher 的 op=1/其它 manager 分支、owner/consumer thread、stop/cancel/drain/join/quiescence、最终业务结果持久交付及历史 `0xc0000005`。产品 feat-005/006 保持 blocked，R4 仍不准入。

### R5：OneBot 文本与 HTTP LAB

- `SendTarget` 区分群与私聊 UIN，`send_msg` 按类型保留目标；目前常驻 native 私聊 UIN→UID 映射未证，adapter 明确拒绝私聊。
- 已实现的 action layer 覆盖文本 send、get_msg、login/status/version；缺失运行时身份或消息时返回 not-ready / not-found，不填假值。OneBot 标准响应保留 `data:null` 与原始 `echo` 类型；`*_async` 是 Caligo 扩展，不是 OneBot V11 公共动作名。标准成功要求 durable int32 OneBot ID。
- MessageStore 是单写者 append-only JSONL，`sync_all` 后才返回稳定 int32 ID；主键是 account + chat type + peer + non-empty native ID，seq/request/cookie/text 均不作为身份。重启、重复身份、冲突及不完整身份有离线测试。
- MessageStore 尚未接入 AccountActor 的接收事件与 caligod 服务装配；现有 HTTP `serve_one` 仍是组件/LAB，没有普通构建的监听/动作启动入口。OneBot action/HTTP 代码因此不代表产品可用。
- HTTP LAB 支持 GET route/query 与 POST JSON/form 的动作解析、分包正文、长度上限、Content-Type 与 token 检查；HTTP POST 事件上报、正向 WS、反向 WS、真实客户端互通和慢消费者矩阵均未交付。
- 新增 `onebot_http_lab` 组合测试：HTTP POST send → fake daemon 观察 typed target → durable MessageStore 写入后重开 → HTTP GET `get_msg` 读出完整标准字段。该链路是本机 fake LAB，不经过 AccountActor 或 QQ。

## 失败诊断

初次运行默认并行的 `.\init.ps1 -Mode all` 在 `l17_hundred_close_rebuild_cycles_return_to_baseline` 报告 allocator live count `before=202 after=450`。该测试此前使用进程级全局计数，可能被同一 test binary 的并行用例分配污染：精确单测通过，`resident_lifecycle` 串行全过。随后测试 allocator 改为当前测试线程的 opt-in 计数，原本两项 L17 检查保留；修改后默认并行运行该二进制 18/18 通过。此诊断解释与数据相符，但不构成对历史 research AV 的解释。

本轮 quarantine 第一次 full mode 检查因新增 orphan-callback 测试把共享 orphan 诊断误认为写到当前测试 context 而失败；源码实际写往进程首个可用 context。将断言改为检查 orphan 计数增量与 drain 无结果后，最终 default/research full mode 均通过。

owned-worker join helper 的定向测试先暴露固定 50ms 轮询可能越过短 deadline，以及日志断言不应依赖 JSON 字段顺序；等待间隔现受剩余 deadline 限制，断言只检查状态字段。最终 helper 测试和完整 default/research 验证均通过。

R2/R3 后续验证中，首次定向编译发现 research cfg 下 callback 数组需可变传参；调整两种 cfg 分支声明后，定向歧义测试与 full mode all 通过。该编译错误已修复，没有触发 QQ/现场动作。

## 验证

本节记录计划执行过程中的失败、修复与最终验证。

| 检查 | 结果 |
|---|---|
| `.\init.ps1 -Mode docs` | PASS：9 live docs / 160 本地链接 / 33 历史入口 / 10 features |
| `cargo test -p caligo-bridge --test resident_lifecycle --locked --offline --target-dir target-harness` | PASS：18 passed；并行资源快照修正后复核 |
| `cargo test -p caligo-core --test onebot_http_lab --locked --offline --target-dir target-harness` | PASS：1 passed；HTTP send/store reopen/get_msg 合成闭环 |
| `.\init.ps1 -Mode all`（最终默认并行复跑） | PASS：普通 workspace 191 passed；research workspace 221 passed；0 failed；原始摘要在忽略目录 `target-harness/n4-all-final.log` |
| `.\init.ps1 -Mode all`（allocator 调整前的首次运行） | FAIL：L17 快照报 `before=202 after=450`；精确单测与整个 resident_lifecycle 串行通过，修复测试计数隔离后默认并行 18/18 及 full mode 均通过 |
| changed Rust files `rustfmt --edition 2021 --config skip_children=true --check` | PASS：34 个修改/新增 Rust 文件（33 tracked + 1 新文件）；跳过子模块，未改动开工时干净的旁支模块 |
| `git diff --check` | PASS；Git 有行尾转换提示，但没有 whitespace error |
| GitNexus `detect-changes --scope unstaged --limit 20` | 35 files / 139 symbols / 381 affected flows / risk `critical`；跨层高影响结果，图不能替代代码审阅或全量测试 |
| R2/R3 follow-up `cargo test -p caligo-bridge --lib --features research native_resident::tests::repeated_identical_records_remain_ambiguous_and_unconsumed --locked --offline --target-dir target-harness` | PASS：1 passed；重复同文/peer/time 记录保留 2 个待发送候选并标记歧义 |
| R2/R3 follow-up `rustfmt --edition 2021 --config skip_children=true --check`（g3_ops / g3_session / native_resident） | PASS：3 个本次触及 Rust 文件格式检查通过 |
| R2/R3 follow-up `.\init.ps1 -Mode all` | PASS：文档 9 live docs / 161 links；default 191 passed / 0 failed；research 222 passed / 0 failed；原始输出 `target-harness/n4-r2-r3-followup.log` |
| R2 callback module pin `cargo test -p caligo-bridge --lib --features research consumer_probe::tests --locked --offline --target-dir target-harness -- --test-threads=1` | PASS：4 passed / 0 failed；验证 context gate 在测试进程成功 pin 并支持后续 generation；不证明 QQ loaded-DLL 现场寿命 |
| R2 callback quarantine `cargo test -p caligo-bridge --lib --features research consumer_probe::tests --locked --offline --target-dir target-harness -- --test-threads=1` | PASS：7 passed / 0 failed；验证 resident generation 隔离等待已准入 callback、迟到 callback 不读错误/跨度参数且不占响应槽，未登记 generation 只计 orphan；仍不证明 QQ dispatcher 停止或排空 |
| R2 callback quarantine `rustfmt --edition 2021 --config skip_children=true --check crates/caligo-bridge/src/consumer_probe.rs crates/caligo-bridge/src/native_resident.rs` | PASS：本次触及的两个 Rust 文件 |
| `cargo test -p caligo-bridge --lib --features research native_resident::tests::owned_worker_join_is_reported_and_timeout_is_not_success --locked --offline --target-dir target-harness -- --test-threads=1` | PASS：1 passed；验证有界 join 成功与超时分别有不同结果 |
| 最终 `.\init.ps1 -Mode all`（输出 `target-harness/n4-r2-owned-join-all.log`） | PASS：docs 9 live docs / 161 links；default 192 passed / 0 failed；research 226 passed / 0 failed；QQ field acceptance separate |
| 最终 `.\init.ps1 -Mode docs` + `git diff --check` + feature JSON parse | PASS：9 live docs / 161 links；whitespace check 通过；feature_list.json 可解析 |
| GitNexus forced reindex + status（CPB identity gate 与状态文档更新后） | PASS：runner identity current、content drift current、coveredFiles 270、incompleteReasons empty、status up-to-date。增量文档链接重解析会继续补图；最终规模和截断提示见忽略日志 `target-harness/n4-final-index.log` 与 `target-harness/n4-final-gitnexus-status.log`，不能用缺失路径证明无调用 |
| GitNexus follow-up impacts | `probe_service` LOW（2 个 process / 2 modules，4 个 impacted nodes）；`native_submit` LOW（1 direct caller）；`NativeHostAdapter.new` UNKNOWN/0 resolved，源搜索确认唯一构造点在 `native_resident_run`。风险以源码核对为准 |
| GitNexus follow-up `impact consumer_invoke --direction upstream` | CRITICAL：6 个直接测试调用点 / 5 个流程 / 1 个模块；`native_resident_run` 上游 LOW：1 个生产入口。流程命中需结合源码审阅，图对分支/入口有截断 |
| GitNexus final `detect-changes --scope all --limit 1000` | risk `critical`；覆盖整棵既有 dirty N4/harness 工作树，不是本次 AV 复核文档改动的单独风险量。增量文档链接重解析时计数会变化，当前计数以忽略日志 `target-harness/n4-final-detect-changes.log` 为准 |

前次 research `0xc0000005` 既有失败保留为未定问题；本次研究测试通过没有抹去或解释它。GitNexus impact 对 `CountingAlloc` 返回 `UNKNOWN/0 resolved`，人工 `rg` 确认只被本测试文件两个分配断言使用。图的其它调用边存在截断/遗漏，代码结论以当前 source 和测试为准。

### 2026-10-11 后续阻塞复核

- 回看忽略目录 `target-harness-verification.local.md` 的原始输出，首次并行 research 失败落在 `caligo_bridge-c00aae2a8f83540a.exe`，退出码 `0xc0000005`；日志只显示前两项 `asyncrun` 测试通过，无法识别当时正在执行的测试或故障地址。
- 本次独立复跑同一个 research bridge lib 测试二进制：`--test-threads=1` 为 69/69；默认并行（`--nocapture`）也为 69/69。忽略输出分别保存于 `target-harness/n4-av-serial-recheck.log` 与 `target-harness/n4-av-parallel-recheck.log`。这说明本次未复现，不解释旧崩溃，也不证明并行竞态已经修复。
- 查询了本机 Application Error/WER 事件和 `CrashDumps` 中匹配的 dump，并检查可用调试器命令；没有找到事件、dump、`cdb` 或 `procdump`。因此仍无独立进程异常堆栈/转储可供根因分析。
- 重新计算固定磁盘 `wrapper.node` SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。这是静态文件身份，不是运行进程加载证明；本轮没有触碰 QQ 或 SnowLuma。
- 固定-hash 现有静态导出继续只闭合发送入口可见的 delegate move/invoke/dispose 边界。当前 consumer/owner thread、完整 manager 操作、QQ dispatcher stop/cancel/drain/quiescence、稳定 native identity 均未闭合；R4 保持未准入。

### 2026-10-11 CPB 事件身份门

- 复核 `native_resident` 接收轮询时发现它曾把会话内 CPB `seq` 合成为 `cpbseq:{seq}` 并交给 `Resident::ingest_event`。`native_receive` 合同明确 seq 不是稳定 native ID；该合成键可能把候选序号错误用于跨事件去重和 OneBot 身份映射。
- 删除该合成路径。`RecordFields` 现在分别表达 opaque `sender_uid`、已解析 `sender_uin`、稳定 `native_id`；当前 CPB decoder 只填原来有证的字段，不填 `sender_uin/native_id`。新的 `cpb_event_from_record` 只有在稳定 native ID、sender UID→UIN 映射、peer/seq、正文/平台时间和配置群目标同时可证时才建业务事件。缺少这些字段的完整候选只增加 `identity_blocked`；字段不完整的记录仍计入 `partial`。不会将未知 peer 推断为私聊。
- `cargo test -p caligo-bridge --lib --features research cpb_record_requires_stable_message_and_resolved_sender_identity --locked --offline --target-dir target-harness` PASS：1/1，覆盖无 native ID、无 sender UIN、以及未分类 peer 的拒绝；完全合成的已齐字段只验证 gate 组合，不代表 CPB decoder 已能提供身份。
- `rustfmt --edition 2021 --config skip_children=true --check crates/caligo-bridge/src/native_receive.rs crates/caligo-bridge/src/native_resident.rs` PASS。
- `.\init.ps1 -Mode all` PASS：docs 9 live docs / 161 local links；default 191 passed / 0 failed；research 227 passed / 0 failed。原始输出 `target-harness/n4-cpb-identity-gate-all.log`。输出仍含已有 Rust warnings。该离线结果不证明 QQ 现场回调/消息身份，也未触碰 QQ 或 SnowLuma。
- R3 的 CPB→强身份桥接没有完成：当前 native ID 与 sender UIN decoder 字段仍为 `None`，因此 CPB 常驻路径不会发业务事件。此改动消除伪身份，不代表 receive/OneBot 接线完成；stable identity、R2 dispatcher/线程/排空合同和历史 AV 仍是 R4 blocker。

## 仍需满足的门与下一步

1. 继续补 callback manager 未穷尽操作、当前 owner/consumer thread、QQ 原生 stop/cancel/drain/quiescence 合同；若 AV 再现，先保留独立进程 dump/异常地址与线程栈。当前串行和并行复跑均通过，不能替代旧失败的根因诊断。bridge module 已在 callback context 发布前 pin，resident stop 另有本地准入隔离；QQ service pair 和 delegate 继续 resident/quarantined，不热卸载。
2. 从固定-hash CPB/native record 研究中建立独立的 stable native ID 与 sender UID→UIN 映射；当前 CPB 事件 gate 会拒绝缺身份记录，标准同步 send 仍不成功。
3. 将 MessageStore 安全装配到 core 接收事件和 OneBot reader；以实际 daemon/journal reopen 验证 ID 稳定后，才推进 R5 产品状态。
4. R4 只有当 R1–R3 完成门和当前 ABI/线程/寿命合同全部满足且现场授权/新准入清晰时才能开始；当前不满足，不启动实验。
5. R6 增补 HTTP POST event、正向 WS、反向 WS，先对照官方 OneBot 文档，再用单一实际应用完成端到端 receive/send/get_msg 与慢连接矩阵。

feat-005 / feat-006 保持 blocked，整个项目保持 NOT READY。

### 2026-10-11 R5 core 接收索引与查询装配（离线）

- 数值 account 的 daemon 从 journal sibling path 打开 MessageStore，并通过
  `AccountActor::open_with_message_store` 装配；非数值 account 不猜测 OneBot account ID，
  不会打开业务索引。
- AccountActor 对具有显式 `OneBotMessageProjection` 的事件先写 core journal，再 append/sync
  MessageStore，之后才允许 EventAck。store 写失败时事件保留待写状态、actor 降级并拒绝 ACK；
  bridge connection 断开以触发原事件 replay。启动恢复从 projection-bearing journal rows
  幂等补齐 index，旧 journal row 继续可读。
- Daemon 实现 `StoreReader`；`daemon_chain::event_roundtrip_through_pipe_and_journal`
  经 bridge pipe 写入合成已投影事件，验证去重、EventAck、control drain、持久 OneBot ID、
  Daemon reader 与 OneBot `get_msg` 动作返回完整字段。recovery contract 覆盖 actor/store
  reopen 后 ID 稳定及无投影事件不分配 ID。
- CPB bridge client 当前明确发送 `onebot_projection: None`。因此 core 接线不构成原生
  receive 业务身份证据；native ID、peer/sender UIN、真实 ID 与平台时间均须由上游独立解析后
  才可生成投影。普通 caligod HTTP listener、HTTP POST event、WS/反向 WS 和真实应用互通仍属后续门。
- 定向检查：`cargo test -p caligo-core --test daemon_chain event_roundtrip_through_pipe_and_journal --locked --offline --target-dir target-harness`
  通过 1/1；随后 `.\init.ps1 -Mode all` 通过，包含 docs 9 live docs / 163 local links / 33 historical entries / 10 features，默认与 research 两个 workspace 的所有 test binaries 均成功退出。现有 compiler warnings 保留，不影响退出结果。
- `rustfmt --edition 2021 --config skip_children=true --check` 对本次 core/model/bridge 变更路径通过；`git diff --check` exit 0（仅有 Git LF→CRLF 提示）；`feature_list.json` JSON parse 通过。验证全为本地/离线结果，不是 QQ/OneBot 应用现场验收。
- GitNexus 最终刷新成功；本节末次 status/detect-changes 结果写于忽略日志 `target-harness/n4-r5-final-index.log`、`target-harness/n4-r5-final-status.log`、`target-harness/n4-r5-final-detect.log`。status 必须显示 `up-to-date`、runner identity/content drift `current`、无 incomplete reasons；detect risk 覆盖整个未提交 N4/harness 工作树。现有 callable-flow/REACHING_DEF/process truncation warnings 意味着图输出不能证明不存在路径。
- 本轮只读复核固定磁盘 `wrapper.node`，SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。已有导出仍未闭合 callback manager 全操作语义、当前会话 owner/consumer 身份与 QQ stop/cancel/drain/quiescence；没有新的 WER/dump/堆栈可解释旧 `0xc0000005`。没有启动或操作 QQ/SnowLuma。
- 搜索本机 local-only evidence 时发现一份更早（2026-10-09）的 QQ Crashpad dump；只读 MINIDUMP header 给出的 exception code 为 `0xE06D7363`（C++ exception），不是旧 research test 记录的 `0xC0000005`。没有证据将二者关联，因此没有读取/提取该 dump 的其它内容，也不拿它解释 AV；旧 research AV 仍缺匹配 dump 与故障栈。
- 更早记录中“MessageStore 尚未装配 AccountActor”属于接线前状态；由本节更新。
  feat-006 与项目状态继续 blocked / NOT READY。

### 2026-10-12 R3 回执期限与未知诊断补齐（离线）

- 所有 native callback 非零 `errCode` 均不再直接映射业务拒绝。`1006501` 与来源未验证的其它码成为 `DeliveryUnknown`；v3 result 保留原始 `errCode`、`errStr` 和 provenance 未验证说明，Actor 写入 append-only receipt audit。审计 journal 写入失败现在向上返回 `JournalFailure` 并触发 actor 降级。
- core 使用请求 journal history 的受理时间计算剩余 deadline，随 v3 `Dispatch.timeout_ms` 发送到 resident→`HostOp::SendText`→native session。v2 DTO 保持原样；v3 dispatch 与 unknown diagnostic 独立定义，business protocol revision 从 1 提升到 2，旧语义 revision 被拒绝。
- response ledger 注册 timeout，到期发出一次 `DeliveryUnknown` terminal。该 registration 保留为最多 60 秒、最多 256 项的 tombstone；同一 cookie/request 后续只有带 native identity 的强成功可以更正 unknown。宽限期后槽被回收；所有路径均不重发原动作。
- 新增/扩展测试覆盖 v3 timeout/diagnostic roundtrip、旧 business revision 拒绝、1006501 unknown 分类与诊断保留、超时 unknown、同请求迟到强身份更正、grace 清理与 resident 有界槽释放，以及 core receipt audit。
- `.\init.ps1 -Mode all` PASS：docs 9 live docs / 163 local links；default 与 research 全套测试二进制均成功退出，且 `QQ field acceptance is separate`。运行中第一次暴露一条预期 audit 数量从 2 增至 3 的断言；断言已修正后 full run PASS。此前 `cargo fmt --all` 带来的旁支纯格式化差异已从工作树撤销。
- 此次完成的是 R3 的 nonzero result classification 与 ordinary deadline/tombstone 子块，不是强身份闭合。稳定 native identity、sender UID→UIN、R2 callback manager 全操作 ABI、当前 owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence 与旧访问冲突根因仍开放；R4 不准入。没有启动 QQ 或改变 SnowLuma。

### 2026-10-12 R5 普通 caligod HTTP listener（离线）

- 普通构建的 `caligod` 现在默认监听 `127.0.0.1:5700`，允许显式配置其它 loopback 地址并拒绝非 loopback bind；listener 复用现有 OneBot parser/action registry、Daemon `StoreReader` 与 action sink，不连接旧注入 runner。HTTP 每连接独立 worker，最多 16 个；header/body 读取共享 10 秒绝对 deadline，写响应限时 5 秒，worker 饱和返回 503。
- 群文本请求仅执行一次 Daemon dispatch；HTTP 标准成功要求同请求强 native 成功和匹配的已持久 OneBot int32 ID。迟到或无持久投影按 unknown 返回，不自动重发。常驻私聊 UID 映射未证，私聊动作明确 unsupported。
- 集成测试用 fake bridge 发出明确的强成功回执和合成 OneBot projection，经过真实 daemon pipe、actor、journal/store 与 HTTP `get_msg` 返回完整字段；这验证装配闭环，不证明当前 native receive 可提供 projection。当前 `daemon_client` 仍发 `onebot_projection: None`，登录昵称/账号和真实消息身份缺口也仍开放。
- `cargo test -p caligo-core --lib onebot_http::tests::slow_header_reader_does_not_block_a_valid_request --locked --offline --target-dir target-harness -- --nocapture` PASS（1/1）；`cargo test -p caligo-core --lib --test daemon_chain --locked --offline --target-dir target-harness` PASS（core 44/44、daemon_chain 5/5）。Rust 编译含已有 bridge/core warnings，不影响退出；未跑 QQ 或 SnowLuma。
- 本机 GitNexus CLI 工作树索引后 status 为 `up-to-date`、runner identity `current`、content drift `current`、无 incomplete reasons；索引报告 17,763 nodes / 47,711 edges / 145 clusters / 441 flows。流程入口丢弃、callee 分支截断与 reaching-defs cap 仍存在，查询缺边不作为无调用证明；`serve_http` 上游 LOW，`serve_parallel` 图返回 UNKNOWN/0 caller，源搜索和 Rust 装配核对确认新增调用位于 `serve_http`。
- 没有可读取的 GitNexus MCP context server；本机 Docker Linux Node 24 容器内运行 descriptor-anchored `read-plan` 与 schema-2 snapshot 重算计划证据。计划路径/digest 匹配；39 个引用项均已比较，HEAD 和全局 dirty digest 因先前 R1–R3 提交与本轮实现变化而不同；没有超出引用清单的新增脏路径。依赖当前源码与提交历史重新核对，不改写计划体。
- R5 HTTP 动作 listener 与慢读隔离只闭合离线入口和假 bridge 闭环。HTTP POST events、正向/反向 WS、真实应用互通、native receive projection、私聊 UID→UIN 及 R2/R3/R4 准入门继续开放；feat-006/产品状态保持 blocked / NOT READY。

### 2026-10-12 R6 OneBot V11 transports（离线合同）

- 普通构建 `caligod` 增加可选 loopback 正向 WS listener、HTTP POST events worker 与反向 WS Universal client。三种 transport 和已有 HTTP API 均复用 `onebot_server::action_handler`，最终仍走同一 Daemon/AccountActor；未新增第二套 native send 路径。
- HTTP POST 使用 JSON body、`X-Self-ID`、可选 HMAC-SHA1 `X-Signature`，连接/读/写 timeout 受限；响应最多读取 64 KiB。JSON 快速操作的 `reply` 通过共享动作层执行，HTTP POST worker 完成响应处理后才向 WS 事件订阅者发布该事件。`caligod` 在启动 bridge 接收循环前等待 HTTP POST subscriber ready，避免首条事件抢跑。
- 快速回复只执行动作层当前已支持的内容。群回复若 `at_sender` 未明确设为 `false` 则不分派，因为当前不支持自动 @ 段；未支持目标/消息段由动作层拒绝，unknown 不自动重发。私聊目标映射缺证时仍 unsupported。
- 正向 WS 限制 `/api`、`/event`、`/` 路由、使用 daemon token 鉴权，最多 16 个并发客户端；每个 WebSocket/HTTP POST transport 有独立的 256-event bounded queue。反向 WS 作为 Universal 客户端连接 `ws`/`wss`，发 OneBot V11 `X-Self-ID`、`X-Client-Role: Universal` 和可选 Bearer token，并按配置间隔重连。HTTP POST 与 WS 同时配置时，事件先完成 HTTP POST（含 quick operation），再推给 WS。对应合同核对：[HTTP POST](https://github.com/botuniverse/onebot-11/blob/master/communication/http-post.md)、[正向 WS](https://github.com/botuniverse/onebot-11/blob/master/communication/ws.md)、[反向 WS](https://github.com/botuniverse/onebot-11/blob/master/communication/ws-reverse.md)、[V11 message event](https://github.com/botuniverse/onebot-11/blob/master/event/message.md)。
- `message_event_json` 仅对 incoming receive、显式 projection 完整具备数值 QQ identity、OneBot message ID、time/subtype/font/native ID 和目标字段时输出标准事件；不从本地 seq、opaque UID 或正文相似性推导身份。当前 `daemon_client` 仍发送 `onebot_projection: None`，所以此 fanout 没有当前 native CPB 生产来源。
- 测试：`cargo test -p caligo-core --lib onebot_transport --locked --offline --target-dir target-harness -- --nocapture` PASS（6/6，含 HMAC vector、签名 HTTP 204、正向 WS action/event、反向 WS V11 headers/action/event、快速回复拒绝边界、HTTP POST→WS 顺序）；完整 `cargo test -p caligo-core --locked --offline --target-dir target-harness` PASS；`daemon_chain` 并行全组 PASS（5/5）。
- 全仓首次 `init.ps1 -Mode all` 在 `caligo-bridge` lib test process 以 `0xc0000005` 退出；同二进制默认构建串行诊断 `cargo test -p caligo-bridge --lib --locked --offline --target-dir target-harness -- --test-threads=1` PASS（58/58）。第二次全仓运行在 `full_chain_send_event_and_stop` 失败，源码核对确认测试 fixture 多测试共享 `CHAIN_SENDS` 全局计数，导致 `NM-1` 断言会受并行用例污染；已改为每个 `ChainHost` 独立计数。该用例单独 PASS、并行 `daemon_chain` 5/5 PASS。第三次 `init.ps1 -Mode all` PASS；首次 bridge AV 仍保留为未解释失败，后续通过不证明其根因已解决。
- 本次 touched Rust paths 的 `rustfmt --edition 2021 --config skip_children=true --check ...` PASS；`git diff --check` PASS；`feature_list.json` JSON 解析 PASS；`init.ps1 -Mode all` 文档门 PASS（9 live docs / 163 local links / 33 historical entries / 10 features）。全仓 `cargo fmt --all -- --check` 曾报告多处既有源文件格式差异，因此未对无关工作区运行全量格式化。
- R6 交付层级仅为普通构建 transport 接线和本机合成协议合同。真实 NoneBot/Koishi 互通、native receive projection、私聊 UID→UIN、同消息 source/request/receipt/store/event 现场链，以及计划中 N4/T14–T17 的完整矩阵均未验收；R2 callback ABI/thread/quiescence 与 R3 stable identity 前置仍开放，R4 不准入。feat-006 保持 blocked，产品保持 NOT READY；没有启动 QQ、注入、发送真实消息或改变 SnowLuma 连接。

### 2026-10-13 R2 callback manager dispatch 静态补充

- 只读复核本机冻结 `wrapper.node`，磁盘 SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；Ghidra 12.1.4 使用 `-noanalysis`，定向脚本固定校验 hash。没有启动、读取或操作 QQ 进程，也没有改变 SnowLuma 连接。
- 从 `73BC5C` 的表 `0x3F7052C` 和 `73C2AA` 的表 `0x3F70540` 解出 op 0–3：inline 分支目标分别为 `73BCBE`、`73BCF3`、`73BC7F`、`73BC7F`；heap 分支为 `73C30F`、`73C32A`、`73C2DA`、`73C2DA`。定向补充反汇编确认 op=1 的 inline/heap 路径差异、op=2/3 的清理路径以及仅 op=2 重置 manager/invoker；没有给未知调用语义补猜测名称。
- 固定样本中两个 manager 都直接按 op 索引相对跳表，入口未见范围检查。`31C5F8` 已知传 op=0，`2632` 已知传 op=3；op=1/op=2 的全部调用者/合同与其它输入值处理仍未证。heap 路径的 `1FD0` 收到 closure 指针与 `0x40` 参数，但本次未确认该目标的释放语义。原始本机输出：`E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-dispatch\`（dispatch 表、机器码、反编译、目标 hash）；不入库。
- 本增量只收窄 manager 分支代码形状，不等于完整 callback ABI：当前 owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence、旧 `0xC0000005` 根因及 R3 强身份仍开放；R4 不准入，feat-005/006 继续 blocked，产品仍 NOT READY。

### 2026-10-13 R2 `73BD6C` 用户 invoker ABI 修正（离线）

- 固定磁盘样本 SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 Ghidra 导出 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\callback-contract\assembly\73bd6c.txt` 显示 `[RSI+0x30]` 调用前提供 RCX/RDX/R8/R9 四个寄存器参数和 `[RSP+0x20]` 第五参数；未设置第六参栈槽。固定样本合同为 `(inner, 0x10, errCode, errStr, responseUid)`。
- `consumer_invoke` 从 6 参改为 5 参；第五参保持 opaque、不解引用；移除把未传入第六参当 span 读取的 `capture_span_shape` 及误导性 `span_shape` 日志。离线单测用 5 参函数指针类型锁定声明，并以不可读第五参覆盖不读取合同。没有对 QQ/SnowLuma 做调用或发送。
- 此项只闭合固定-hash `73BD6C` 用户 invoker 参数个数与 Caligo 声明的一致性。第五参 UID 编码/与 request cookie 的关系、manager op 1/2 全调用者与语义、owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence 和历史 `0xC0000005` 仍开放；R3 stable native identity 缺口未变，R4 仍不准入。
- `cargo test -p caligo-bridge --lib --features research consumer_probe::tests --locked --offline --target-dir target-harness -- --test-threads=1` PASS（7/7）；`rustfmt --edition 2021 --config skip_children=true --check crates/caligo-bridge/src/consumer_probe.rs crates/caligo-bridge/src/g3.rs` PASS；`.\init.ps1 -Mode all` PASS，docs 9 live docs / 168 local links，普通与 research workspace 均完成。构建保留已有 warnings。
- 最终 `git diff --check` PASS。GitNexus 在脏工作树下已刷新到当前 base HEAD 并经 `status --json` 确认为 `up-to-date`/runner `current`/content `current`；post-commit detect gate 在本节收尾完成后核验。以上均为离线验证，不构成 QQ 现场或生产验收。

### 2026-10-13 R2 heap-fallback closure 分配/释放配对补充（离线）

- 再次计算冻结磁盘 `wrapper.node` SHA-256，仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Ghidra 12.1.4 `-noanalysis` 定向导出 `0x24C0`、`0x1FD0`、`0x1FE0`、`0xB75000`、`0xB75029`；结果位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\allocator-helper-analysis\followup-20261013\`，没有加载/调用 QQ。
- 与既有 `73BBF8`、`73C2AA` 反汇编合并看：`73BBF8` 的 heap fallback 分配 `24C0(0x40)`，而 `24C0` 跳到 `B75000`；heap manager op=2/3 清理 closure 后以 `(closure, 0x40)` 调 `1FD0`，其经 `1FE0` 跳到 `B75029`。这闭合固定 hash 中 heap-fallback closure 的同尺寸 QQ wrapper 分配/释放配对。
- 未将该结论扩展到 `4D3588` 的 arena 分支或 wrapper allocator 内部，也未推断 op=1/op=2 的完整调用者与外部合同、op=3 重复调用安全。owner/consumer thread、QQ stop/cancel/drain/quiescence、历史 `0xC0000005` 和 R3 stable identity 仍开放；R4 不准入，feat-005/006 与产品状态不变。没有 QQ 操作或 SnowLuma 改动。
- 文档门 `init.ps1 -Mode docs` PASS（9 live docs、168 local links、33 historical entries、10 features）；`git diff --check` PASS。GitNexus 1.6.12 增量重索引后 status `up-to-date`，runner/content current、incompleteReasons empty；索引日志仍提示 callable-flow candidates 超 cap 及流程截断，故不作“无调用”推断。
- `detect-changes --scope staged` 报 CRITICAL、449 affected processes；改动集严格是四份 Markdown，唯一 changed symbol 是执行记录中的 section heading，列出的环境启动/OneBot/native 流程不对应源码变更。已核查 staged path 清单与 diff；该影响面判为文档节点解析扩散，保留检测结果，不把它解释为 449 条运行调用受本次改动影响。
