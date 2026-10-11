# 会话交接

## Current Objective

- 日期：2026-10-12（Asia/Shanghai）；计划源基线 `0f5b483`，本次从 `57e7190` 继续。
- 用户请求执行 [N4 响应恢复计划](docs/plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md)。
- 当前 R6 已接入 HTTP POST event、正向 WS 与反向 WS；三者复用动作层/Daemon，HTTP POST quick operation 结束后再向 WS 扇出，caligod 等 POST 订阅就绪才启动 bridge 接收。离线协议合同通过；native receive projection 与真实应用矩阵尚未验收。此前 R1–R5 离线子块及详证见 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- 产品仍 **NOT READY**；feat-005/006 仍 `blocked`。R2/R3 native 与 identity 门未闭合，因此 R4 不准入；没有启动 QQ/注入/现场发送，也没有改变 SnowLuma 连接。

## Changed Areas

- Rust：bridge 回执期限与 unknown 诊断语义、response ledger timeout/tombstone 和迟到更正；OneBot HTTP/HTTP POST/正反向 WS transport、统一动作层、bounded event queues 与 POST→WS 顺序合同。
- 文档：执行记录、wire sequence [ADR](docs/decisions/2026-10-11-wire-sequence-authority.md)、progress/feature/index/architecture 状态。
- 测试 harness：`resident_lifecycle` 的资源快照从进程级并行污染源改为当前测试线程 opt-in 计数，L17 assertions 保留。
- 既有 README/AGENTS/guides/skills/verification harness 等未提交改动继续保留，勿回滚或重写。

## Verification

- `cargo test -p caligo-core --locked --offline --target-dir target-harness` 与 R6 transport 定向测试通过；本次完整文档/工作区验证结果见 progress 与 N4 执行记录。具体限制：只属离线测试，不替代 native/QQ 回执或真实 OneBot 应用验收。
- `cargo fmt --all` 后审阅并撤销了本范围外的纯格式化变更；保留修改仅涉及 R3 timeout/result path 与对应测试。
- 既有完整 R1/R2/R3 sequence/R5 离线证据仍见下方历史记录；不将本次验证写成当前 callback manager ABI、线程/排空或 R4 现场通过。

- 前一轮 `.\init.ps1 -Mode all` PASS，docs 9 live docs / 161 links；普通 workspace 192 passed、research workspace 226 passed；历史原始摘要 `target-harness/n4-r2-owned-join-all.log`。CPB identity follow-up 的最新完整结果见下一条。
- `consumer_probe::tests` 7/7 PASS；另有 owned-worker join helper 1/1 PASS。callback context 注册前通过 Win32 `GetModuleHandleExW(FROM_ADDRESS|PIN)` 固定 bridge 到进程结束；resident stop 隔离代次并等本地 in-flight callback 退出，随后有界 join Caligo 自有 worker。此结果不证明 QQ loaded-DLL 现场行为、QQ dispatcher 停止或原生 quiescence。
- `cargo test -p caligo-core --lib --test daemon_chain --locked --offline --target-dir target-harness` PASS：core lib 44/44、daemon_chain 5/5；慢头并发隔离、loopback bind、fake bridge 强回执→持久 message_id→get_msg 闭环均通过。测试中的 bridge projection 是合成输入，不证明 native receive 或真实 OneBot 客户端。
- 修正分配统计后 `cargo test -p caligo-bridge --test resident_lifecycle --locked --offline --target-dir target-harness`：18/18 PASS（默认并行）。
- 文档检查、Rust 格式检查与 `git diff --check` 均已通过，状态记录已同步最终验证结果。
- 34 个已跟踪 Rust 文件与 1 个新增 LAB 文件通过无子模块递归的 rustfmt 检查；最后格式化后重新跑了 full mode all。
- R5 状态文档更新后的 GitNexus 重建成功；最终 status 与 detect-changes 在最后复核后记录于忽略日志 `target-harness/n4-r5-final-index.log`、`target-harness/n4-r5-final-status.log`、`target-harness/n4-r5-final-detect.log`。status 须 up-to-date、runner identity/content drift current、无 incomplete reasons；detect risk 覆盖整棵既有 dirty N4/harness 工作树，不能当作本次 R5 单独风险。图有 callable-flow/REACHING_DEF/process truncation，缺失 edge/flow 不能证明无路径。
- CPB receive follow-up 已删除 `cpbseq:{seq}` 假 native ID。`RecordFields` 的 `native_id`/`sender_uin` 当前仍未映射；CPB 候选在 identity gate 处只审计，不进入业务事件流。定向 gate 1/1；`init.ps1 -Mode all` 为 docs 9 live docs / 161 links、default 191/191、research 227/227；随后 docs-only 检查为 9 live docs / 162 links。输出 `target-harness/n4-cpb-identity-gate-all.log`。不代表 CPB/OneBot receive 完成。
- R5 core/store 接线补充：数值 account 的 daemon 为 AccountActor 打开 sibling MessageStore；显式 projection 在 journal 后、ACK 前持久化并可由 journal 恢复，Daemon 暴露 StoreReader。`daemon_chain` pipe 集成覆盖直接 OneBot `get_msg` 完整响应。当前 `daemon_client` 仍上送 `None` projection。定向测试 1/1、rustfmt、JSON 与 diff 检查通过；本次 `.\init.ps1 -Mode all` PASS，docs 9/163 links 且默认/research workspace 均成功。详细边界见 execution R5 小节。
- 本轮只读复核固定 `wrapper.node`，当前磁盘 SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。已有静态报告仍缺 callback manager 全操作调用者/语义和 QQ stop/cancel/drain/quiescence；historical field/thread observation 不能证明当前会话身份，旧 `0xc0000005` 仍没有 dump/堆栈。未启动或操作 QQ/SnowLuma。
- 本机 local-only evidence 中有一份较早 QQ Crashpad dump，header exception code 为 `0xE06D7363`，与 research test 的 `0xC0000005` 不同且无关联证据；未读取 dump 其它内容，不作根因证据。旧 research AV 仍缺匹配 dump 与故障栈。
- 既往 `0xc0000005` research 失败现已定位到 `caligo_bridge-c00aae2a8f83540a.exe`，旧日志显示前两项 `asyncrun` 通过后进程退出，但无故障测试/线程/地址；本次 research bridge lib 串行与默认并行各 69/69 未复现。无 WER 事件或 dump，也无可用调试器堆栈，根因仍 open。所有结果均非 QQ 现场/真实应用验收。

