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
| P2 路线冻结 | **PASS（文档层；2026-10-08）** | `docs/research/k4-native-route-decision.md`、`docs/contracts/qq-9.9.33-52230.capability-profile.json`；loading-route-decision §4 / source-register S16–S18 增补 |
| P4–P9 | **P6 G1 准入 PASS**(含收口常驻观测,2026-10-08,实例 31208;见 P6 表);P4/P5 已交付;**G2 接收设计已定稿**(`docs/research/qq-native-receive-design.md`:push 监听器链 +0x170/slot6 注册方案);待实现:注册函数定位→LAB→现场采样 | 本文件 P6 表 + 接收设计文档 |
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
| 2026-10-08 | P1 受控观测 | 新增 `caligo-cli observe-msf`（外部只读:OpenProcess 仅 QUERY\|VM_READ,零注入/零写入/零 QQ 函数调用,不消耗实例首次 bootstrap）。读:MSF 双单例槽(+0x50/+0x60 transport/控制块计数)、执行器单例、全线程 TLS 提交目标(经验校准 TEB 布局,本机实证 TlsSlots@0x1480 非 0xE10)。self-test 全机械验证通过(含植入 pair 命中路径) | `crates/caligo-cli/src/observe_msf.rs`;`evidence/p1-observe-selftest.json` |
| 2026-10-08 | P1 现场闭合 | **实例 47524**（创建 2026-10-08T11:58:14Z UTC,9.9.33-52230,锚点命中）三次外部只读采样 t1/t2/t3:this+0x60 transport **已安装**（vtable RVA 0x41403B8,slot7=**0x1B4E4EC** 发送实际目标(锁 transport+0x60 pair),未连接时同步失败返回 0）;全局 dispatcher（0x67510A8,发送链提交目标）与 41 个 TLS dispatcher**同类**（vtable 0x43B6088,slot0=0x31F8CEE,入队+owner-TID 跨线程唤醒）;执行器单例 0x750088 **未构造**;MSFCoreService **未构造**（运行时 switch 选了 MSFService——首个运行时分支证据）。phaseH/I/J 静态反编译交叉定性 | `evidence/p1-observe-field-47524-t{1,2,3}.json`;`docs/research/qq-native-thread-contract.md` §10 |
| 2026-10-08 | P2 | 路线冻结:原生 MSF/SSO(服务族 MSFService,getter 72DE38;运行时分支现场证据);V8 降为备选研究资产(须修 C6-C8/C10+补等价线程证据方可独立评估);MSFCoreService/0x750088 执行器单例/stop-cancel-drain 原语显式禁用;capability profile 逐门标注(loading=admitted, attach=admitted-conditional, session_ready=blocked, receive/send=lab-only*, lifecycle_stop=rejected) | `k4-native-route-decision.md`;`qq-9.9.33-52230.capability-profile.json` |

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

| 2026-10-08 | P4 第一批 | 新增 `native_abi.rs`（恢复布局:ctrlblk 0x28/tagged24/byte24/callback move 协议/提交入口签名/panic 边界）、`native_handle.rs`（拥有 pair 句柄:代次绑定/释放对称/缺失释放家族→隔离计数）、`native_msf.rs`（adapter:claim 前置核对→无锁调用边界→独立有界完成通道/调度许可/去重幂等/失效审计/在途账本;真实链接 G1 前恒 None=拒绝占位）。假宿主按同一 C 形状受试。修正:文档 transport pair 偏移 +0x30→**+0x60**（1B4E4EC 的 param_1+0xC 按 longlong* 即 0x60 字节）。LAB 中发现并修正 move 协议语义（转移时销毁 capture→use-after-free;改为 guts 转移+源失效+dst 侧销毁恰一次;真实 op=0 transfer-in 细节留 P6 ABI 冻结定证）。附带修复 daemon_client_lab reconnect 预存在 flake（Stop 前等 worker 首连） | `crates/caligo-bridge/src/native_*.rs`;`tests/native_adapter_contract.rs`;`evidence/p4-lab/after.txt` |

