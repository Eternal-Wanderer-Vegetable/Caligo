# K4 第一批阶段报告:入口契约状态 + LAB 测试结果 + 剩余缺项

> 计划 §7.2 规定交付物。报告日期:2026-10-07,Asia/Shanghai。
> 范围:D0–D4 + **D5(LAB 全链接线)** 全部离线完成;**全程零 QQ 现场动作**
> (未注入、未扫描存活进程内存、未发送任何消息;仅对磁盘上安装构建做
> 只读哈希核对,对进程列表做只读枚举)。首次阶段报告不要求真实 QQ 发新消息 —— 已满足。

## 1. 入口契约状态

| 契约 | 状态 | 依据 |
|---|---|---|
| D0 遗留路线门控 | **生效** | CLI `inject` / 旧 onebotd / bridge 七个宿主导出在普通构建首条语句拒绝(CLI/runner 退出码 3,导出结果码 0xE0);`--features research` 仅自建宿主;绕过面如实清单见 k4-execution-scope §3 |
| loader 超时资源缺陷(§5.3) | **已修** | 7 处等待点改为先判 wait;超时留存账本,不释放、不 TerminateThread |
| B0 bootstrap 契约 | **OFFLINE-CANDIDATE;B0-FIELD = BLOCKED(待 D7)** | 候选 B(RequestInterrupt 首入 → owner 线程初始化)为唯一有现场证据的路线;候选 A(Node-API addon)无可用装载点,判 BLOCKED;十项契约表与开放点见 k4-bootstrap-contract §3 |
| 事故结论修正 | **完成** | "stack 无 caligo 帧"被原始 dump 证伪(frame6 = caligo_bridge_diag2.dll);六例损失全部入账,机制分类与未知项分离(k4-incident-register §2) |
| 生产链路(k4 路线) | **离线全链接线并验收(K4-LAB)** | `resident(host harness)→ 命名管道 → caligod daemon(账号 actor)→ journal → 控制客户端`,全程零 CLI inject;D5 验收:持续通信不启动 inject、断连恢复不重复初始化、限额/拒绝显式呈现 |

## 2. LAB 测试结果

`cargo test --workspace --offline`:**90 通过 / 0 失败 / 0 警告**(工具链 1.97.1,MSVC)。
D5 新增 7 项(transport_contract 3、daemon_chain 3、caligod_smoke 1)。

| 套件 | 数量 | 覆盖 |
|---|---:|---|
| caligo-bridge 单元 | 13 | 门控默认关闭、async/exec 门控先于状态访问、历史脚本常量回归 |
| resident_lifecycle(D4) | 11 | L07(precheck 先于一切宿主 API:外来线程/零上下文/env 失效)、L16(关闭协议、迟回调零 native、double close、Quarantined 不可洗白)、L17(10,000 次非发送调度 + 100 轮关闭/重建,计数分配器监控无增长、回基线)、跨线程提交/owner 执行、有界队列显式拒绝、重复 send 不重派发 |
| caligo-cli 单元 | 10 | 含留存账本登记测试 |
| legacy_gate(D0) | 4 | inject 任意形态均拒绝(先于门1/门2/进程访问)、文案指向恢复路径 |
| runtime_contract(D3) | 10 | L03 同数字群/私聊隔离、L04 同正文不同 ID 保留 + 重复抑制、L08 取消 vs native 竞争、L10 迟到/重复/未知回执幂等审计、L11 请求身份语义、准入相位/旧代次零 native、动作队列满、事件队列满 → Gap+Degraded、stop/close 协议 |
| recovery_contract(D3) | 6 | L09 dispatch_intent 前后崩溃不重发(重启后仅 QueryRequest)、native 后崩溃 → unknown 非失败、L13 尾截断保守恢复/中段损坏拒绝且原文件不动、去重与游标跨重启、ACK 依据持久账本 |
| ipc_contract(含 D5 前置) | 12 | 原 9 项回归保留 + L12(逐字节喂入、百帧大 chunk 缓存有界、超长声明仅读头拒绝) |
| journal 单元 | 6 | CRC32 标准向量、roundtrip、尾截断、中段损坏、容量上限 |
| transport_contract(D5) | 3 | 真实命名管道:逐字节分片还原、错误 token 拒绝不降级、阻塞读及时取消(不悬挂)、client PID 校验 |
| daemon_chain(D5) | 3 | **全链**:resident→管道→actor→journal→客户端;发送确认并关联原生 ID、重复 ID 查询语义不重派发、payload 冲突拒绝;事件经管道持久化 + 去重 + 交付游标;伪造 token 无会话副作用;journal 重开终态可审计、phase 回 Detached(身份须重核) |
| caligod_smoke(D5) | 1 | 子进程启动、stdout 首行交付管道名/token、Health、Stop 真实 Stopping 协议、退出码 0 |
| onebotd + gate | 4 | 旧 runner 退出码 3、不进入 ARM/poll |
| caligo-model | 7 | 三代次、RunId 迁移、payload hash 绑定、终态分类 |

