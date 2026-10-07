# K4 bootstrap 契约:首次初始化入口与会话所有权(k4-bootstrap-contract)

> D2 交付物(计划 §7-D2 / §6.1)。编制:2026-10-07。状态:**离线候选成立,B0-FIELD 未通过(待 D7)**。
> 本轮执行了 D2 op1 的离线检查(只读文件哈希);**没有对任何存活 QQ 做注入、扫描或探针**。
> 标签:`[verified]` 已核对证据;`[inferred]` 工程判断;`[assumed]` 待现场实测。

## 1. 离线检查记录(D2 op1,2026-10-07 执行)

- 冻结版本 9.9.33-52230 仍为本机安装版本。五项 SHA-256 与
  `docs/research/environment.json` 逐字节一致(QQ.exe ABBED71F…、QQNT.dll
  F6F35B85…、major.node 3FDC17FA…、wrapper.node 63112AB9…、application.asar
  882AA006…)。→ K1/K2 全部静态图谱(vtable RVA、导出表、布局链)继续有效。
- 观察到 18 个 QQ 进程在运行(只读枚举;运行实例不自动成为实验目标,test-scope §4)。
- 本版本运行时能力(沿用已核对证据,非桌面 Node 文档外推):QQNT.dll =
  Node 24.11.1 / V8 14.4.258.16-electron.0 / Electron 40.0.0(K2-03 r3 指纹,
  [verified]);导出全套 `napi_*`、`node::AddLinkedBinding`、
  `node::RequestInterrupt`、完整环境创建栈与标准 libuv(K1 导出表,[verified])。
- 调试后门:CDP(--remote-debugging-port)应用层禁用、--inspect 熔断位编译期
  关闭(K2 §8,[verified])→ 该通道保持判死。

## 2. 候选对照(D2 op2)

### 候选 A:自有 Node-API addon + 最小 QQ 宿主引导

前提:存在"把 addon 送入 QQ 有效 JS 环境"的合法装载点。

| 通道 | 结论 | 证据 |
|---|---|---|
| `qq_magic_napi_register` 注册 | **判死**(进表 A,不在 linked 查找域,JS 永不可见) | EXP-K2-00 [verified] |
| CDP/inspector 注入 JS | 判死(§1) | K2 §8 [verified] |
| 自建 Environment(方案 d) | 判负冻结(NewIsolate 崩溃;复活需 ABI 解码) | K2 §5 [verified] |
| major.load 加载自有模块 | 判非:那是"替 QQ 执行业务加载器",风险档不符 | K2-04 [verified] |
| 标准 dlopen/require 进入主 env | 无已知路径:require 非 JS 全局、cache=0、业务模块在闭包内 | K2-04/05/09 [verified] |

**A 的 B0 前置"可用装载位置" = unknown → 候选 A BLOCKED。** Node-API/TSFN 的
公开契约本身([verified-by-standard])继续作为**自建宿主(LAB)夹具**的调度模型
——LAB 用桌面 Node 或纯 Rust 模拟宿主验证同一套线程/生命周期语义,但这不构成
QQ 可行性证据(计划 §6.1 明示禁止)。

### 候选 B:原生 owner 入口(RequestInterrupt 首入 + owner 线程常驻初始化)

已验证资产(全部为指定实例现场实证,版本绑定 9.9.33-52230):

| 资产 | 内容 | 证据 |
|---|---|---|
| Environment 定位 | `envscan`:离线 vtable 图谱(主 vtable RVA 0xA804990)+ 外部 RPM 扫描,唯一命中、±0 一致性判据 | K2-02 §11.1–11.2 [verified] |
| **owner 线程入口** | `node::RequestInterrupt(env, cb, ctx)` 回调在 QQ 的 JS 线程执行,≤10ms(活跃时) | K2-02 §11.3 [verified] |
| 轮转点载荷 | uv_async 回调在 JS 线程 55832 执行(与 RequestInterrupt 回调同线程,跨机制互证);mode 2 在该轮转点取得 entered 主上下文并执行 JS | K2-03 mode1/2 r3 [verified] |
| env 新鲜度门 | `[env+0] === qqnt_base + 0xA804990`,失配即拒(I-03 修复) | K3-E [verified] |
| 失败样本(反面教材) | 远程线程直调 v8 必崩;钉扎上下文跨时间执行 JS 必崩 | 事故台账 I-02/03/04/05 [verified] |

**候选 B 的初始化序列(契约草案,D7 验证对象)**:

1. envscan 取 env(外部只读)→ 新鲜度门 → 干跑校验布局链;
2. **首入**:RequestInterrupt 投递"初始化载荷",回调在 JS/loop 线程执行;
3. 回调内(此时才允许):核 `isolate_get_current`(零 current 不放行)→
   构造 HandleScope → **在 loop 线程上** `uv_async_init` 常驻唤醒句柄 →
   注册消息监听(经 mode2 已实证的 JS 执行通道,**每 session 一次**)→
   发布 ready;
4. 此后:工作线程只 `uv_async_send` + 有界队列(合法跨线程面);关闭在
   owner 线程 `uv_close` + 等关闭回调。

**已知开放点(不掩饰,均为 D7 设计输入)**:

- 首入延迟:RequestInterrupt 在空闲 QQ 上可挂起数分钟(K2-03 §12.3 记录)。
  首次初始化的时延上界无法承诺;D7 观察窗必须 ≥ 该延迟,或先确认
  "uv loop 线程 == JS 线程"后由初始化线程直接在同线程完成(仍需合法入口)。
- "loop 线程 == JS 线程"是 [inferred](两机制回调线程 id 同为 55832),
  D7 需以 `uv_thread_self`/`GetCurrentThreadId` 双读证实。
- 旧 asyncrun 从远程线程 `uv_async_init` 属 **libuv 契约违例**(恰未崩,
  不可作为先例);候选 B 的初始化点修复此违例。

## 3. B0 契约表(计划 §6.1 十项)

| 项 | 候选 B 当前答案 | 标签 |
|---|---|---|
| 宿主进程角色 | 官方 QQ 主进程(wrapper.node 独载判据);仅执行者指定的新测试实例 | [verified 判据 / 现场待指定] |
| 装载点 | bridge DLL 经研究 loader 装载(已门控);**JS/运行时初始化不经 DllMain、不经远程线程**,经 RequestInterrupt 首入 | [inferred,机制已实证/序列待 D7] |
| 真实回调来源 | RequestInterrupt 处理点 + 主 env uv 轮转点(线程 55832) | [verified] |
| API/ABI 版本 | Node 24.11.1 / V8 14.4 / Electron 40;调用按 K2-03 §12.2.1 sret 规则表 | [verified] |
| owner 线程 | JS/loop 线程;所有宿主 API 仅在该线程 | [verified 入口 / 线程同一性待 D7] |
| 合法环境来源 | envscan + 新鲜度门 + 干跑;**禁止**把扫描值当永久句柄,每次绑定即验证 | [verified] |
| 引用持有 | 待定:宿主侧引用机制(napi ref / 自管 tagged 槽)D7 实测后冻结;LAB 先用自管 token | [assumed] |
| 会话失效信号 | **unknown**:重登/会话失效的通知形态未实测(候选:监听器回调/轮询合法引用);D8 项 | [assumed] |
| cleanup 来源 | owner 线程统一清理(uv_close、对称移除监听器并确认);**监听器移除 API 的确切 token 未实测** | [assumed] |
| 一次性初始化与版本拒绝 | bootstrap 完成位 + host nonce;manifest 哈希不符即拒绝(普通构建已全局拒绝) | [verified 判据 / 现场待验] |

**B0 判定:OFFLINE-CANDIDATE(候选 B);B0-FIELD = BLOCKED,待 D7 现场证据。**
任一 [assumed] 项在 D7 落空 → 按 §6.1 记 BLOCKED,不降级、不拼凑。

## 4. 对实施序的影响(D2 op3/op4)

- D3(model/actor/journal)与 D4(resident/宿主夹具)按 **HostAdapter 抽象**
  实施:LAB 夹具实现同一 owner-token/上下文检查/清理语义,不含 QQ 地址与
  QQ ABI;`host_adapter` 生产分支**只写被证实路线**——在 D7 之前它只提供
  trait 与显式拒绝的占位实现。
- RequestInterrupt/uv/JS 通道的全部 K2/K3 知识只作为 D7 的**候选设计输入**,
  不提前接线;旧 asyncrun/exec 的实现不复制进 resident(计划 §5.2)。
- 若 D7 判定候选 B 的首入延迟不可接受且无合法替代入口 → B0 BLOCKED,
  按 §7-D2 失败分支:保留 LAB,提出 A2(协议边界)单独范围。

## 5. 变更控制

本契约替代 `js-context-acquisition.md` §9–§12 中"当前有效"的路线状态:
F-1/R-A/uv_async 载荷降级为**历史证据**;K4 候选 = 本文候选 B(B0-FIELD 未通过)。
后续变更须在本文件追加变更记录并同步 source-register。
