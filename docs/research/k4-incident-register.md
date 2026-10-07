# K4 事故台账与资源审计(k4-incident-register)

> D1 交付物(计划 §7-D1)。编制:2026-10-07,Asia/Shanghai。
> 方法边界:**纯文档/证据复核**,未执行任何 QQ 交互、未重现崩溃、未启动注入
> (计划 D1 停止条件:不为补证据重现崩溃;崩溃复现只在自建测试进程)。
> 原始 crashpad JSON/txt 全部留在 `local-evidence/`(不入库);本文只写脱敏摘要。
> 敏感提示:k2-03 的 crashpad 伴随 txt 含窗口列表与历史账号 ID 原文,不入 Git。

## 1. 证据材料与口径

| 材料 | 位置 | 能提供 | 不能提供 |
|---|---|---|---|
| crashpad JSON(帧列表) | `local-evidence/k2-03/90bb0b36*.json`、`82337034*.json`、`local-evidence/k3-e/63a82da9*.json`、`9a0d383c*.json`、`7f75e838*.json` | 帧地址/模块/RVA offset | PID、时间戳、异常码、线程 ID、寄存器(字段不存在) |
| run-notes / retrospective / incident 记录 | `local-evidence/k2-03/run-notes*.md`、`k3-e/final-retrospective.md`、`k3-e/incident-stale-env.md`、`k3-g/final-boundary.md` | 时间线(文字)、当时归因、实例 PID | 二进制级证据 |
| 源码 | `crates/`(HEAD 6fd8329) | 资源创建/持有/关闭点 | 当时事故构建的确切源码版本(部分修复在事故后入库,见 §3) |
| 模块摘要 | `docs/research/environment.json` | 全部事故同版本 QQ 9.9.33-52230(aff854e8) | — |

桥接 DLL 构建哈希/PDB:**未归档**(k203 / diag2 / e_100637 / k3e_f 各构建均无
SHA-256 或 PDB 记录)。因此**各 dump 中我方 DLL 的 offset 无法映射回源码行**,
一律记"无法定位",不拿当前构建偏移套旧栈。

## 2. 事故表(六次实例损失)

时间均为文字记录的本地时间(UTC+8);JSON 无时间字段,精度以记录为准。

| ID | 实例 PID | 阶段 | 时间(文字记录) | crashpad | 模块栈摘要(已核对 JSON) | 已确认机制 | 未知项 |
|---|---|---|---|---|---|---|---|
| I-01 | 9000 | K2-03 r1(mode 2,上下文阶梯) | 2026-10-06 ~20:26(当日 10:53 启动) | `90bb0b36`(k2-03/) | frame0=QQNT+0x429a0c0(GetEnteredOrMicrotaskContext 域);**frame1=caligo_bridge_k203.dll+0x1467c** | sret ABI 反序:成员函数参数顺序颠倒、返回值误写 `[isolate+0]` → 静默内存腐蚀后 AV | 异常码/线程 id(dump 无字段);腐蚀范围 |
| I-02 | 28532 | K2-03 r2(mode 3 挂起中断捕获) | 2026-10-06(第二轮) | `82337034`(k2-03/) | run-notes:AV@QQNT+0x429A081;`[栈+0x100D0]` 不可读 | 钉扎/捕获的 tagged Context 跨时间失效后按 Local 解引用 | 同上;未逐帧核对 82337034 JSON(留待现场证据若需) |
| I-03 | 13056 | K3-E exec 首轮(ARMED 探针) | 2026-10-07 10:07 | `63a82da9`(k3-e/) | frames0-3=QQNT(+0x429a0d1 等);**frames4-5=caligo_bridge_e_100637.dll(+0x1ad19/+0x14b3c)**;**frame6=KERNEL32(远程线程跳板)**;frame7=ntdll | **故障线程 = 我方远程线程**:上下文阶梯在外来线程调 v8(v8 TLS 未初始化);叠加 env 陈旧(数小时前 envscan 值) | PDB 缺失,+0x1ad19 无法映射源码行 |
| I-04 | 49688 | K3-E(阶梯移入 exec_cb 后复验) | 2026-10-07 10:31 | `9a0d383c`(k3-e/) | 与 I-03 同型:**caligo_bridge_k3e_f.dll(+0x1aec4)** + KERNEL32 跳板帧 | env 全新鲜仍崩 → **推翻"陈旧 env"单因**,坐实外来线程 v8 TLS 机制 | 同上 |
| I-05 | 44028 | K3-E(hint 上下文执行 JS) | 2026-10-07(文字 ~02:5x,与同表 10:07/10:31 时序矛盾,存疑待核) | **未归档**(retrospective 无 id) | 文字:v8 内部崩溃 | 钉扎 arm 时上下文跨时间执行 JS,app 状态已演进 | crashpad 原件、精确时间、帧 |
| I-06 | 22532 | K3-F 验收通过后的 drain/重试注入 | 2026-10-07(K3-F 之后) | `7f75e838`(k3-e/) | 29 帧:frames0-5=QQNT(**offset 70034075/70250027** 等);**frame6=caligo_bridge_diag2.dll+0x1A858(108632)**;frames7+ 深层 QQNT | AV 在 QQNT 会话服务内部;**我方模块在栈上(frame6)** | +0x1A858 无法映射(diag2 无 PDB);我方帧是被回调还是返回路径残留,无法判定;PID/时间未入 dump |

