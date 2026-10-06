# JS 上下文获取方案评估(js-context-acquisition)

> K2 起点的方案决定(计划 §3.3/K1 契约 §4.4)。冻结于 2026-10-06。
> 问题:bridge 运行在 QQ 主进程内,但 QQ 的 Node Environment(V8 上下文)由 QQ 自建;
> 要枚举/调用 `major` 的服务面(nodeIKernel*),必须先取得一个可用的 `napi_env`。

## 0. 已核对的可行性事实 [verified]

- QQNT.dll 导出**完整 Node 环境创建栈**:`node::NewIsolate`(两重载)、`node::NewContext`、
  `node::CreateEnvironment`(两重载)、`node::LoadEnvironment`(见 local-evidence/k1-exports-qqnt-dll-full.txt
  ord 1063/1064/1827/1955/1974/1975)。注意这些符号使用 `__Cr::std`(Chromium libc++)命名,
  与业务层 `__qq::std` 不同——运行时层是 libc++。
- 注册链路已实证:qq_magic_napi_register(napi_module*) → 堆分配 node_module → 挂入 `_linked` 链表;
  context_register_func 经共享 thunk(RVA 0x01C8B430)转到 napi_module.nm_register_func。
- major 的 napi_module 实测模式:nm_version=1、nm_flags=2、三个指针字段;node.flags 观测=2(NM_F_LINKED)。

## 1. 候选方案

| 方案 | 描述 | 评估 | 决定 |
|---|---|---|---|
| a) 注册自有 linked binding,等待新 Environment | 挂入 `_linked` 后,若 Node 对每个新 Environment 急切调用全部 linked 绑定,回调即送来 napi_env | 是否急切调用**未知**——正是 EXP-K2-00 要测的;主进程已存在,新 Environment 频率未知 | **先测(EXP-K2-00)** |
| b) 复用 qq_magic_napi_register 注册自有 napi_module | 与 a 相同的注册机制,显式调用 QQ 导出完成注册 | 机制已解码(node_module_register = 链表插入);QQ 不会主动请求我们的绑定名 → 若注册是惰性的,回调永不触发 | **作为 a 的注册手段** |
| c) 扫描已运行 Environment(内存挖掘) | 在主进程内存中定位主 Environment/V8 isolate | 侵入性强、结构随版本漂移、误读即崩 | **拒绝**(除非 a/d 全部失败) |
| d) 自建独立 Node 环境(导出 API 路线) | NewIsolate → NewContext → CreateEnvironment → LoadEnvironment,在自己的 uv_loop 上运行自己的 JS 引导,在自有 env 内 `process._linkedBinding('major')` | 全部所需 API 均已导出;不触碰 QQ 的 env、不改内存;自带线程与生命周期(契合计划 §3.1);风险点:major 的服务实现可能假设主 Environment 状态(app/userData/IPC)——需实验验证服务在自建 env 中能否初始化 | **主路线(K2 生产候选)** |
| e) inspector/CDP 观测 | 以 `--remote-debugging-port` 等启动参数重启 QQ,经 DevTools 协议枚举 major 导出面 | 零注入、零内存风险的**调查工具**;但依赖启动参数(需重启 QQ、QQ 可能剥离参数);不适合生产 | **调查辅助(待实验)** |

## 2. EXP-K2-00 结果(2026-10-06,已执行)

- 注册调用本身成功(qq_magic_napi_register 返回正常,QQ 存活),但观测揭示**关键架构事实**:
  - QQNT.dll 有**两条 node_module 链表**:
    - **表 A** @ RVA 0x0C7092EA:`node_module_register`(qq_magic 尾跳目标,实测解码 0x01C8F5EB)维护的
      通用 addon 注册表——我们的节点进了这张表;
    - **表 B** @ RVA 0x0C7092F0:`get_linked_module` 读取的 linked-binding 查找表(major/electron_* 所在,
      我们 obs 走的就是它)。
  - 因此经 qq_magic 注册的绑定**不会**出现在 `process._linkedBinding` 的查找域,回调永不触发
    (entry_fired=false,实测);EXP-K2-00 对"急切 or 惰性"的回答是:**两者都不是——是另一张表**。
  - node_module 结构体为堆分配(qq_magic 内 new),进表 A 的节点不可摘除,随进程退出回收。