| 2026-10-08 | P5 第一批 | 新增 `crates/caligo-core/src/qq_protocol/`(自研有界 protobuf wire 编解码,零第三方依赖):wire(varint/len/fixed,group 拒绝,未知字段跳过留痕,截断/坏 tag 显式拒绝)、send(PbSendMsg 私聊/群文本最小编码)、recv(MsgPush 最小路由:多级未知归集/未支持类型显式拒绝/群号 hint)、receipt(result/errmsg 分层;**成功缺稳定身份→Unconfirmed,不伪造**)、identity(UIN/群号/方向/平台时间守卫;方向不靠 sender==account 猜,时间缺失如实 None)。字段号按公开协议参考,**全部标记 verified:false**——字段级真实性待 G1/G2 真实抓包替换 fixture 后复核;fixture 全部脱敏自生成 | `crates/caligo-core/src/qq_protocol/`;`tests/qq_text_codec.rs` |

| 2026-10-08 | P4 第二批 | **resident 锁范围改造**(计划 §7-P4 核心):adapter 移出 Core 加独立 Mutex,全局锁序 adapter→core;`drain` 三段式(claim→invoke 无 core 锁→complete),`close` 的 ListenerRemove 同样无 core 锁 —— 内联回调重入 `token()/deliver_callback()` 不死锁(红验证:改造前该路径确定性挂起,timeout 击杀)。**C7 修复**:close 在途分类补上 SendPending→DeliveryUnknown(此前只分类 NativeStarted)。`precheck` 提为自由函数(计数经 core 锁,锁序不变)。bootstrap 保持双锁并注释理由(ListenerAdd 前无监听器=回调不可能到达)。新增 3 测试:inline 重入/close 期间重入/T14 SendPending 分类 | `crates/caligo-bridge/src/resident.rs`;`tests/resident_lifecycle.rs`(14/14) |

### P4 第二批验证(2026-10-08)

- workspace 119 + core research 70 + bridge research 45 = **234 passed / 0 failed**(分套计数;`evidence/p4-lab/after-b.txt` 为单次全量日志)。
- 重入红验证:改造前 drain 内联 token() 必死锁(std Mutex 不可重入),测试 timeout 击杀;改造后绿。

### P6 — G1 首次原生调用(2026-10-08,实例 31208)

**实例登记**(P6 纪律:新实例新 PID):
- PID **31208**,创建 2026-10-08T13:17:19Z(UTC),QQNT 9.9.33-52230;
- 探针前外部核对(observe-msf t1):锚点命中,单例活跃,transport 已安装;
- manifest 全模块 PASS(双门控之 gate 1),执行者重启实例即为指定确认(gate 2)。

**首次原生调用**(inject → caligo_g1_probe_run,loader 新建线程 tid=4672):

| 阶段 | 结果 |
|---|---|
| pre_state | 单例在位 obj=0x25c12d70000 strong=433 |
| **getter 72DE38 调用** | **成功**——非 QQ 线程被准入,返回 pair(线程准入问题的首个运行时答案) |
| anchor | obj vptr RVA 0x3f6ded8 == 锚点 |
| refcount | strong 433→**434**(恰 +1,getter 契约成立) |
| transport | +0x60 在位,vptr RVA 0x41403b8 == R3.1 现场观测值 |
| inner_pair | transport+0x60 在位(0x25c1284ea40)—— 会话已连接 |
| 纪律 | **零发送**;无释放(家族未定证,租约持有至进程退出,如实记录) |

**事后**:QQ 实例存活(同 PID/创建时间);t2 外部复查 strong=465(自身活动平稳)、锚点仍命中。

**结论**:G1 的"线程准入 + 真实对象/引用 + 账号连接状态"三项全部通过;getter 从非 QQ worker 线程可调用是 capability profile `gates.send` 有条件准入的关键证据。

### G1 收口:常驻原生观测(2026-10-08,同实例)

**第二次注入**(bridge v2,新 target 目录构建 —— v1 DLL 仍被进程持有,按"不热卸载"纪律共存;v2 额外含 `caligo_g1_native_run`):

