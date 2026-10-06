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

## 4. 决定(修订)

1. ~~方案 b 单独作为 env 路线~~ → 失败(表 A 不可查),保留为机制组件;
2. **方案 d = 生产主路线**(EXP-K2-01 实施),配合 AddLinkedBinding 把自有绑定挂入自建 env;
3. 方案 e(inspector/CDP)保留为调查加速器,需执行者以特定启动参数重启 QQ 时另行请求;
4. 方案 c(内存扫描)维持拒绝。

## 3. 纪律与生命周期

- `caligo_bridge` 注册**不可逆**(node_module_register 只有插入):随测试 QQ 进程退出回收;
  注册动作必须经 inject 的显式门控(--register-entry)。
- 重复注入多份 bridge 会产生多个同名链表节点(每份 DLL 一个);obs 报告如实呈现,不为去重撒谎。
- 回调内禁止任何 IO/长操作:本轮仅原子写;未来在回调内也只做"记录 env + 唤醒自有线程",
  遵守计划 §3.1(QQ 敏感回调内不做重活)。
- 版本绑定:qq_magic_napi_register 的使用绑定 manifest 冻结版本;换版本重新评估。