- **结论:方案 b 作为 env 获取路线失败(查不到);但作为机制验证完全成功**(注册调用、结构布局、
  链表写入全部按静态解码预期工作)。

## 3. 修订后的路线:方案 d(自建 Environment)细化

自建环境所需的全部 API 均在 QQNT.dll 导出面 [verified]:

| 步骤 | 导出 | 备注 |
|---|---|---|
| 1. 平台 | `node::CreatePlatform(int, v8::TracingController*)`(ord 1071) | TracingController 参数可否为 null 待实验;或复用 QQ 的实例 |
| 2. 事件循环 | `uv_loop_init` / `uv_default_loop`(ord 3096/3212) | 标准 libuv |
| 3. Isolate | `node::NewIsolate`(两重载,ord 1974/1975) | 需 ArrayBufferAllocator(查 `CreateArrayBufferAllocator` 导出) |
| 4. IsolateData | `node::CreateIsolateData`(ord 1067)/ `FreeIsolateData`(1241) | |
| 5. 上下文 | `node::NewContext`(ord 1955) | |
| 6. 环境 | `node::CreateEnvironment`(ord 1063/1064) | 参数含 libc++ `std::vector<std::string>`(空 vector 可零构造)与 EnvironmentFlags |
| 7. 注册绑定 | `node::AddLinkedBinding`(ord 803,napi_module 重载) | **把我们的 napi_module 挂进自建 env**——这解决 lookup 域问题 |
| 8. 启动 | `node::LoadEnvironment`(ord 1827) | std::function 参数的 ABI 需 shim;或走 StartExecution 字符串重载(待查) |
| 9. 取得 env | 引导 JS 调 `process._linkedBinding('caligo_bridge')` → 我们的 register_func 收到 **napi_env** | 之后请求 `process._linkedBinding('major')` 枚举其导出面 |

- 关键收益:自建 env 是**我们自己的线程与 uv_loop**,QQ 敏感回调零占用(计划 §3.1);major 的服务
  能否在非主 env 中初始化是 EXP-K2-01 的核心实验问题。
- `__Cr::std`(libc++)与 `__qq::std` 的区别:Node/Electron 运行时层用 libc++;调用导出函数时
  需按 libc++ ABI 构造少量参数对象(vector/function),此为 EXP-K2-01 的主要工程量。
- 旧结论修正:两处静态解码笔误已修正(qq_magic 尾跳目标 0x01C8F5EB;链表头 0x0C7092F0 的
  手工算术此前有 ±2 误差,以运行时 pattern 推导为准)。

## 5. EXP-K2-01 结果(2026-10-06,已执行,**崩溃级负结果**)

- **成功部分**:阶段化执行器按设计工作——platform/uv_loop/allocator 三个阶段都在 QQ 主进程内
  自有线程上**创建成功**(live 报告 local-evidence/k2-01b-pid49148-env.jsonl);第一轮还暴露并修正了
  两处修饰名笔误(NewIsolate 的 `VCppHeap` 前缀、Context::Global 的 const 限定)。
- **负结果**:`node::NewIsolate` 调用**崩溃了 QQ 主进程**(实例 49148 及其子进程终止;第二实例 38472
  未被触碰、健康存活;无 WER 转储——QQ 自有 crashpad 吞掉)。阶段 JSONL 精确定位崩溃点:
  报告停格于 allocator 之后、"new_isolate" 行之前。
- 根因候选(按可能性排序,未再实验验证):
  1. `CreatePlatform(2, null)` —— null TracingController:返回了非空 platform,但其内部不完整,
     首次创建 isolate 时 tracing 路径解引用空控制器;
  2. 零块 IsolateSettings(4 KiB)不满足该 Electron 构建的不变量(settings 含非 POD 字段,
     零值选择到不支持路径);
  3. CppHeap 空 unique_ptr 对此重载非法。