L 清单未覆盖项(如实列出):L01/L02 的运行时身份面在 IPC v2(D5)落地;
L05/L06 的现场面在 D8;L14/L15 的完整面在 D5(replay 窗口)与 D10;
F01–F04 全部为现场项,未被任何 LAB 结果替代。

## 3. 剩余缺项与下一步

**D5 状态:LAB 完成(2026-10-07,提交 267a7f7 / 1d9fe9e)。** 实现事实:
- 命名管道:当前用户 SID DACL、`PIPE_REJECT_REMOTE_CLIENTS`、OS 随机 32 字节
  token(不进命令行/日志)、指定进程 PID 校验、`CancelIoEx` 读取消
  (取消后先回收 OVERLAPPED —— 计划 §5.3 语义)、断开前 200ms 排水窗;
- IPC v2:固定消息集(Bridge/Control 双角色)、认证/版本/角色门控、
  EventAck 仅在持久化后前移、receipt 三分类;
- caligod:journal 打开失败不静默重建(退出码 3);Ctrl+C 与 control Stop
  走同一 Stopping 协议(actor.stop→close,退出码 0);
- 全链:重复 ID 查询语义、冲突拒绝、事件去重跨管道、交付游标持久。

D5 遗留(如实,不影响 LAB 验收):
1. bridge 断连恢复路径在 daemon 中为 Degraded + 会话幂等重绑定;跨进程
   "core 重启不重复派发"的端到端演示目前由 recovery_contract(L09)在
   journal 层覆盖,子进程级 kill/restart 演练属 D10 现场轮次的 LAB 预演;
2. 控制面事件交付为拉取式(DrainEvents);push 推送在 K5 OneBot 层实现;
3. 单帧/队列/heartbeat 参数沿用计划建议初值,实测调优记录在 D11。

**现场前置(需执行者参与)**:
- D6 准入核对表**已建立**(`docs/acceptance/k4-field-entry.md`):离线项
  1.1–1.6、2.1–2.4、3.1–3.6 均为 PASS;唯一 UNKNOWN = 2.5(执行者在 D7
  当日指定并登记新测试实例);
- 候选 B 首入机器**已实现并通过 LAB**(`crates/caligo-bridge/src/qq_entry.rs`,
  仅 research 构建):RequestInterrupt 首入 → owner 线程核 current(零不放行)
  → owner 线程 uv_async_init 常驻句柄 → uv_async_send 唤醒面 → CLOSE_REQ
  关闭协议(超时保留句柄);qq_entry_lab 5 项(含零 current 拒绝、陈旧 env
  拒绝、关闭协议、D8/D9 面显式拒绝/延迟);真实 QQNT 地址/ABI 仍属 D7 现场
  证据(B0-FIELD),LAB 假符号不构成现场证明;
- D7 首次接入:B0-FIELD 十项中全部 [assumed] 项的现场实测;**在此之前
  现场保持暂停,final-boundary 铁律继续有效**;
- D8–D11:收发样本、三轮恢复、2 小时运行 —— 全部需要执行者指定测试实例与对端观察。

**已知开放问题(不掩饰)**:
- RequestInterrupt 首入在空闲 QQ 上延迟可达数分钟(K2-03 记录),D7 观察窗设计必须吸收;
- "loop 线程 == JS 线程"是强推断(同线程 id 互证),D7 需双读证实;
- G3 的回执关联(实际原生消息 ID)依赖 D8/D9 实测字段,当前模型以
  `Option<NativeMessageId>` 显式留空,不得用猜测填充。

## 4. 提交索引

| 提交 | 内容 |
|---|---|
| 1897f6a | D0 门控 + 留存账本 + k4-execution-scope |
| fddf8d9 | D1 事故台账 + 资源审计 |
| 831a78c | D2 bootstrap 契约 |
| 23cc9f4 | D3 模型/journal/actor + 20 项契约测试 |
| b8b688d | D4 resident + 自建宿主 11 项生命周期测试 |
| 8765fac | D5 前置 FrameDecoder 增量有界修复 |
| d5bc91e | 阶段报告 01(首版) |
| 267a7f7 | D5a 命名管道 transport + IPC v2 |
| 1d9fe9e | D5b caligod daemon + 全链测试(90 全绿) |

## 5. 纪律声明

- 未执行旧 inject/exec/onebotd(门控存在且测试证明拒绝先于一切访问);
- 未对任何存活 QQ 做注入、内存扫描或探针;未发送任何 QQ 消息;
- 只读操作:安装构建文件哈希核对(与冻结基线逐字节一致)、进程列表枚举;
- 原始证据(local-evidence/)零改动;敏感数据未入 Git。