**与 final-boundary.md 表述的核对(D1 op2)**:`k3-g/final-boundary.md` 称 7f75e838
"stack 无 caligo 帧"。逐帧核对结果:**frame 6 = caligo_bridge_diag2.dll** ——
该表述与原始证据不符(检测报告 2026-10-07 同样指出)。修正口径:

- "崩溃点在 QQNT 内部"成立(frames0-5);但"栈上没有我方模块"**不成立**。
- 我方帧在栈上不等于我方帧是因(可能为正常返回路径残留);但"全部损坏均与
  我方调用无关"的推断**失去该论据**。本台账按"无法排除调用方线程/ABI/时序
  缺陷"处理,K4 设计必须覆盖这些风险(§5)。
- 7f75e838 无法唯一映射到 PID/时间/构建(dump 无这些字段);I-06 的 PID 22532
  来自文字记录,非 dump 内证。

累计损失口径:final-boundary 列出的六例(9000/28532/13056/49688/44028/22532)
**有证据支撑为本项目实验直接相关损失**;不外推为"项目历史全部实例损失总数"
(更早的干跑/牺牲进程死亡未逐例归档)。

## 3. 资源审计:创建 / 持有 / 关闭账本(D1 op3)

逐资源核对当前 HEAD(6fd8329 + D0 提交)源码。行号为当前源码。

### 3.1 加载器侧(caligo-cli/winutil.rs,research 隔离)

| 资源 | 创建 | 持有 | 关闭 | D0/D1 后状态 |
|---|---|---|---|---|
| 远程线程 ×7(load/probe/obs/env/intr/exec/async) | `CreateRemoteThread` 各点 | 线程句柄 | `CloseHandle` 全部成对 | 句柄无泄漏;**等待语义已修**(下条) |
| 等待结果 | `WaitForSingleObject` | — | — | **原缺陷**:7 处均"先释放后判 wait";已改为先判 `wait`,超时转入留存账本(`RetainedRemoteAlloc`),不释放、不 TerminateThread |
| 远端缓冲(bridge/report/obs/env/intr/async 路径,obs/intr/exec/async ctx,exec js,async params) | `VirtualAllocEx`+`WriteProcessMemory` | 远程线程执行期间 | `VirtualFreeEx` | 超时路径全部留存登记;**env 报告缓冲因内部 worker 生命周期不可证明,一律留存** |
| 进程句柄 | `OpenProcess` | 全程 | 出口 `CloseHandle` | 正常 |

### 3.2 bridge 侧(caligo-bridge,research 隔离)

| 资源 | 创建 | 持有 | 关闭 | 风险判定 |
|---|---|---|---|---|
| uv_async 句柄 | `alloc`@asyncrun.rs:1034 + `uv_async_init`@1048 | 宿主 loop | **从不 uv_close**(1097 注释"随进程退出回收") | **泄漏式**:每轮 inject 泄 1 个句柄;60+ 轮探针即数十个;K2-03 r3 已记录"两枚惰性句柄遗留"。loop 上的死句柄在 close 前不可复用同名初始化(K4 常驻设计必须一次初始化) |
| 导出束 Exports | `Box::leak`@asyncrun.rs:899 | 进程生存期 | 从不 | 每 inject 泄 1 份;量小但属计划 §5.2 点名的"泄漏式对象持有" |
| 动态发送 JS | `Box::leak`@asyncrun.rs:991 | 进程生存期 | 从不 | **随参数化发送次数线性增长** |
| 全局回调状态(CB_*/RESULT_*/EXPORTS) | 静态原子 | 跨轮 | 每轮 `async_run` 入口复位(846–857) | 无单飞约束:两轮重叠会互踩;K3 实际串行使用未触发 |
| HandleScope | ctor@553 | 回调内 | dtor 成对(各 early-return 均配对,574/595/605/613/623) | 核对无失配 |
| linked binding 节点 | `register.rs`(qq_magic 注册链) | 宿主 node_module 表 A | **不可摘除**,随进程回收 | 每 DLL 装载永久+1 节点;重复注入多份即多节点(js-context-acquisition §3 如实记录) |
| JS 消息监听器(script 82 ARM) | 每轮 ARM 注册 | QQ 会话内跨 inject 存活(K3-D 实证) | script 12 仅移除 RM tap,**未见移除 msg listener 的对称路径**(检测报告 §4.3) | 注册/移除不对称 → 累积监听器;final-boundary 归因的内核磨损源之一 |