- **过程修正记录**:此前文档两处静态解码笔误已勘误(qq_magic 尾跳目标 0x01C8F5EB;
  手工算术的链表头 ±2 误差以运行时推导为准)。
- **结论**:方案 d 的朴素实现(零块 settings + null controller)被判负。继续深挖 ABI
  (解码 IsolateSettings/构造 TracingController)成本高且仍在 QQ 内玩火。

## 6. 修订后的路线:方案 e(CDP/inspector)升级为主调查路线

- 理由:**零进程内风险**(不注入、不创建 v8),直接在 QQ 的**真实主 Environment** 里枚举 major
  服务面的方法名与签名——这正是 K2 需要的证据;且 DevTools 协议输出可直接回填 §6.1 契约。
- 需要执行者配合:以调试参数重启 QQ(见 recovery-notes;仅影响新实例,不改安装)。
  候选参数(依次尝试,QQNT 基于 Electron 40):
  1. `QQ.exe --remote-debugging-port=9222`(Electron 主开关,最可能生效);
  2. 若被剥离:`set ELECTRON_ENABLE_LOGGING=1` + `--remote-debugging-port`;
  3. 若仍无效:调查 QQ 是否白名单化启动参数(需重新评估)。
- 方案 d 保留为长期候选,重启条件:完成 IsolateSettings/TracingController 的 ABI 解码,
  或拿到 QQ 自身 platform/isolate 的复用通道;进入任何再次"进程内创建"实验前先做方案变更记录。

## 7. 决定(修订 v2)

1. 方案 b(机制组件)保留;~~单独 env 路线~~判负(双链表);
2. 方案 d 朴素实现判负(NewIsolate 崩溃,见 §5);冻结,重启条件见 §6;
3. ~~方案 e(CDP)= 当前主调查路线~~ → **实测判死(2026-10-06,§8)**;
4. 方案 c(内存扫描)维持拒绝。

## 8. 方案 e 实测记录(2026-10-06,判死)

| 尝试 | 结果 |
|---|---|
| `--remote-debugging-port=9222` | 参数进入主进程 argv(实测确认);**9222 从未绑定**;QQ 主进程 9210/9211 端口是其内部 JWT 服务(响应 `errCode:4001 请求数据格式错误`),非 CDP → 应用层主动禁用 |
| `--inspect=9229` | 参数进入主进程 argv(实测确认);9229 关闭 → **`EnableNodeCliInspectArguments` 熔断被编译期禁用** |

- 证据:local-evidence/k2-cdp-port-hijack.txt。
- 结论:腾讯对调试后门做了系统性关闭(应用层 + 熔断位双层)。命令行注入路线全部关闭。

## 9. 决定(修订 v3,当前有效)

**主路线转为 F-1:wrapper.node C++ ABI 离线解码** —— 完全离线、零 QQ 交互风险,且直接服务
五类入口(native-entry-contract E-3 本就是候选):

1. 解码 `__qq::std::string` 内存布局(wrapper 导出函数的反汇编可实证:SSO 判定位、容量字段);
2. 解码 `CreateNTSessionShell(const string&) -> shared_ptr<INTCSessionShellBase>` 的返回对象布局
   (vtable → 接口方法表,即 C++ 层的会话/消息入口面);
3. 以解码结果回填 E-3 的 §6.1 契约字段;ABI/线程/生命周期闭合后才进入任何调用实验。

备选(按序,未激活):F-2 内存扫描定位主 Environment(侵入性强,维持拒绝倾向);
F-3 A2 协议路线(计划 §3.2 分支,需 A1 结论先行)。
方案 d 复活条件(不变):完成 IsolateSettings/TracingController ABI 解码并作方案变更记录。

## 10. K2-02 路线细化(F-1 终局后,2026-10-06;执行结果与勘误见 §11)

