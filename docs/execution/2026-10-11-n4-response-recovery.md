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

### 2026-10-13 N4-R2 callback-manager consumer callers follow-up（静态）

- 固定磁盘样本 `wrapper.node` 的 SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Ghidra 12.1.4 `-noanalysis` 对 `.pdata` 范围内的有限调用者做反汇编与反编译；本次没有启动、读取或操作 QQ，也没有改变 SnowLuma 连接。原始输出保存在 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\` 的 `manager-callers-followup-20261013\`、`caller-refs-20261013\`、`candidate-context-20261013\`、`candidate-context-followup-20261013\`。
- `0x54CE00` 对 16 字节 wrapper assignment 先对 destination 做 op=3，再从 source 做 op=1。`0x9B31C8` 先查 source callback presence，再经 helper 复制，最后对 source 做 op=3；`0x744D7E` 也在复制后释放 source；`0x31FA9D4` 复制两个相邻 wrapper。这为共享 callback-manager 接口提供具体的 op=1/op=3 consumer 语义证据。`0x3BC545C` 将 request object `+0xA0` 的候选 callback wrapper 经 op=1 复制到本地，再传给后续 callback/request helper；其具体 manager table 尚未确定。
- 排除的 lookalike 包括 `0x970E2A`、`0x982F4E`、`0x340CA75`（op 返回值形状和已知 void callback manager 不匹配）以及 `0x3225B8A`、`0x33FCBA4`（另一 `+0x68` property manager）。`0x34A156E` 的 0x20 步长数组 copy 暂留未分类。有限 `E8 rel32` 扫描的 42 个命中不是完整 xref；不能作无调用推断。
- 仓库侧 GitNexus 在专用 Docker 容器 `caligo-gitnexus` 以 CLI 1.6.12、Node 22.23.2/Linux x64 对 `/repo` 增量建索引，commit `4da7167966e8ea3c1ddf63c9d93951f305bd0429`。更新后的 `status --json` 为 `up-to-date`，runner/content current、无 incomplete reasons；索引 5,466 nodes、12,173 edges、149 clusters、459 flows，覆盖 272 文件。`context drain_responses --file crates/caligo-bridge/src/consumer_probe.rs` 列出 4 个测试调用与 `NativeHostAdapter.drain_send_responses` 一个生产侧直接调用，并已用 `rg` 对照源码。索引有 33 callable-flow 候选超 cap、336 个入口候选未入流程、902 个 callee 分支跳过及 14 个 walk 到预算；GitNexus 用于仓库调用链定位，不能补足二进制的间接引用，也不能以缺图证明无调用。索引存于容器 `/var/lib/gitnexus-caligo`，后续在该容器查询需传 `GITNEXUS_STORAGE_PATH=/var/lib/gitnexus-caligo`。
- 使用 Linux Docker 内的 descriptor-anchored provenance helper 复读计划并重算 schema-2 snapshot：计划 digest `sha256:7ffaf98b4b4a25f5083fa91feab3655a5dab84272dfc15a3529c467ade1dee3f`，39 个引用路径均存在；计划 HEAD `0f5b4834dc17534083a0c503b2a0223ab44e3451` / dirty digest `298284fa9c9bade8a54b6baccf9cb982f6f5d27d399b2cac701604f3d82daeff` 与重锚时 HEAD `4da7167966e8ea3c1ddf63c9d93951f305bd0429` / dirty digest `cd8fda719e873b6bd580aa8bf89130c6c7afee28e816dd0e6955951b7e6262e2` 不同，差异来自本计划已执行的提交。重锚时唯一额外未跟踪目录 `.agents/` 是用户 harness，已保留且不纳入本次提交；计划正文未改。
- 此静态增量只加强 op=1/op=3 用法，不闭合 op=1/op=2 全部调用点、manager-table 归属、arena 分支、线程、QQ dispatcher quiescence、历史 `0xC0000005` 或 R3 identity。feat-005/006 仍 blocked，产品 NOT READY；R4 不准入。按本轮限制没有运行 QQ 或 Cargo 测试。

### 2026-10-14 R2 manager table owner 与 op=2 调用筛查（静态）

- 再验固定安装磁盘 `wrapper.node` SHA-256：`63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本次只读 PE/`.pdata` 并用 Capstone 按精确函数范围检查，没有启动/读取 QQ 进程、调用原生函数或触碰 SnowLuma。可复现筛查器与文本输出在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-caller-screen-20261014\`。
- 有界直接调用筛查在目标集找到 41 个 `E8 rel32` 命中；`73BBF8` 的两个直接 caller 分别为 `0x732A80` 外部构造路径与 inline manager op=0 clone 路径 `0x73BC5C+0x81`。builder 的 arena 分支选择 inline manager/invoker，fallback 分支选择 heap manager/invoker 并走 0x40 wrapper 分配。它连接了表身份与 builder，不证明下游回调所有权或寿命。
- 全可执行节筛查 `mov edx,2` 有 5,496 个候选点/4,801 个 `.pdata` 函数；同基本块首个 `[reg+0x10]` 调用筛出 12 个形状候选，均不能绑定至已知 callback manager table，其中已知 lookalike 的返回字节形状与 manager 的 void op=2 不符。这是有界筛查结果，不是完整 xref，也不能推出没有 op=2 调用者。
- 仍开放：op=2 外部合同/全调用者、manager 外层 owner teardown、arena 分支寿命、callback consumer thread、QQ dispatcher stop/cancel/drain/quiescence、旧 AV 根因和 R3 强身份。R4 前置门不变；未做 QQ、Cargo 测试或消息发送。

### 2026-10-13 R2 指定 QQ 实例 dispatcher 外部只读复采

- 用户明确授权使用最新启动的指定测试 QQ 实例。通过进程创建时间、父进程关系、主窗口句柄与 CUA 返回窗口句柄交叉确认目标；对应 `wrapper.node` 已加载路径的磁盘 SHA-256 与冻结 profile 一致，磁盘文件字节数也匹配。QQ 窗口最小化；为避免读取聊天正文，没有激活窗口或打开任何会话。
- 运行四次 `caligo-cli observe-msf`（`--pid` 与完整报告仅写入本机证据），采样间隔分别 219 秒、183 秒、1274.1 秒。observer 源码只使用 `OpenProcess(QUERY_INFORMATION | VM_READ)` / `ReadProcessMemory`，不写目标进程、不注入、不调用 QQ 函数；四次命令均 exit 0 且目标进程代次未变化。第四次采样前复核 PID、进程创建时间、加载的 `wrapper.node` 路径和磁盘 SHA-256。
- 每次观测都报告 profile hash、MSFService vptr 锚点、有效 TLS index、145 个目标线程、41 个同 vptr 的 TLS dispatcher、2 个 manager chain visits；全局 dispatcher vptr 与静态 R3.1 类身份相符。四次全局 dispatcher 对象地址均命中某一线程 TLS dispatcher 项。Core executor 未构造/锚点未命中，符合 profile `not_in_play` 项。TLS 地址命中不证明该线程正在消费 callback。
- `+0x58` 四次保持相同的 DWORD 位型，均不属于报告中的目标线程。第四次前修正 observer 把该字段从 `i32` 改为 `u32`，重建 CLI 后再采样；当次另枚举 8,874 个可访问系统线程 ID，也无匹配。该系统视图限于可访问进程，字段与存活线程不匹配的事实不能单独解释其来源。没有 native invocation，所以没有 callback-enter/enqueue/drain 证据；owner/consumer 合同及 stop/cancel/drain/quiescence 门仍 open。
- 静态交叉检查定位一个 `+0x58` writer：`0x31F90B0` 将 `0x7879D0` 返回的当前 TID 写入 `arg+0x58`；已导出的调用者 `0x217D8CC` 在 worker context 初始化循环 `worker+0x48` 集合，对 `0x31F83DC` 返回的条目调用该 setter。dispatcher ctor `0x31F878C` 只写 vptr 和 `+0x08/+0x10` pair，slot0 inner enqueue `0x31F88EA` 读取并比较该字段。现场确认全局 dispatcher 指针也是一个线程 TLS 项，但尚未证明它是否为 worker `+0x48` 集合成员，也未定位线程退出清零/迁移；有限静态导出不代表全 binary xref。
- 对照历史 PID 47524 的原 P1 报告：global dispatcher `+0x58` 在 t1/t2 为零，t3 非零但不匹配报告内线程 TID。旧与新实例证据分别保留，不把它们拼成一条生命周期证明。
- 原始四份内存报告、CLI 原始日志、目标身份元数据和脱敏汇总只保存在 `local-evidence/n4-r2-field-20261013-224510/`。GUI state capture 显示窗口最小化；本轮未激活窗口、未打开聊天、未注入、未调用 native、未发送消息，也未操作 SnowLuma。`cargo build -p caligo-cli --locked --offline --target-dir target-harness` 通过，保留既有 warning；未运行 Cargo 测试。
- R2 manager 全 ABI/所有调用点、当前 callback consumer 与有效 owner、QQ dispatcher stop/cancel/drain/quiescence、历史 `0xC0000005` 根因仍 open；R3 stable identity 与 sender UID→UIN 未闭合。用户的实例使用授权不替代 N4 R4 的 R1–R3 完成门，故未进行发送。feat-005/006 继续 blocked，项目仍 NOT READY。

### 2026-10-14 R2 dispatcher owner 只读复核（T5）

- 在相同 QQ 进程代次和冻结 profile 上完成第五次 `observe-msf`，exit 0。SHA-256、加载路径和进程创建时间复核匹配；报告写入本机 local-evidence。窗口未激活、未打开聊天；没有注入、目标进程写入、QQ 函数调用、callback invocation 或消息发送，SnowLuma 未操作。
- T5 再次观察 145 条目标线程、41 个同 vptr TLS dispatcher；global dispatcher object 与一条 TLS dispatcher 项相同。五份原始报告的 `+0x58` DWORD 位型归一后相同，五次均不匹配目标线程 ID。T4 的 8,874 个可访问系统线程 ID 也不匹配；T5 未再次进行系统范围枚举。
- 固定 profile 的静态复核确认 `0x7879D0` IAT 指向 `KERNEL32!GetCurrentThreadId`。`.pdata` 起点解码覆盖 525,291 函数范围、18,889,360 条指令；`0x217D8CC` 以 `worker+0x2C` 为界，通过 `0x31F83DC` 从 `worker+0x48` 的指针-backed collection 取 16-byte `{object, control}` pair，并调用 `0x31F90B0` 写当前 TID。发送函数 `0xB7CE8A` 从 `0xB816E8` 静态 shared pair 的 `+8/+0x10` 取 object/control，增加 control 强引用后交给 `D32138`。
- 先前按内联数组计算出的 index-40/count-53 候选已作废：`0x31F83DC` 明确先从 collection descriptor 首 qword 取元素数组指针，再以 16-byte stride 访问。按该布局检查全部可读提交映射（2,274 regions、每轮约 901 MB）时出现 3 次局部读取不足；没有找到符合固定 worker-context 双 vptr、vector element、global object 与相同 control block 的精确成员候选。collection membership 仍 unknown，有限可读扫描不构成全内存否定证明。
- 这次证据只确认 global dispatcher 同时持有于一条 TLS 项；它是否属于 `worker+0x48` collection、该 TLS 项是否是 callback consumer、线程退出清理/迁移和 dispatcher stop/cancel/drain/quiescence 均未闭合。没有 native invocation，所以没有 callback-enter/enqueue/drain 证据。
- 原始报告、地址和线程数据保存在本机 `local-evidence/n4-r2-field-20261014-000619/`，脱敏摘要为 `summary-t5-redacted.json`；新增 manager RTTI/caller 结果见同目录 `summary-r2-manager-static-redacted.json`。T1–T4 原始报告仍在 `local-evidence/n4-r2-field-20261013-224510/`。详细线程合同见 [qq-native-thread-contract §12](../research/qq-native-thread-contract.md#12-2026-10-14-r2-global-dispatcher-owner-只读复核)。
- R2 manager ABI / owner-consumer / teardown / quiescence 与历史 AV 根因仍 open；R3 stable identity 与 sender UID→UIN 未闭合。R4 前置门仍不满足，未发送消息；feat-005/006 保持 blocked，产品 NOT READY。
- 另补固定-hash RTTI/vtable 交叉核验：`.rdata` RVA `0x4260448` 的 `ThreadTaskManagerImpl` vtable 含 `0x217DD98` 析构项；其链 `0x217DD98 → 0x217D4FA → 0x217D556 → 0x31F8372 → 0x14830` 逐项释放 shared pair 并释放 vector buffer。仅证明该类有清理实现，不证明 owner 调用/析构注册、线程退出触发、stop 顺序或 drain/quiescence。
- `.pdata` xref 确认构造函数 `0x217D242` 的唯一直接调用者是 factory `0x217D16C`；它分配 0xF0 字节 `ThreadTaskManagerImpl` 并建 shared holder。`0xD31686` 把 holder 放入自身 `+0x110` 后调用 `0x217D58C`；全局发送初始化近邻 `0xB81758` 是 `0xD31686` 的多个直接调用点之一。尚未找到这些外层对象的析构路径或 QQ callback drain。
- 其后对 `0x217D58C` 的定向反汇编显示：该启动方法按 manager 配置数构造 task 并填入 `ThreadTaskManagerImpl+0x48`；`0x217D8CC` 遍历同一 task vector 设置每个 dispatcher 的 TID，析构路径逐项 release 并释放 vector buffer。此前 T5 记录的 `worker+offset` 是暂定反汇编变量名，现按 RTTI class 修订为 `ThreadTaskManagerImpl this+offset`。此补充闭合该类内部 task vector 的构造/遍历/清理静态合同，但仍不证明 global dispatcher 是成员、外层 owner 会何时析构或 callback 已 drain。

### 2026-10-14 R2 manager op=4 与 op=2 调用位置扩展（静态）

- 复核冻结磁盘 `wrapper.node` 的 SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。相对跳表 `0x3F7052C`/`0x3F70540` 相距 `0x14`；两者 `+0x10` 的 op=4 分支分别到 `0x73BCB0`/`0x73C309`，都清零第五参数指向的 qword。此处确认了已知两表的 op=4 实现；不是 callback presence 语义证明，也不允许调用 op=5 或其它越界值。
- 对全部 `.pdata` 函数扩展 exact `mov edx,2` 筛选，按简单寄存器来源传播验证 `RCX` 与间接函数指针槽地址一致、`R9D=0x10`，并在控制转移/写覆盖前未观察返回值。找到 9 个 manager-ABI-shaped op=2 调用点，位于 7 个函数：`0x9C50FF`、`0x9C511A`、`0x9C5135`、`0x9E13A7`、`0x3456CE6`、`0x34576E5`、`0x35140D5`、`0x3B36599`、`0x3DD3ECD`。`0x35140D5` 在同一成员上先调用 `0x3FCE5E`；`0x9E13A7` 经 `0x3FBAB2 → 0x3FCE5E` 检查同一 manager slot 后，在条件分支执行 op=2。结果支持继续按具体地址分析 op=2，而非断言所有调用目标的动态 manager table 身份。
- 勘误 §7.4 对 `0x3FCE5E` 的“presence helper”称呼：该函数只说明 op=4 的第五参数输出是否非零；已知两张 manager 表的 op=4 都将输出清零，所以语义名称过强。现称“manager op=4 输出查询 helper”；是否表达 callback 是否存在仍未证明。
- 筛查器与完整地址级结果保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\`，仅读固定磁盘样本；未访问 QQ 进程、未调用 native 函数、未发送消息、未操作 SnowLuma。筛选不覆盖其它 immediate 编码、间接数据流、分支合流或动态 manager slot 值，不代表完整 xref。
- R2 owner/consumer thread、manager 外层 teardown、QQ dispatcher stop/cancel/drain/quiescence 与旧 `0xC0000005` 根因仍开放；R3 stable identity 与 sender UID→UIN 未闭合。R4 前置门不变，产品仍 NOT READY；本轮不运行 Cargo 测试。

### 2026-10-14 R2 op=2 候选 RTTI/data-reference 复核（静态）

- 固定 `wrapper.node` hash 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。全 `.rdata` 合法 COL/vtable 扫描并通过已知 `ThreadTaskManagerImpl` RTTI 正向对照后，确认 `0x9C509C` 是 `FileDragManagerWin` vtable slot 5，`0x345763E` 是 `IgnoreBodyHttpRequestHandler` slot 3，`0x3456C1C` 是 `SaveToStringHttpRequestHandler` slot 3。三个原始 qword 都是 vtable 中间的函数指针；前置 qword 是上一 slot，不是 COL。对应 vptr 写入点分别为 `0x9C47ED`、`0x3457484`、`0x34569AD`。
- `0x345745A` 还提供一处具体 op=1-shaped 初始化：从输入对象 `+0x10` 取动态目标，传 `EDX=1`、`R9=0x10`，目标 pair 地址为 `this+0x20`。`0x3456A0A` 以 op=0 调 manager 并写入相邻 pair/invoker 字段。此处确定 enclosing handler class 与其 vptr 写入，不证明上述动态 manager 等于 `0x73BC5C/0x73C2AA`，也未将 `0x345763E/0x3456C1C` 所取 nested object 的 pair owner 绑定到 handler 字段。
- 可复现脚本/输出：本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\r2-op2-vtable-membership-20261014.py/.txt` 和 `r2-op2-owner-method-context-20261014.py/.txt`。COL 与 RIP-relative xref 扫描有解析边界，不是全 binary xref。动态 manager table 值、op=2 外部合同/全调用者、`0x3BC545C` 字段 owner、外层 teardown、callback/dispatcher stop-cancel-drain-quiescence、历史 AV 与 R3 stable identity 仍 open；未启动/操作 QQ 或 SnowLuma、未跑 Cargo 测试，R4 准入与产品状态不变。