### 3.3 审计结论

1. **无单点"神秘"资源**:六次事故中三次(I-03/04/05)有已确认的代码级机制
   (外来线程 v8、ABI 反序、钉扎上下文失效),两次(I-01/02)同族,一次
   (I-06)机制未定但存在累积磨损假设下的时间相关性。
2. **累积面真实存在**:uv 句柄/导出束/DYN_JS/监听器四类资源随轮次单调增长,
   与 final-boundary"数百次累积后内核侧损坏"的观察在方向上一致;但
   "累积磨损不可避免"未被证明 —— K4 的一次初始化 + 确定性关闭设计(计划
   §6.3/§6.7)正是针对该审计结果。
3. **timeout≠终止** 的释放缺陷已在 D0 修复(留存账本);该缺陷与 I-03/04 的
   外来线程机制独立,属计划 §5.3 的独立风险面。

## 4. 已证实事实 / 候选根因 / 仍缺证据

**已证实(证据可直接支撑)**
- I-01/02:sret ABI 反序与挂起中断捕获缺陷,事故后修复并有阶段化 JSONL 定位。
- I-03/04:故障线程为我方远程线程(KERNEL32 跳板帧 + 我方 DLL 帧),外来线程
  调 v8 上下文访问器必崩且与 env 新鲜度无关。
- 三份 crashpad(63a82da9/9a0d383c/7f75e838)的栈上都存在我方模块。
- 全部六例运行在同一 QQ 版本(9.9.33-52230)。
- 资源累积面(§3.2)在当前代码中仍以 research 构建形式存在(已门控)。

**候选根因(有证据指向,未唯一证实)**
- I-06:高频 wrapper 会话实例化/监听器注册-移除循环导致的内核侧状态磨损
  (final-boundary 假设)。支持:时间相关性、§3.2 累积面;反对证据不足:
  无逐轮资源计数、无对照实验(也不再做)。
- 超时后释放被引用缓冲(UAF):当前代码中真实存在过(D0 已修),但**无证据
  表明它是六例中任何一例的直接原因**(超时场景在这些事故中未出现)。

**仍缺证据(如实登记,不虚构)**
- 全部六例的异常码、故障线程 ID、寄存器、当次构建 SHA/PDB。
- I-05 的 crashpad 原件与精确时间;I-06 的我方帧语义(回调 or 返回残留)。
- "每轮注入对内核会话状态的磨损量"无计数(资源计数能力在 K4-D4 补齐)。
- 累积磨损与单次缺陷(线程/ABI)的归因比例。

## 5. 对 K4 设计的约束映射(审计 → 计划条款)

| 审计发现 | 约束 | 计划落点 |
|---|---|---|
| 外来线程进 v8 必崩(I-03/04) | 一切宿主 API 前 owner/thread/context 验证,先于 HandleScope | §5.1、§6.3、D4 验收 |
| ABI 反序静默腐蚀(I-01) | 导出调用封装单点化 + 阶段化可定位报告;不再手写 sret 链 | D4 失败分支 |
| 钉扎上下文跨时间失效(I-02/05) | 对象带生命周期 token,每次进入前验证;不做跨时间裸复用 | §6.2/§6.3 |
| uv 句柄/导出束/JS 泄漏式持有 | 一次初始化、owner 线程 uv_close、无 Box::leak 生命周期方案 | §5.2、§6.3 |
| 监听器注册/移除不对称 | 每 session 一次注册、保存确切 token、对称移除并确认 | §6.3、D7/D8 |
| 超时释放(UAF 面) | 终止证明前留存;已落地留存账本 | §5.3(D0 已修) |
| 多轮全局状态互踩 | 每请求 owned 数据、宿主代次隔离 | §5.2、§6.2 |

## 6. 与历史文档的冲突修正

- `k3-g/final-boundary.md`:"stack 无 caligo 帧" → 修正为"栈上存在我方模块帧
  (frame6),其因果未定"(§2)。原文件保留不改,以本台账为更正记录。
- `k3-e/final-retrospective.md`:I-05 时间 "~02:5x" 与同表 I-03(10:07)/
  I-04(10:31)排序矛盾 → 本台账标记存疑,待执行者如有本地日志可修正。
- "累计磨损不可避免""所有自有 bridge 路线不可行":证据不足,不作为 K4 结论
  (检测报告同判定);K4 以新所有权方案先在自建宿主验证。