F-1 五轮证明 wrapper 壳接口无消息业务后,到达 nodeIKernel* 服务面的通道收敛为:

**路线 R-A:主进程 Environment 发现 + RequestInterrupt(优先)**
1. 离线:QQNT.dll 内做 MSVC RTTI 扫描,定位 `node::Environment` 的 vtable RVA
   (类型描述符串 `.?AVEnvironment@node@@` → 完整对象定位器(COL) → vtable[-1]);
   需给 CLI 加文件偏移→RVA 反向映射或字节模式搜索;
2. obs 内存扫描(全部经 checked_read,崩溃免疫):在可读区域扫描指向
   `qqnt_base + vtable_rva` 的指针 → 候选 Environment 对象,辅以字段合理性校验;
3. `node::RequestInterrupt(env, callback, ctx)`(ord 2070,已确认导出)让回调在
   QQ 的 JS 线程上安全执行 —— 该上下文内枚举 major 服务面;
4. 风险:扫描只读且崩溃免疫;RequestInterrupt 回调上下文的保证需实验确认。

**路线 R-B:自建 Environment(维持冻结)**
- 已确认 `GetCurrentPlatform` **未导出**(无法复用 QQ 的 platform);
- 解码 IsolateSettings + 构造 TracingController 成本高且带实例崩溃风险;
- 维持冻结,除非 R-A 失败。

R-A 的第 1-2 步完全离线/只读,是下一步的开工点。

## 11. K2-02 执行结果(2026-10-06,R-A 三步全部完成,当前有效)

### 11.1 第 1 步(vtable 发现)——RTTI 路线判负,ctor 推导路线成立

- **勘误**:QQNT.dll / wrapper.node / major.node 均无 `.?AVEnvironment@node@@` 字节串
  (grep -aob = 0)。Chromium 系构建 `/GR-` 关闭 RTTI;§10 与此前会话记录的
  "5 个 RTTI 引用"实为**函数签名字符串**中的 `VEnvironment@node@@` 子串。
- 替代路线(全离线反汇编,证据 local-evidence/k2-02/):CreateEnvironment(ord 1063)
  → 尾调 ord 1064 → `operator new(0xB60)`(sizeof(Environment)=0xB60)→ ctor 0x1C44810
  → `lea rax,[0xA804990]; mov [rcx],rax`。
- **node::Environment vtable 图谱**(QQNT 9.9.33-52230):主(完整对象)vtable
  **RVA 0x0A804990**(@+0x0);子对象 vtable 0xA8049D0@+0x540、0xA804A10@+0x5B8、
  0xA804A90@+0x9F0、0xA804B10@+0xA60 —— 恰 5 个 vtable,多重继承五基类。

### 11.2 第 2 步(内存扫描)——外部 ReadProcessMemory 实现(对 §10 的有意偏差)

- §10 原文写"obs 内存扫描"(进程内 bridge);实际实现为 **CLI `envscan` 子命令,
  外部 RPM 零注入**——坏读只是 RPM 调用失败,绝不触发目标 AV,是 checked_read
  语义的严格超集。若未来 RPM 被拒绝再回退进程内方案。
- 扫描 PID 9000(指定测试实例,F1 轮同款):**唯一命中 0x762C002AD800**,
  5 个 vtable 命中相对偏移与 ctor 图谱完全一致(±0 一致性判据通过);
  +0x2C 处 1.0f 与 ctor `mov [rcx+2Ch],3F800000h` 吻合 → 真 Environment,非巧合。
- 扫描覆盖 0.73 GiB(image 618MB + private 177MB),1 秒级完成,read_failed=0。

### 11.3 第 3 步(RequestInterrupt)——通过

- 干跑(env=0)验证链路后实弹:`RequestInterrupt(env, cb, null)` 在
  **≤10ms 内回调触发**(fired=1,回调线程 id 55832 = QQ 的 JS 线程),
  实例存活,env 页后验可读,Environment 地址稳定。
