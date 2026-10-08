# 本机 QQ 原生契约与 SnowLuma 版本适配调查（R2）

日期：2026-10-08。Caligo 基线：`main @ 681434bb8c0a32fd4e98ddaceadedf7e48f95782`。范围：当前 QQ 固定样本的定向静态分析、被动进程模块身份核对、SnowLuma 原生适配逻辑与官方维护历史；没有注入、执行目标函数、修改 QQ 内存或修改 Caligo 业务代码，没有提交或推送。

## 1. 结论与决策

**需要分析本机 QQ，且本轮已经完成一轮可复核的定向分析。** SnowLuma 的 caller 只能说明它怎样尝试调用 QQ；QQ 本体才能说明参数是否被复制、对象是否带引用、original 是否接管输入、回调数据何时失效。仅分析 SnowLuma，不足以设计自有 Rust 内核。

本轮已将“日志定位的候选入口”推进到固定版本的对象、部分 ABI、拥有权和回调消费契约。服务 getter、账户 getter、两类 MSF 对象和收包布局得到交叉支持；发送 command/body 同步深复制及结果回调形状得到机器码支持。**线程入口、实际 transport/executor、取消与销毁排空仍未闭合，不能据此宣称 K4 完成或继续以 G3 样本收敛为唯一目标。**

SnowLuma 通过“语义日志锚点 + RTTI + PE 异常表/引用关系 + 有限已知布局”应对部分 QQ 更新。它没有被证明能自动理解任意新 ABI；官方历史明确出现更新 native 二进制来修复 QQ 发送的维护记录。Caligo 应参考定位方法，另行建立版本身份、能力合同和未知版本拒绝规则。

## 2. 样本与实际加载身份

| 项目 | 本轮核对结果 |
|---|---|
| 安装版本目录 | `D:/Program Files/Tencent/QQNT/versions/9.9.33-52230` |
| Launcher 文件版本 | `9.9.33.52230` |
| 原生输入 | `resources/app/wrapper.node`，115,118,632 字节，Windows x64 PE |
| SHA256 | `63112ab9161e127f5f7e17998a7196e143808923fb54cbbf7b4e21426187a5f0` |
| PE preferred image base | `0x180000000`；下文所有短地址均为 RVA |
| 异常表 | 525,295 个 RUNTIME_FUNCTION 记录；记录不等于完整逻辑函数数 |
| 只读分析副本 | `E:/stella/_reference/qq-native-r2-20261008/inputs/wrapper.node`，与安装文件摘要一致 |
| 被动加载核对 | 2026-10-08 16:23:09 +08:00，PID 1340、15944 的 `QQNT.dll` 与 `wrapper.node` 均来自该版本目录 |

被动模块记录证明观察时的加载路径，不证明两个进程的账号、角色、当前会话可用性或回调线程；没有读取 switch 当前配置或提取运行对象。文件摘要绑定固定输入，并不是本轮已验证每个加载模块内存页的摘要。进程/模块状态会变，后续现场必须重新核对 PID、创建时间和模块身份。

证据：[输入清单](E:/stella/_reference/qq-native-r2-20261008/input-manifest.json)、[被动模块记录](E:/stella/_reference/qq-native-r2-20261008/qq-loaded-module-metadata.json)。

SnowLuma 原生分析来自 v1.14.21 固定 commit `9632b006385e603c9d9fe9150ab433e43792591f`。本轮另下载 v1.14.22 仓库标签中的 Windows x64 DLL/.node 作为数据，逐文件摘要与 R1 样本完全相同：DLL `215a32fe72120f23bed5130fa14c121877db28c69c2088aa51893cfbfee26ec4`；.node `6346a28cfcd09b1c99c0c15f3094ed579200f3dfd3edbf27512e81d865a26991`。因此 R1 的这两份原生文件分析可以复用。这个比较不覆盖完整发行 ZIP、其他平台或 TypeScript 源码。

