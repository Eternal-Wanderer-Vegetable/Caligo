# K4 第一批阶段报告:入口契约状态 + LAB 测试结果 + 剩余缺项

> 计划 §7.2 规定交付物。报告日期:2026-10-07,Asia/Shanghai。
> 范围:D0–D4 全部离线完成 + D5 前置修复;**全程零 QQ 现场动作**
> (未注入、未扫描存活进程内存、未发送任何消息;仅对磁盘上安装构建做
> 只读哈希核对,对进程列表做只读枚举)。首次阶段报告不要求真实 QQ 发新消息 —— 已满足。

## 1. 入口契约状态

| 契约 | 状态 | 依据 |
|---|---|---|
| D0 遗留路线门控 | **生效** | CLI `inject` / 旧 onebotd / bridge 七个宿主导出在普通构建首条语句拒绝(CLI/runner 退出码 3,导出结果码 0xE0);`--features research` 仅自建宿主;绕过面如实清单见 k4-execution-scope §3 |
| loader 超时资源缺陷(§5.3) | **已修** | 7 处等待点改为先判 wait;超时留存账本,不释放、不 TerminateThread |
| B0 bootstrap 契约 | **OFFLINE-CANDIDATE;B0-FIELD = BLOCKED(待 D7)** | 候选 B(RequestInterrupt 首入 → owner 线程初始化)为唯一有现场证据的路线;候选 A(Node-API addon)无可用装载点,判 BLOCKED;十项契约表与开放点见 k4-bootstrap-contract §3 |
| 事故结论修正 | **完成** | "stack 无 caligo 帧"被原始 dump 证伪(frame6 = caligo_bridge_diag2.dll);六例损失全部入账,机制分类与未知项分离(k4-incident-register §2) |
| 生产链路(k4 路线) | **离线已接线,LAB 层可运行** | model 三代次 → core actor/journal → bridge resident(自建宿主);真实命名管道与 caligod 属 D5,未开工 |

## 2. LAB 测试结果

`cargo test --workspace --offline`:**83 通过 / 0 失败 / 0 警告**(工具链 1.97.1,MSVC)。

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
| onebotd + gate | 4 | 旧 runner 退出码 3、不进入 ARM/poll |
| caligo-model | 7 | 三代次、RunId 迁移、payload hash 绑定、终态分类 |

L 清单未覆盖项(如实列出):L01/L02 的运行时身份面在 IPC v2(D5)落地;
L05/L06 的现场面在 D8;L14/L15 的完整面在 D5(replay 窗口)与 D10;
F01–F04 全部为现场项,未被任何 LAB 结果替代。

## 3. 剩余缺项与下一步

**D5(下一个工作批,纯离线)**:
1. IPC v2 语义(Hello/Ack 携带三代次 + 认证 + 恢复游标;消息类型固定集);
2. Windows 命名管道 transport(SID ACL、认证、指定进程校验、I/O 取消);
3. `caligod` 常驻 core + 测试客户端;`harness → resident → pipe → actor → journal → client` 全链接线;
4. L15(慢消费者/replay 超限 → 显式 Gap)与 L02 完整面。

**现场前置(需执行者参与)**:
- D6 准入核对表(`docs/acceptance/k4-field-entry.md`)—— D5 LAB 通过后逐项填写;
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

## 5. 纪律声明

- 未执行旧 inject/exec/onebotd(门控存在且测试证明拒绝先于一切访问);
- 未对任何存活 QQ 做注入、内存扫描或探针;未发送任何 QQ 消息;
- 只读操作:安装构建文件哈希核对(与冻结基线逐字节一致)、进程列表枚举;
- 原始证据(local-evidence/)零改动;敏感数据未入 Git。