| 项 | 结果 |
|---|---|
| 租约获取 | getter 一次(tid 45160),strong=801 起租,**零重复获取** |
| 观测窗口 | 10s,20 次只读探测(500ms 周期):transport_present **20/20**、inner_present **20/20**(会话全程在位) |
| 租约完整性 | strong min=801(全程未低于起租值 = 我们的 +1 始终持有);801→809 波动为 QQ 自身引用活动 |
| 停止链 | 窗口结束 → 内部停止 → 汇总落盘 → **stop=clean**,远程线程正常退出 |
| 事后 | QQ 存活(同 PID/创建时间);t3 外部复查锚点命中、strong=824 平稳 |

**G1 准入判定:PASS** —— 线程准入(探测循环持续于我们线程)、对象寿命(租约全程完整)、账号状态(transport/inner 连续在位)、停止链(有界窗口 + 干净退出)四项齐备。**G2 收消息门未动**(发送路径维持零调用;接收接线属下一阶段)。

注:进程内现存两个 bridge 模块实例(v1 探针租约 + v2 观测租约,各持 1 个强引用),随 QQ 退出回收;不热卸载。

证据:`evidence/p6-g1/`(observe t1/t2、probe json、g1 jsonl);工具 `crates/caligo-bridge/src/g1.rs` + `caligo-cli observe-msf/inject --g1-report`。

## P5 反例与验证(2026-10-08,4/4 绿 + 模块单测 17 绿)

| ID | 场景 | 结果 |
|---|---|---|
| T17 | 群/私聊 MsgPush、同正文双 ID 保留、Unicode/12KB 长文本零截断 | 绿 |
| T18 | 未知字段跳过留痕(多级归集)/截断/未支持类型/缺 group hint 拒绝 | 绿 |
| T19 | result=0 缺稳定身份→Unconfirmed;失败带原因;缺 result/截断→Unreadable;零伪造 | 绿 |
| T20 | 群号==sender 拒绝/空账号零 peer 拒绝/方向不猜/platform_time 不补齐 | 绿 |

### P5 验证命令结果(2026-10-08)

- 全套命令:**228 passed / 0 failed**;`cargo check` 通过;`git diff --check` 干净。
- 原始输出:`evidence/p5-lab/after.txt`。

### P4 反例与验证（LAB,2026-10-08,7/7 绿）

| ID | 场景 | 结果 |
|---|---|---|
| T11 | inline callback:不死锁、结果恰一次、临时源释放后数据完整（分配对账） | 绿 |
| T12 | getter/handle/释放对称 ×100 lifecycle 回基线;释放家族缺失→隔离计数（不猜地址） | 绿 |
| T13 | 重复回调幂等/失效代次只审计/reply 先于 ACK 不改语义 | 绿 |
| T14 | 在途账本:已提交未终态→pending 分类 Unknown;失效后迟到回执不翻案 | 绿 |
| T16 | 链接未接线/线程未准入/超短形容量命令 → 全部调用前拒绝,native counter 零 | 绿 |
| T15 | （既有 resident_lifecycle 已绿,本轮未动） | 绿 |
| 10k | 10,000 次非发送调度:计数对账、零假宿主分配增长、溢出零 | 绿 |

### P4 验证命令结果（after,2026-10-08）

- 全套命令:**190 passed / 0 failed**;`cargo check` 通过;`git diff --check` 干净。
- 原始输出:`evidence/p4-lab/after.txt`。

### P3 验证命令结果（after-fix,2026-10-08）

### P3 验证命令结果（after-fix,2026-10-08）

- `cargo test --workspace --locked --offline` / `cargo test -p caligo-core --features research --locked --offline` / `cargo test -p caligo-bridge --features research --locked --offline`:**175 passed / 0 failed**（含 5 个新测试目标场景:T02、T01×3 单测、T07、C5、T03+T05、ipc_v3×2 —— 均在上述计内）。
- `cargo check --workspace --locked --offline`:通过。
- `git diff --check`:干净。
- 原始输出:`evidence/p3-lab/baseline-before.txt`（88 绿 + T02 红）、`evidence/p3-lab/after-fix.txt`（175 绿）。

声明:T03 红是"仅回退 C1 两处语义"的受控对照（其余修复保留）;T01/C5/C4 红为源码级分析实证（修改前行号已在表中），未做同置信度现场对照;LAB 均走真实命名管道与真实 worker/daemon 进程内线程,零 QQ 依赖。