证据：[v1.14.22 原生比较](E:/stella/_reference/qq-native-r2-20261008/snow-native-v1.14.22-comparison.json)、[官方版本页](https://github.com/SnowLuma/SnowLuma/releases/tag/v1.14.22)。

## 3. 分析引擎、覆盖范围与可信边界

沿用已经验证的 Ghidra 12.1.4 headless，引擎直接处理 `.node` PE。REA 作为调查组织方法；其当前 Windows provider 的目标过滤不接受本轮 DLL/.node，不能把“安装了 REA”视为已完成二进制分析。分析调用直接使用 `analyzeHeadless`，没有运行目标文件。

面对 115 MB 模块与大量异常表条目，使用 `-noanalysis`、PE 函数范围定位及有限 direct callees 展开，不进行无界全模块自动分析。引擎设置仅在本次子进程环境中生效；没有修改全局 Java、MCP 或 QQ 配置。

| 阶段 | 定向函数导出 | 重点 |
|---|---:|---|
| phase1 | 84 | 发送、收包、登录锚点及直接辅助链 |
| phase2 | 95 | MSF 子对象、Core 收包、调度、callback 管理 |
| phase3 | 36 | getter、构造、Core consumer、body copy |
| phase4 | 44 | 身份 getter、发送结果消费、引用辅助链 |
| phase5 | 135 | 主 vtable 非叶入口及有限直接调用扩展；达到设定 cap |
| 合计 | 394 次导出、313 个不同 RVA | 含跨阶段重复导出，不能声称覆盖整个 QQ |

各阶段失败计数为零并保存项目；`complete=true` 只表示该定向脚本没有取消，**不代表模块、类或线程链完整分析**。不在异常表的短 leaf/thunk 另用 dumpbin 交叉验证。Ghidra 自动原型、变量名、不可达块及 allocator/PIC 警告均不作为作者符号事实；关键 ABI 和移动/复制以 caller、callee、机器码共同确认。

证据：[分析脚本](E:/stella/_reference/qq-native-r2-20261008/scripts/TargetedQqAnalysis.java)、[阶段与原始证据目录](E:/stella/_reference/qq-native-r2-20261008)、[保存的项目](E:/stella/_reference/qq-native-r2-20261008/projects/QqWrapper52230.gpr)。

## 4. 对象发现：不能把两种 getter 混为一个接口

### 4.1 MSFService / MSFCoreService

两种 RTTI type descriptor 各唯一一份。COL 子对象偏移、vtable 和构造函数写入吻合：MSFService 分配 `0x380` 字节，有 0/8/0x10/0x18/0x20 子对象；MSFCoreService 分配 `0x3D0`，另有 `+0x40` 子对象。Core `+0x40` 的 slot 5 指向 `748E5A`，支持 SnowLuma 该接收安装点的布局。

精确复现 SnowLuma 的 `107775_switch` caller 数筛选后，服务 getter 被选择为 `72DE38`。它用调用者输出缓冲区返回两个 qword：对象和控制块，RAX 返回该缓冲区地址；配置分支可转向 Core getter `74203C`。非零控制块会增加 `+8` 引用计数。当前运行配置值没有读取，因此不能断言当前 QQ 走哪套服务。

### 4.2 账户身份 getter

从登录成功日志入口复现排序和形状规则，唯一最高频候选为 `9C3B14`。它无参返回内联账户对象 `6750220`，不是服务输出 pair getter。SnowLuma 的这两项基线槽位分别为服务 `[0x27]` 和身份 `[0x2B]`，ABI 也不同。

### 4.3 引用管理的静态缺口

QQ getter 增加控制块引用；SnowLuma `1760/15C0/4C40` 三条限定函数路径只消费对象 qword，未见控制块读取、转移或平衡释放。伪代码和完整限定函数指令均检查过。在“本版选中 getter、控制块非零、该路径实际执行”的条件下，这是需要解决的引用合同缺口；没有现场证据证明泄漏速度、QQ 不能退出或它造成 Caligo D9。

自有内核应保留完整 native handle，恢复 retain/release/disposer 并绑定代次。不能只缓存裸对象地址，也不能直接把未知控制块解释成某个 STL 版本的公共 `shared_ptr` ABI。

证据：[完整对象与选择报告](E:/stella/_reference/qq-native-r2-20261008/reports/qq-object-resolver.md)、[服务选择结果](E:/stella/_reference/qq-native-r2-20261008/qq-snow-service-getter-selection.json)、[账户选择结果](E:/stella/_reference/qq-native-r2-20261008/qq-snow-login-getter-selection.json)、[调用侧引用审计](E:/stella/_reference/qq-native-r2-20261008/snow-getter-call-audit.json)。

## 5. 发送：已恢复什么，断在哪一跳

kind 1 的 QQ 主函数 `732A80` 与 SnowLuma caller 的五参数一致：raw service、24 字节 tagged command 引用、begin/end byte container、reply-mode flag、带 manager/invoker 的 callback 管理对象。QQ 在当前调用中深复制 command/body，分配自己的请求，移动用户 callback。不能把这些参数替换成 Rust `String`、`Vec` 或普通函数指针。

最终发送在 `732EB9` 进入 `*(this+0x60)` 的 vtable `+0x38`。**这个 transport 对象的安装来源和 actual target 仍 UNKNOWN。** 已检查构造、主 vtable 入口和有限 direct callees；构造把字段置空，尚未找到闭合安装链。主函数没有线程 ID guard，不构成“任意线程调用安全”的证明。

QQ 结果 invoker `73C34C/73BD08` 同步调用消费者 `73BD6C`，消费者再同步调用用户 callback。用户参数为 callback object、常数 `0x10`、归一化 signed status、局部 tagged reason、局部 body container。reason/body 在回调返回后清理，Rust 必须在回调内复制为拥有数据，再异步送 IPC。常数 `0x10` 的完整语义未命名；out-token/原生完成不是聊天消息 ID 或对端已收到的凭据。

kind 2 的 Core `+0x10` slot 3/4 具有不同参数形状，不能沿用 kind 1 的 flag + stack callback 原型。登录锚点 `6E65E8` 是成功处理链，不是发起登录函数，也不是当前 Session ready 证明。

证据：[发送、登录与线程报告](E:/stella/_reference/qq-native-r2-20261008/reports/qq-send-thread-contract.md)。

## 6. 接收：复制必须在原函数接管之前

MSFService `1B41AE6` 读取 packet 的 seq `+0x18`、command `+0x20`、body pointer `+0x38`；匹配 pending 请求后可将输入指针移走并清零，最终执行 holder cleanup。SnowLuma 在 original 之前复制，符合这个有效期边界。Rust 观察点不能在 original 返回后借用 packet，也不能直接把 SSO packet 当 OneBot 消息。

该分支经 `1B3F740` 构造 closure，从当前线程 TLS 取 executor，并调用 `D32138 → executor vtable slot 0`。Core 则从 `748E5A` 接管输入，保存 `this-0x40`，通过带控制块的静态 manager/executor 链进入相同 dispatch 包装；consumer `74CF22` 的标签 3/4、seq `+4`、command `+0x40`、body `+0x58` 和跳过四字节行为支持第二套 SnowLuma 布局。

共同 dispatch 包装不证明两个 executor 相同，更不证明 owner 或实际执行线程。下一步必须解析各自 actual target 和 stop/drain；当前只能确认调度结构及拥有权转移。

证据：[接收、拥有权与调度报告](E:/stella/_reference/qq-native-r2-20261008/reports/qq-receive-thread-contract.md)。

## 7. SnowLuma 如何应对 QQ 更新

| 变化 | 实际机制 | 已知边界 |
|---|---|---|
| ASLR、函数 RVA 变化 | 找加载的 wrapper.node，解析 PE；从日志文本、RIP 引用、异常表恢复函数 | 不依赖固定绝对地址；日志删除/引用图改变仍可失败 |
| vtable 地址移动 | 类型名 → RTTI/COL → vtable → 构造/实例 vptr | 类型改名、禁用 RTTI、继承布局变化不能自动解决 |
| MSF 两套实现 | 分别恢复 MSFService / MSFCoreService，套已知字段/slot 和 this 调整 | 是有限布局支持；没有跨版本矩阵，不能命名成“全部旧版/新版” |
| 模块/对象暂未就绪 | 启动约 250ms 重试解析、约 20 秒等待；运行中约 5 秒刷新对象和补装 hook | 运行中对象刷新没有重跑完整 F6C0 resolver；不是升级新 ABI |
| socket/session 变化 | bridge client 重建、outbound 健康状态更新 | 不是更新 native 或签名数据库 |
| QQ ABI 实际改变 | 公开维护历史有更新 bundled native 修复发送 | 需要维护与分发；本轮没有旧/新二进制内部差分 |

被审查链包含空值、部分唯一性、形状、vtable 可执行页和实例验证；部分路径仍是首匹配策略。未发现此链的 QQ build 白名单、wrapper SHA256 gate、远程签名库下载或完整未知 ABI 拒绝合同。FNV 类函数输入是固定字符串 `1`，不能误报为 QQ 文件 hash 校验。接收 hook 成功布尔值并非启动 pipes 的完整门槛，因此 pipe online 不能等同 native receive 已成立。

官方历史证据：2026-08-14 的 [refresh bundled native so sending stays up](https://github.com/SnowLuma/SnowLuma/commit/7ae0fa3c0e391ea810330b43e8797adc113c7dad) 和 2026-08-16 的 [restore sending on current QQ builds](https://github.com/SnowLuma/SnowLuma/commit/44bbaaa1bb44541cbd0a0bc8f425b46896952c94) 均替换六份 native 文件。它们证明原生维护/发布是策略组成部分；未公开内部补丁，不能推断具体修改了哪条签名或 ABI。固定公开源码的更新检查提供发行链接，由用户手动更新，没有证据表明它自动下载新原生适配配置。

详细证据与限定范围：[版本适配报告](E:/stella/_reference/qq-native-r2-20261008/reports/snowluma-version-adaptation.md)、[官方历史与锚点索引](E:/stella/_reference/qq-native-r2-20261008/reports/snowluma-version-adaptation-evidence-index.md)。

## 8. 对 Caligo 计划的修订建议

这是新证据提出的工作顺序，不代表已经实现；原 K4 计划和上一轮修改保留，本轮没有覆盖它们。继续使用官方 QQ 提供登录/会话/网络，自有 Rust 内核负责观察、受合同约束的调用、生命周期与 IPC，后续再实现 OneBot V11；运行时不依赖 SnowLuma 二进制或第三方 QQ 后端。

| 顺序 | 操作与交付物 | 通过条件 | 失败分支 |
|---|---|---|---|
| R3-A 原生目标闭合 | 定向找 service `+0x60` 安装链及 transport vtable `+0x38`；分别恢复 TLS/Core executor slot 0、回调触发与 stop/cancel/drain | 每个间接跳点有对象来源、实际目标、线程/重入/所有权证据 | 静态无法闭合则记录精确缺口，设计独立受控观测；不宣称线程安全 |
| R3-B 生命周期合同 | 恢复 getter handle 的 disposer/refcount；callback move/destroy；detach 与任务排空；形成 ABI/拥有权表 | 每个拥有引用可平衡释放；关闭后不再调用已释放回调；异常不跨未知边界 | 清理无法证明则禁止热卸载，保留 Quarantined 规则 |
| R3-C 当前 build profile | 固定文件身份、类/子对象/入口/callback/allocator，按 receive/send/lifecycle 分开能力状态 | known module identity + 各能力结构检查通过，线程合同成立后才允许写操作 | 新 hash 默认 unsupported；可做只读调查并产出新 profile，不能因日志匹配直接启用发送 |
| K4 现场验收 | 重新冻结 Host/Session/Connection 代次；单一固定实例，先观察，再真实私聊/群聊收发和恢复 | G2/G3 各 10 私聊+10 群聊可追溯样本，G4 三次重启/重连；超时写入保持 delivery_unknown | 失败按身份/原生能力/调度/IPC/业务凭据分层留证，不推断“只剩一个问题” |
| K5 OneBot V11 | 在已验收内核上做 action/event、receipt 和错误映射 | OneBot 请求可关联到真实 QQ 结果，重连不跨代次重放未知写入 | 内核门槛未过则不以协议绿灯替代原生验收 |

不要在本轮之后继续广撒扫描或增加无目标现场注入。最有效的下一步是 R3-A/R3-B：**把已定位的具体间接调用与释放链补齐**。版本适配是一项持续维护能力：每次 QQ 更新都需固定新样本、重新验证 profile、有限受控验收，再准入能力；不能复制 SnowLuma 的一组字段后承诺永久兼容。

## 9. 当前通过与未通过

通过：固定文件与被动加载版本一致；313 个不同函数的定向导出；两类 RTTI/构造布局；服务与身份 getter 的选择理由；固定版本输入复制、callback 移动/消费和两条收包拥有权结构；SnowLuma 适配机制与原生维护历史；当前标签两份 Windows 原生文件摘要比较。

未通过：实际服务分支/账号/Session ready；transport/executor actual target 和线程准入；取消/断开/排空/恰好一次 callback；完整 handle disposer；跨 QQ 版本 ABI 验收；真实聊天收发与恢复。**结论为研究阶段部分契约已闭合，原生线程与生命周期门槛仍未通过。**

复核入口：[验证结果](E:/stella/_reference/qq-native-r2-20261008/validation.json)、[产物摘要清单](E:/stella/_reference/qq-native-r2-20261008/artifact-manifest.json)。原始材料集中于独立 evidence 目录；仓库本轮仅新增此研究报告，未修改业务实现。