- 回调侧只做原子写(tid/tick/fired),遵守计划 §3.1。
- vfptr 核对门有效:expected_vftable 不匹配时拒绝调用(intr_code=6)。
- 口径闭合:QQNT 运行时基址 0x7FF919120000(Toolhelp)与 vfptr−0xA804990 一致;
  RequestInterrupt 地址 0x7FF91ACD24A0 = base + RVA 0x01BB24A0(与 K1 导出表一致)。

### 11.4 结论与下一步

- R-A 的可行性别(发现 → 定位 → 注入回调)三层全部实证。主 Environment 的
  **原生指针获取通道**已闭合:`envscan`(离线 vtable 图谱 + 外部扫描)+
  `RequestInterrupt`(JS 线程执行点)。
- 下一步(K2-03 候选):在回调上下文内安全地执行 JS/枚举 major 服务面。
  RequestInterrupt 回调是原生中断点,在其中直接调 V8 需要HandleScope 且时机敏感;
  候选设计:回调内只投递一个安全载荷(如向 env 的 uv_loop 提交 async 工作),
  在 Node 自己的轮转点上运行枚举——设计前先记录方案变更。
- 版本绑定:0xA804990 等全部 RVA 绑定 manifest 冻结版本(9.9.33-52230),换版本重测。

## 12. K2-03:主 Environment 事件循环点载荷(2026-10-06,mode 0/1 实证,mode 2 事故归因闭合,mode 3 待实弹)

方案变更(记录于 local-evidence/k2-03/design.md):§11.4 原计划"RequestInterrupt
回调内枚举"修订为 **uv_async 载荷**——RequestInterrupt 保留为 env 身份验证器
(K2-02),枚举载荷改走主 env 自己的 uv_loop 轮转点(计划 §3.1 允许的执行点)。

### 12.1 静态解码(全离线,证据 local-evidence/k2-03/design.md)

- **uv_loop 链**:`env+0xB0 → IsolateData*(ctor 参数2);IsolateData+0x11E8 → uv_loop_t*`;
  `env+0xA0 → Isolate*`;sizeof(IsolateData)=0x1258。
- **ScriptOrigin 布局(0x28)**:+0x0 name、+0x8 line、+0xC column、+0x10 选项位域、
  +0x14 script_id、+0x18 source_map_url、+0x20 host_defined_options;零构造安全
  (ctor 尾调用在 host_defined 为空时短路)。
- JS 执行面导出齐备:GetCurrent/GetEnteredOrMicrotaskContext/GetIncumbentContext/
  HandleScope ctor+dtor+CreateHandle(1066)/Script::Compile(1026)/Run(2090)/
  String::NewFromUtf8(1965)/Utf8Value(239/356/729)。

### 12.2 实弹结果(PID 9000,指定测试实例)

- **mode 0 PASS**:布局链实测 IsolateData=0x762C0037C000、loop=0x7FF925AD7670
  (落在 QQNT 映像 .data 内,符合 node 主循环为模块内静态的预期),uv_loop_alive=1。
- **mode 1 PASS**:uv_async_init/send r=0,回调 **0ms 内触发,线程 55832** ——
  与 K2-02 RequestInterrupt 回调同线程(QQ 的 JS 线程),跨机制互证。
  "载荷跑在 env 自己的事件循环轮转点上"成立。
- **mode 2 事故(进程死亡,归因闭合)**:回调延迟 ~15s 后运行,在
  `GetEnteredOrMicrotaskContext` 内部 AV,实例 9000 死亡(crashpad 90bb0b36,已归档)。
  根因:**v8 Local/MaybeLocal 非平凡返回的 sret ABI**(详见 12.5)。JSONL 原子快照
  (isolate_match=1/has_ctx=0/RESULT_SEQ 未置位)与崩溃时序完全自洽。
- 事故后处置:未触碰任何存活实例;修复入库;复验需执行者重新指定测试实例。

### 12.2.1 第二轮实弹(PID 28532,2026-10-06;第二起事故,同根因,归因闭合)