## Blockers / Stop Conditions

- 固定-hash `1DB2(pair*)` 已确认并接入未提交 pair 清理；成功提交后的 service pair 继续 resident/quarantined。`732A80` body 深复制后由 `B75029` 释放输入，未使用的 self-string clone 已删除。bridge callback image 已在共用 `leak_ctx` 门中 PIN 到 QQ 进程退出；resident generation stop 时本地关闭 callback 准入并等在途 callback 退出。QQ manager 完整 copy/move/destroy、回调/owner thread、QQ dispatcher stop/cancel/drain/quiescence 与既往 AV 仍未证。
- CPB 相似消息没有强身份桥接，`stable_identity unsupported`；标准同步 send 不得返回 `ok`，AcceptedUnconfirmed/unknown 不重发。
- R5 core/store 已装配到 AccountActor 事件接收与 Daemon `StoreReader`；普通 caligod HTTP、HTTP POST event、正向/反向 WS 已接动作层，群 send 等强回执及持久消息 ID，私聊目标明确 unsupported。HTTP POST 快速操作结束后才向 WS 扇出；transport 队列有界。native bridge 当前 projection 是 `None`，实际 CPB 接收仍不产生该索引；真实应用互通、私聊映射与消息身份仍未完成。
- R4 只有在 R1–R3 全门完成、当次合同与新现场准入明确后才可开始；当前不满足。

## Next Restartable Steps

1. 读 [AGENTS](AGENTS.md)、[progress](progress.md)、本记录及上述执行记录；确认当前 status/diff，保留所有脏 harness 文件。
2. 从固定 `wrapper.node` hash 继续 callback manager op1/其它分支与 owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence 静态核验；bridge PIN 和本地 generation 隔离不代替这些门。如 AV 再现，保留独立进程 dump、故障地址及线程栈；本次串行/并行通过不关闭旧失败。
3. R3 强 native identity 仍 unsupported；重复/相同 CPB 行歧义 fixture 已确认不会消费 pending sends，缺稳定 ID/发送者映射的 CPB 记录现在不会伪造事件，不通过真实 QQ 实验绕过。
4. R2 callback manager/owner-thread/dispatcher quiescence 和历史 AV 根因继续未闭合；若 AV 再现，先保留 dump、异常地址和线程栈。
5. 从固定-hash CPB/native record 继续研究稳定 native ID 与 sender UID→UIN 的证据来源；当前 decoder 缺这两项，不能靠序号或文本补身份。
6. R6 HTTP POST event、正向 WS、反向 WS 的离线合同已实现并测试；剩余是 native projection/私聊身份、真实 OneBot 应用收发/get_msg/慢连接矩阵。先完成 R2/R3 准入缺口；不以合成投影替代现场数据，也不绕过 R4 前置条件。
7. `git diff --check`、文档检查和本次适用的离线验证；R2/R3 ABI/身份准入门完成前不开始 R4，不恢复旧 PID/旧 hook。
