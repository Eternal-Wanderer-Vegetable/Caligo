# 会话交接

## Current Objective

- 日期：2026-10-14（Asia/Shanghai）；计划证据源基线 `0f5b483`，当前后续切片 HEAD 以 `git log` 为准；T5/observer 修正已提交。用户 `.agents/` harness 保留为未跟踪工作，现场原始只读证据在 local-evidence。恢复时重读 HEAD/status 与 Docker GitNexus index。
- 用户请求执行 [N4 响应恢复计划](docs/plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md)。
- 当前 R6 已接入 HTTP POST event、正向 WS 与反向 WS；三者复用动作层/Daemon，HTTP POST quick operation 结束后再向 WS 扇出，caligod 等 POST 订阅就绪才启动 bridge 接收。离线协议合同通过；native receive projection 与真实应用矩阵尚未验收。此前 R1–R5 离线子块及详证见 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- 产品仍 **NOT READY**；feat-005/006 仍 `blocked`。用户已授权指定测试 QQ 实例；T1–T5 仅做固定 hash 外部只读 `observe-msf` 和受限内存扫描，没有注入、写入目标进程、调用 QQ 函数、callback invocation 或发送消息，也没有操作 SnowLuma。R2/R3 native 与 identity 门仍未闭合，R4 发信不准入。
- T1–T4 观测相隔 219 秒、183 秒、1274.1 秒，均确认同一目标进程代次、profile hash 与 MSFService/global-dispatcher 锚点匹配；每次 145 条线程、41 个已安装 TLS dispatcher、manager 链 2 次 visit。global dispatcher 四次都与一个线程 TLS 项相同；`field_58_tid` DWORD 位型一致但不匹配目标线程，T4 对 8,874 个可访问系统线程 ID 枚举也无匹配。T5 在 2026-10-14 同一进程代次/profile 再次观测到 global dispatcher 命中 TLS 项且 `+0x58` 位型相同、不匹配目标线程。静态 RTTI 已将 `worker+offset` 暂定名修正为 `ThreadTaskManagerImpl this+offset`：`0x217D58C` 填充 `this+0x48` task vector，`0x217D8CC` 遍历并写当前 TID；析构清理该 vector。尚未证明 global dispatcher 是否属于 `ThreadTaskManagerImpl+0x48`、TLS 项是否是 callback consumer，或退出清理/迁移与 drain 路径。T1–T4 原始报告与日志在 `local-evidence/n4-r2-field-20261013-224510/`，T5 原始材料在 `local-evidence/n4-r2-field-20261014-000619/`。GUI 最小化；未激活或读取聊天内容。详情见 [thread contract §11](docs/research/qq-native-thread-contract.md#11-2026-10-13-同一实例的外部只读dispatcher复采)、[thread contract §12](docs/research/qq-native-thread-contract.md#12-2026-10-14-r2-global-dispatcher-owner-只读复核) 与 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。
- 固定磁盘 hash 的 manager op 0–4 静态映射、`73BD6C` 用户 invoker 五参 ABI、heap-fallback 0x40 分配/释放配对、`0x54CE00` op=1/op=3 consumer 用法及 2026-10-14 的 op=2 ABI-shaped callers/op=4 输出清零复核见 [寿命合同](docs/research/qq-native-lifetime-contract.md) 与 [执行记录](docs/execution/2026-10-11-n4-response-recovery.md)。本轮未启动/读取 QQ、未调用原生函数、未发送消息、未操作 SnowLuma。GitNexus 1.6.12 已在 `caligo-gitnexus` Docker 容器索引 `/repo`。

## Changed Areas

- Rust：bridge 回执期限与 unknown 诊断语义、response ledger timeout/tombstone 和迟到更正；OneBot HTTP/HTTP POST/正反向 WS transport、统一动作层、bounded event queues 与 POST→WS 顺序合同；`caligo-cli observe_msf` 的 `field_58_tid` 改按 unsigned DWORD 输出。
- 文档：执行记录、固定 hash callback manager 静态研究增量、wire sequence [ADR](docs/decisions/2026-10-11-wire-sequence-authority.md)、progress/feature/index/architecture 状态。
- 测试 harness：`resident_lifecycle` 的资源快照从进程级并行污染源改为当前测试线程 opt-in 计数，L17 assertions 保留。
- 既有 README/AGENTS/guides/skills/verification harness 等未提交改动继续保留，勿回滚或重写。

## Verification

- 本轮离线文档/静态筛查：`init.ps1 -Mode docs` PASS（9 live docs / 178 local links / 33 historical entries / 10 features），`git diff --check` exit 0（仅 LF→CRLF 提示），Docker GitNexus staged gate 为 4 files / 14 section symbols / 0 affected processes / low risk；没有运行 Cargo 测试。
- `cargo test -p caligo-core --locked --offline --target-dir target-harness` 与 R6 transport 定向测试通过；本次完整文档/工作区验证结果见 progress 与 N4 执行记录。具体限制：只属离线测试，不替代 native/QQ 回执或真实 OneBot 应用验收。
- `cargo fmt --all` 后审阅并撤销了本范围外的纯格式化变更；保留修改仅涉及 R3 timeout/result path 与对应测试。
- 本轮 `cargo build -p caligo-cli --locked --offline --target-dir target-harness` 通过，observer unsigned 字段修改已加载到新构建并完成 T4 只读采样；存在仓库既有 warnings，未运行 Cargo 测试。
- 本轮提交前复验 `init.ps1 -Mode docs` PASS（9 live docs / 174 local links / 33 historical entries / 10 features）；`git diff --check` PASS，Git 仅提示现有 CRLF 规范化。Docker GitNexus 1.6.12 / Node 22.23.2 对 `/repo` 强制分析后为 18,386 nodes / 49,242 edges / 148 clusters / 459 flows，覆盖 272 files；提交前 `detect-changes --scope staged` 为 5 files / 31 symbols / 2 flows、medium risk。提交后已重建索引并确认 `status --json` up-to-date、runner/content current、无 incomplete reasons，final `detect-changes --scope all` 为 No changes detected；分析仍有流程、分支和 PDG 截断警告，缺图不证明无路径。
- Linux Docker descriptor helper 重读计划并核对 39 个 cited paths 的 HEAD/index/worktree/untracked digest；计划 digest `sha256:7ffaf98b4b4a25f5083fa91feab3655a5dab84272dfc15a3529c467ade1dee3f`，重锚时 HEAD 为 `1b7f6147a7ee213ec070f7b795502a2e9eddab69`。已落地 R1–R6 差异按引用路径复核；`.agents/` 是保留的用户 harness。之后追加的 T5 RTTI/owner-call 文档在 progress 与执行记录中。
- 既有完整 R1/R2/R3 sequence/R5 离线证据仍见下方历史记录；不将本次验证写成当前 callback manager ABI、线程/排空或 R4 现场通过。

- 前一轮 `.\init.ps1 -Mode all` PASS，docs 9 live docs / 161 links；普通 workspace 192 passed、research workspace 226 passed；历史原始摘要 `target-harness/n4-r2-owned-join-all.log`。CPB identity follow-up 的最新完整结果见下一条。
- `consumer_probe::tests` 7/7 PASS；另有 owned-worker join helper 1/1 PASS。callback context 注册前通过 Win32 `GetModuleHandleExW(FROM_ADDRESS|PIN)` 固定 bridge 到进程结束；resident stop 隔离代次并等本地 in-flight callback 退出，随后有界 join Caligo 自有 worker。此结果不证明 QQ loaded-DLL 现场行为、QQ dispatcher 停止或原生 quiescence。
- `cargo test -p caligo-core --lib --test daemon_chain --locked --offline --target-dir target-harness` PASS：core lib 44/44、daemon_chain 5/5；慢头并发隔离、loopback bind、fake bridge 强回执→持久 message_id→get_msg 闭环均通过。测试中的 bridge projection 是合成输入，不证明 native receive 或真实 OneBot 客户端。
- 修正分配统计后 `cargo test -p caligo-bridge --test resident_lifecycle --locked --offline --target-dir target-harness`：18/18 PASS（默认并行）。
- 本轮 R2 静态 Ghidra `-noanalysis` 定向导出 hash 匹配、5 个种子函数 5/5 反编译成功、两张 manager 跳转表 8/8 入口已映射；不证明真实 manager 调用合同、QQ 线程或停止排空。本轮 `init.ps1 -Mode docs` PASS（9 份维护文档、167 个本地链接、33 个历史入口、10 个 feature），`git diff --check` 与 JSON 解析 PASS。
- 文档检查、Rust 格式检查与 `git diff --check` 均已通过，状态记录已同步最终验证结果。
- 34 个已跟踪 Rust 文件与 1 个新增 LAB 文件通过无子模块递归的 rustfmt 检查；最后格式化后重新跑了 full mode all。
- R5 状态文档更新后的 GitNexus 重建成功；最终 status 与 detect-changes 在最后复核后记录于忽略日志 `target-harness/n4-r5-final-index.log`、`target-harness/n4-r5-final-status.log`、`target-harness/n4-r5-final-detect.log`。status 须 up-to-date、runner identity/content drift current、无 incomplete reasons；detect risk 覆盖整棵既有 dirty N4/harness 工作树，不能当作本次 R5 单独风险。图有 callable-flow/REACHING_DEF/process truncation，缺失 edge/flow 不能证明无路径。
- CPB receive follow-up 已删除 `cpbseq:{seq}` 假 native ID。`RecordFields` 的 `native_id`/`sender_uin` 当前仍未映射；CPB 候选在 identity gate 处只审计，不进入业务事件流。定向 gate 1/1；`init.ps1 -Mode all` 为 docs 9 live docs / 161 links、default 191/191、research 227/227；随后 docs-only 检查为 9 live docs / 162 links。输出 `target-harness/n4-cpb-identity-gate-all.log`。不代表 CPB/OneBot receive 完成。
- R5 core/store 接线补充：数值 account 的 daemon 为 AccountActor 打开 sibling MessageStore；显式 projection 在 journal 后、ACK 前持久化并可由 journal 恢复，Daemon 暴露 StoreReader。`daemon_chain` pipe 集成覆盖直接 OneBot `get_msg` 完整响应。当前 `daemon_client` 仍上送 `None` projection。定向测试 1/1、rustfmt、JSON 与 diff 检查通过；本次 `.\init.ps1 -Mode all` PASS，docs 9/163 links 且默认/research workspace 均成功。详细边界见 execution R5 小节。
- R2 manager 的 0–3 双表分支现有固定-hash 静态导出；op=1/op=2 的调用合同/完整调用者与 QQ stop/cancel/drain/quiescence 仍未证。historical field/thread observation 不能证明当前会话身份，旧 `0xc0000005` 仍没有匹配 dump/堆栈。未启动或操作 QQ/SnowLuma。
- `consumer_invoke` 已按固定样本 `73BD6C` 改为 5 参数；第五参 opaque、不读取。尚未在 QQ 实例加载本轮 bridge 或发送消息，需独立完成下列准入合同后再评估现场样本。
- 本轮 `consumer_probe::tests` 7/7 PASS；`init.ps1 -Mode all` PASS（docs 9 / 168 links，default 与 research workspace 完成），定向 rustfmt 与 `git diff --check` PASS。构建含已有 warnings；GitNexus 1.6.12 脏树重建后显示 up-to-date、runner/content current、无 incomplete reasons。post-commit final detect 在最终答复记录。
- 本机 local-only evidence 中有一份较早 QQ Crashpad dump，header exception code 为 `0xE06D7363`，与 research test 的 `0xC0000005` 不同且无关联证据；未读取 dump 其它内容，不作根因证据。旧 research AV 仍缺匹配 dump 与故障栈。
- 既往 `0xc0000005` research 失败现已定位到 `caligo_bridge-c00aae2a8f83540a.exe`，旧日志显示前两项 `asyncrun` 通过后进程退出，但无故障测试/线程/地址；本次 research bridge lib 串行与默认并行各 69/69 未复现。无 WER 事件或 dump，也无可用调试器堆栈，根因仍 open。所有结果均非 QQ 现场/真实应用验收。

## Blockers / Stop Conditions

- 固定-hash `1DB2(pair*)` 已确认并接入未提交 pair 清理；成功提交后的 service pair 继续 resident/quarantined。`732A80` body 深复制后由 `B75029` 释放输入，未使用的 self-string clone 已删除。bridge callback image 已在共用 `leak_ctx` 门中 PIN 到 QQ 进程退出；resident generation stop 时本地关闭 callback 准入并等在途 callback 退出。T1–T5 固定-profile 外部只读观察确认 global dispatcher object 出现在一条 TLS 项；其 `+0x58` 是 `GetCurrentThreadId` 值且五次都不匹配当前目标线程。按指针-backed vector 正确布局扫描后仍无与已知 worker-context 双 vptr / 同一 object-control pair 相符的成员候选。RTTI 将析构虚表项归属为 `ThreadTaskManagerImpl`；唯一直接构造调用链为 `0xD31686 → 0x217D16C → 0x217D242`，holder 安装在外层对象 `+0x110`，但外层销毁者/析构触发仍未知。实际 callback consumer/owner、QQ dispatcher stop/cancel/drain/quiescence、完整 manager ABI 与既往 AV 仍未证。
- CPB 相似消息没有强身份桥接，`stable_identity unsupported`；标准同步 send 不得返回 `ok`，AcceptedUnconfirmed/unknown 不重发。
- R5 core/store 已装配到 AccountActor 事件接收与 Daemon `StoreReader`；普通 caligod HTTP、HTTP POST event、正向/反向 WS 已接动作层，群 send 等强回执及持久消息 ID，私聊目标明确 unsupported。HTTP POST 快速操作结束后才向 WS 扇出；transport 队列有界。native bridge 当前 projection 是 `None`，实际 CPB 接收仍不产生该索引；真实应用互通、私聊映射与消息身份仍未完成。
- R4 只有在 R1–R3 全门完成、当次合同与新现场准入明确后才可开始；当前不满足。

## Next Restartable Steps

1. 读 [AGENTS](AGENTS.md)、[progress](progress.md)、本记录及上述执行记录；确认当前 status/diff，保留所有脏 harness 文件。
2. 继续固定 `wrapper.node` 的 R2 静态调用者/lifetime 调查；`0x31F90B0` 是 `+0x58` 当前 TID writer，`0x217D8CC` 经 `0x31F83DC` 从 `ThreadTaskManagerImpl+0x48` pointer-backed task vector 取 pair。T1–T5 确认 global dispatcher object 同时存在于一条 TLS 项，但 T5 对全部可读映射的 vector-owner 复核未找到满足已知双 vptr 与同一 object/control pair 的成员候选；membership 保持 unknown，不把有限扫描作为否定证明。已确认 `ThreadTaskManagerImpl` 析构会清理该 shared-pair vector；静态调用链为 `0xB81758 → 0xD31686 → 0x217D16C`，`0xD31686` 将 manager holder 装入外层 `+0x110` 后调用 `0x217D58C`；尚未定位外层析构/退出调用。`+0x58` 与真实 `GetCurrentThreadId` IAT 对应，五次均无当前目标线程匹配。最新 op=2 ABI-shaped 调用点与本机脚本见寿命合同 §7.6：9 个同-slot/size 形状调用分布于 7 个函数，两个还对同一 slot 调 op=4 helper；仍需解析动态 manager-table 值、外层析构 owner、op=1/op=2 剩余 ABI 和 `0x3BC545C` 字段 owner。`0x3FCE5E` 的 callback-presence 命名已校正为 op=4 output-query helper，不推断语义。consumer 线程与 stop/cancel/drain/quiescence 仍未证。T5 与 RTTI/vtable 证据写入 thread contract §12。用户授权了指定 QQ 实例，但 R1–R3 门未满足，不能开始 R4 native send。
3. R3 强 native identity 仍 unsupported；重复/相同 CPB 行歧义 fixture 已确认不会消费 pending sends，缺稳定 ID/发送者映射的 CPB 记录现在不会伪造事件，不通过真实 QQ 实验绕过。
4. R2 callback manager/owner-thread/dispatcher quiescence 和历史 AV 根因继续未闭合；若 AV 再现，先保留 dump、异常地址和线程栈。
5. 从固定-hash CPB/native record 继续研究稳定 native ID 与 sender UID→UIN 的证据来源；当前 decoder 缺这两项，不能靠序号或文本补身份。
6. R6 HTTP POST event、正向 WS、反向 WS 的离线合同已实现并测试；剩余是 native projection/私聊身份、真实 OneBot 应用收发/get_msg/慢连接矩阵。先完成 R2/R3 准入缺口；不以合成投影替代现场数据，也不绕过 R4 前置条件。
7. `git diff --check`、`init.ps1 -Mode docs` 与 Docker GitNexus current-index 检查；R2/R3 ABI/身份准入门完成前不开始 R4，不恢复旧 PID/旧 hook。