- mode 0/1 全 PASS(新实例,env=0x65CC002AD800,布局链/回调线程 25096 互证)。
- mode 2(sret-v1 修复版)存活但 **has_ctx=0 为污染输出**:成员函数参数序仍反,
  返回值误写进 [isolate+0](静默腐蚀)。
- mode 3 的挂起中断在下一次 JS 执行时触发捕获回调,`[栈+0x100D0]` 不可读 →
  **AV@QQNT+0x429A081,实例死亡**(crashpad 82337034,已归档)。
- **最终 ABI 规则(全部反汇编实证,证据 local-evidence/k2-03/run-notes-round2.md)**:

| 函数 | 类型 | 约定 |
|---|---|---|
| `String::NewFromUtf8` | 静态 | (sret=RCX, isolate=RDX, data=R8, type=R9d, len=栈) |
| `Script::Compile` | 静态 | (sret=RCX, ctx=RDX, src=R8, origin=R9) |
| `Script::Run` | 成员 | (this=RCX, sret=RDX, ctx=R8, data=R9) |
| `GetEnteredOrMicrotaskContext` / `GetIncumbentContext` | 成员 | (isolate=RCX, sret=RDX) |

- **Run 的单参导出是裸跳板**(直跳双参本体不准备 R9)——必须调用双参重载,
  data 传空 Local。修复入库后待第三轮实弹复验。

### 12.3 第三轮实弹(PID 9328,2026-10-06):**主 Environment 内 JS 执行通道闭合**

- 修正 ABI 后 mode 2 全通:**uv 回调点上 `GetEnteredOrMicrotaskContext` 真实返回
  主上下文(has_ctx=1)**——前两轮的 0 均为反序调用造成的假象/崩溃。枚举脚本在
  QQ 主 Environment 内执行并取回结果,post_check=true,实例存活。
- **指纹证据(r3-mode2b)**:node 24.11.1 / v8 14.4.258.16-electron.0 /
  electron 40.0.0 / chrome 144;`process._linkedBinding('major')` = `{ load: [native] }`
  ——服务面在 `load()` 之后惰性展开;**require 在该上下文为 undefined**(非全局),
  process/Buffer/console 等标准全局在位。
- **mode 3(中断点捕获)降为备用方案**,本轮未执行——mode 2 的轮转点已足以取得
  entered 上下文。注意:RequestInterrupt 在空闲 QQ 上可挂起数分钟(JS 执行间隙
  才被处理),若启用 mode 3 需 ≥120s 捕获窗。
- **下一实验档(未开工,需方案记录)**:调用 `major.load(...)` = 首次调用 QQ 业务
  函数,属 test-scope §3 的 K2 样本纪律范围,须独立设计与门控后进行。
- 实例存活复验:envscan 复扫 1 命中,地址稳定;两枚惰性 uv_async 句柄遗留
  (不 close,随进程退出回收,如实记录)。

### 12.4 纪律

- 回调内零 IO/零分配/零锁(静态原子 + 预分配缓冲);uv_async 句柄不 close,
  随进程退出回收,如实记录。
- 所有偏移版本绑定 9.9.33-52230;换版本重解。
- 事故台账:9000(2026-10-06 10:53 启动,20:26 死于 sret 缺陷)记入
  local-evidence/k2-03/run-notes.md;crashpad 原件归档同目录。

## 3. 纪律与生命周期

- `caligo_bridge` 注册**不可逆**(node_module_register 只有插入):随测试 QQ 进程退出回收;
  注册动作必须经 inject 的显式门控(--register-entry)。
- 重复注入多份 bridge 会产生多个同名链表节点(每份 DLL 一个);obs 报告如实呈现,不为去重撒谎。
- 回调内禁止任何 IO/长操作:本轮仅原子写;未来在回调内也只做"记录 env + 唤醒自有线程",
  遵守计划 §3.1(QQ 敏感回调内不做重活)。
- 版本绑定:qq_magic_napi_register 的使用绑定 manifest 冻结版本;换版本重新评估。
