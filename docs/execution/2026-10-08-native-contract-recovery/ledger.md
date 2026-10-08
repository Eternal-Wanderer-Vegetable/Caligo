# 执行台账：原生契约、可靠链路与 OneBot MVP（P0–P9）

> 计划：`docs/plans/2026-10-08-gitnexus-plan-caligo-native-contract-recovery.md`
> 本台账只记录执行事实与证据归属；每条 `[pass]`/`[fail]`/`[blocked]` 附证据路径。不把静态/LAB 结果写成现场通过。

## P0 — 执行基线（2026-10-08）

| 检查项 | 期望 | 实际 | 结论 |
|---|---|---|---|
| HEAD | `681434bb8c0a32fd4e98ddaceadedf7e48f95782` | 同左 | [pass] |
| dirty 状态 | 计划 §11 manifest：2026-10-07 计划 unstaged 修改；新计划与两轮 R 报告 untracked | `M docs/plans/2026-10-07-...k4-runtime-recovery.md`；untracked：`docs/plans/2026-10-08-...native-contract-recovery.md`、`docs/research/2026-10-08-qq-native-contract-and-version-adaptation.md`、`docs/research/2026-10-08-snowluma-native-workflow-reconstruction.md` | [pass]（与 manifest 一致） |
| wrapper.node SHA256 | `63112ab9161e127f5f7e17998a7196e143808923fb54cbbf7b4e21426187a5f0` | 同左（`D:/Program Files/Tencent/QQNT/versions/9.9.33-52230/resources/app/wrapper.node`） | [pass] |
| R1 evidence | `E:/stella/_reference/snowluma-native-r1-20261008/artifact-manifest.json` sha256 `82d9b73e…` | 目录存在；含 projects/、decompiled-hook/、decompiled-node/、resolver anchors | [pass]（manifest 摘要未逐字节重算，见下） |
| R2 evidence | `E:/stella/_reference/qq-native-r2-20261008/artifact-manifest.json` sha256 `f22f3d18…` | 目录存在；含 phase1–5、projects/QqWrapper52230.gpr、reports/、validation.json | [pass]（同上） |
| Ghidra | 12.1.4 已验证可用（R1 记录） | `E:/stella/_reference/tools/ghidra_12.1.4_PUBLIC/`；保存项目 `qq-native-r2-20261008/projects/QqWrapper52230.gpr` | [pass] |
| 现场 PID | 不指定任何 PID | 未指定；P6/P7 由执行者另行登记 | [pass]（按计划保持） |

原始输出：`evidence/p0-baseline.txt`。

声明与限制：
- 本步为磁盘/被动身份检查；未执行 QQ、未加载目标模块、未改动业务源码。
- 计划 §11 全部 8 个仓库外证据摘要（R1 manifest、R2 manifest/validation/comparison、4 份 R2 报告）已逐字节重算并全部一致，见 `evidence/p0-baseline.txt`。
- P0 通过门：文件与现有 hash 一致、证据归属明确 → **本轮不触发 P2 新 build 调查**。

## 门状态总览

| 门 | 状态 | 证据 |
|---|---|---|
| P0 基线 | PASS | 本文件 P0 表 |
| P1 原生合同（G-ABI/G-THREAD/G-LIFE） | **IN PROGRESS（首轮有界交付完成；关键间接目标未闭合 → 走计划失败分支的受控观测方案）** | `docs/research/qq-native-thread-contract.md`、`docs/research/qq-native-lifetime-contract.md`；外部证据 `E:/stella/_reference/qq-native-r3-p1-20261008/`（phaseA–G 共 165 函数 + 3 个字节级扫描脚本） |
| P2 路线冻结 | NOT STARTED | 依赖 P1 闭合项 |
| P3 公共 IPC/身份/恢复（LAB） | **PASS（LAB 层）** | 本文件 P3 表；`evidence/p3-lab/` |
| P4–P9 | NOT STARTED | — |

P1 首轮交付的实质进展（细节见两份合同文档）：
1. `[corrected]` D32138 = 通用任务调用原语（slot0 4 参 vcall），非"执行器派发"——R2 语义更正（汇编凭据）。
2. `[verified]` TLS 提交目标拓扑：index 单例 `6753454`、安装器 `D32E5A`（强引用获取）、安装链 `217D8CC→217DA24`（worker 上下文 +0x30 pair）；访问族封闭（D32DAD 仅 3 个直接调用方）。
3. `[verified]` 控制块 0x28 布局、强/弱双计数、五类获取位点、析构尾"弱归零→控制块 slot3 自删"、四类删除析构包装同构。
4. `[verified]` `nt::login::LoginRequestImpl` RTTI 解码、三 vptr、弱 pair 观察者接口形状（slot1 成功记录/slot2 状态码）。
5. `[unknown]`（按计划失败分支处理）：`732A80` 的 `this+0x60` 安装来源（候选淘汰表 5 组全空，含 dword 误读实锤）；TLS dispatcher 的类别与 slot0 体；stop/cancel/drain；观察者注册方。→ 需要执行者指定测试实例做受控观测（一次一未知）。

## 变更登记（实现产物按步骤追加，不覆盖历史）

| 日期 | 阶段 | 变更 | 提交/证据 |
|---|---|---|---|
| 2026-10-08 | P0 | 新建本台账与 evidence 目录 | 本文件、`evidence/p0-baseline.txt` |
| 2026-10-08 | P3 | C2/C3 帧:daemon `FrameStream`、worker `FramePipe`（增量 decoder + 完整帧 FIFO;TimedOut 保留半帧前缀;单读合并按序全交付）;删除 worker 重复重放块（此前每轮双发未确认窗口） | 见 P3 提交；`evidence/p3-lab/` |
| 2026-10-08 | P3 | C4:worker 未确认事件窗口（内容+字节预算）从 session_loop 上移 run_worker,跨连接持有 | 同上 |
| 2026-10-08 | P3 | C5:Dispatch 先提交 resident,受理（claim 进入）后才上报 NativeStarted;pending 补交受理时补报 | 同上 |
| 2026-10-08 | P3 | C1:daemon 每条新 bridge 连接 `fetch_add` 分配 epoch;reconnect 错误传播,失败即拒绝（accepted 只在完整绑定/恢复成功后返回） | 同上 |
| 2026-10-08 | P3 | C9:新增 `caligo-model::ipc_v3`（Hello + host_nonce + host_process_created_utc;v2 原样保留且被显式拒绝）;宿主身份由 bridge 提供（进程 nonce + GetProcessTimes + OS PID）,daemon 同代次+同宿主才允许幂等重连 | 同上 |
| 2026-10-08 | P3 | 测试基础设施:daemon LAB 故障注入 `test_fault_break_bridge_after_hello`（一次性;不读在途字节直接断开,T07 确定性构造点） | 同上 |
| 2026-10-08 | P1 | 两份 R3 静态合同（thread/lifetime）+ 外部证据树 phaseA–G(165 函数)+3 个字节级扫描脚本;D32138 语义更正;TLS 提交目标拓扑定位;+0x60 候选淘汰表;LoginRequestImpl 观察者形状 | `qq-native-thread-contract.md`、`qq-native-lifetime-contract.md`、`E:/stella/_reference/qq-native-r3-p1-20261008/` |
| 2026-10-08 | P1 伴生 | **发现并修复 D9 遗留缺陷**:`qq_entry::bootstrap` 把局部 UTF-8 String 指针登记为全局报告路径,悬空后被按 UTF-16 读取,LAB 运行在 CWD 产生乱码名诊断转储（10 个文件入库混入 P3 提交,已清理）;修复为进程生存期宽字符副本 | 提交 910dc5b |

### P3 反例与验证（LAB,2026-10-08）

| ID | 场景 | 修复前（红） | 修复后（绿） |
|---|---|---|---|
| T02 | 真实 daemon:Health 帧 header 后被 recv_wake 打断,补齐 payload | **红（实测）**:daemon 把已消费 header 丢弃,补齐字节被当新帧头 → `magic 0x2274227b`（`{"t"`) → 会话错位死亡,HealthAck 永不到达（`daemon_chain::t02_mid_frame_wake_preserves_stream_prefix` 失败,见 `baseline-before.txt`） | 绿（同测试通过） |
| T01 | 单次读合并 HelloAck+Dispatch+EventAck | 红（分析实证:修复前 `out.pop()` 只返回最后一帧,前两帧静默丢弃 —— daemon_client.rs 原 398–421） | 绿（`daemon_client::tests::coalesced_frames_delivered_in_order_once` + 半帧/魔数单测,真实 FrameDecoder 路径） |
| T07 | daemon 故障钩子断开（不读在途字节）→ 断开前未确认事件 | **红（实测）**:stash 修复后 T07 失败（8.09s 超时,事件双侧丢失） | 绿（1.01s,事件经重连重放对账落地恰一次;`t07_unacked_event_replays_after_break_before_consume`） |
| C5 | resident 未就绪时 Dispatch 的上报边界 | 红（分析实证:原 738–758 先报 NativeStarted 再提交,未执行项被记成已开始） | 绿（`native_started_only_after_resident_accepts`:未就绪期查询停留 DispatchIntent,bootstrap 后补交→started→ConfirmedSuccess） |
| T03 | 故障断开后同宿主重连 | **红（实测）**:回退 epoch/reconnect 两处语义后 T03 失败（epoch 不推进,actor 停 Degraded,SendText 被拒） | 绿（`reconnect_requires_same_host_and_advances_epoch`:epoch≥2 且 SendText 受理） |
| T05 | 同账号同代次不同宿主 nonce | 红（结构上不可表达:原 Hello 无宿主身份字段;`ipc_v3::tests::v2_hello_without_host_identity_fails_to_decode` 证明 v2 消息不再被放行） | 绿（不同宿主被拒;同宿主重连受理且业务可继续） |
| C4 | 断开前 unacked 内容 | 红（分析实证:原 session_loop:561 每连接重建窗口,seq/ACK 游标却跨连接;另发现重放块重复两份=每轮双发） | 绿（T07 覆盖;窗口随 run_worker 持有） |

### P3 验证命令结果（after-fix,2026-10-08）

- `cargo test --workspace --locked --offline` / `cargo test -p caligo-core --features research --locked --offline` / `cargo test -p caligo-bridge --features research --locked --offline`:**175 passed / 0 failed**（含 5 个新测试目标场景:T02、T01×3 单测、T07、C5、T03+T05、ipc_v3×2 —— 均在上述计内）。
- `cargo check --workspace --locked --offline`:通过。
- `git diff --check`:干净。
- 原始输出:`evidence/p3-lab/baseline-before.txt`（88 绿 + T02 红）、`evidence/p3-lab/after-fix.txt`（175 绿）。

声明:T03 红是"仅回退 C1 两处语义"的受控对照（其余修复保留）;T01/C5/C4 红为源码级分析实证（修改前行号已在表中），未做同置信度现场对照;LAB 均走真实命名管道与真实 worker/daemon 进程内线程,零 QQ 依赖。
