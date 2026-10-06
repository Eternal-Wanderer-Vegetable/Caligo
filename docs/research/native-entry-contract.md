# 原生入口契约(native-entry-contract)

> K1 交付物。状态:静态调查完成(2026-10-06),**实测验证未开始**。
> 标签沿用计划约定:`[verified]` 本机已核对;`[inferred]` 证据形成的工程判断;`[assumed]` 必须实测验证。
> 证据文件在 `local-evidence/`(不入库):k1-exports-*.txt(导出表全量)、k1-probe-process.txt(进程快照)。

## 0. 调查方法与证据边界

本轮证据 = 只读静态观察(PE 导出表、文件元数据、asar/package.json 字符串)+ 运行中进程模块快照 + 公开资料线索(S13,仅作假设来源)。
**没有**任何运行时 Hook、注入实验或消息收发。因此本契约的每个候选都处于"已定位、未验证"状态;
`[assumed]` 项在实测前一律不得进入发送调用(计划 §6.1 纪律)。

## 1. 版本与进程身份(适配基础)

| 项 | 值 | 标签 |
|---|---|---|
| 冻结版本 | 9.9.33-52230(aff854e8),见 `version-adapter-manifest.json` | [verified] |
| 运行时层 | QQNT.dll = Node.js 运行时(导出 3385 个符号,含全套 `napi_*`;二进制内含 `Electron/40.0.0` 标识) | [verified] |
| 启动器 | QQ.exe(FileVersion 9.9.33.52230)不等于运行内核;实际版本以进程加载的 versions/9.9.33-52230 目录为准 | [verified] |
| 主进程判定 | 同时加载 wrapper.node + major.node + qq-proton.node + ipc.node + initIpc_x64.node 的进程为主进程候选(2026-10-06 观察:PID 19352、27992) | [verified](当日快照) |
| 渲染层特征 | major.node 同时出现在多个渲染进程中;wrapper.node 仅出现在主进程候选 | [verified](当日快照) |
| 第三方协议端 | 当日模块快照未见 NapCat/LLOneBot 式第三方 .node | [verified](当日快照) |

## 2. 候选入口清单

### E-1 Node 运行时层(QQNT.dll)

- **职责** [inferred]:提供 Node.js/Electron 运行时;所有 .node 模块经它加载与注册。
- **可静态确认的导出** [verified]:
  - 全套 Node-API(`napi_*`),含 `napi_create_threadsafe_function` 等跨线程回调原语;
  - `node::AddLinkedBinding`(四个重载,含 `(Environment*, const char*, napi_addon_register_func)`);
  - `node::binding::get_linked_module(const char*)`;
  - **`qq_magic_napi_register`** —— 腾讯自有导出,语义未知,疑似 major.node/initIpc_x64.node 等零导出模块的注册通道;
  - `uv_dlopen`(libuv 动态加载)。
- **§6.1 契约字段**:
  - 取得方法:`GetModuleHandleW("QQNT.dll")` + `GetProcAddress` 即可取得全部符号 [inferred,机制标准];是否允许第三方调用无契约保证 [assumed];
  - 调用约定/参数/返回:Node-API 部分遵循公开 napi 标准(S2) [verified-by-standard];`qq_magic_napi_register` 完全未知 [assumed];
  - 线程/重入:Node-API 要求在拥有 `napi_env` 的上下文调用;外部注入线程直接调用未定义 [assumed];
  - 生命周期:随 QQ 进程 [inferred];
  - 账号/会话确认:本层不提供 [verified-by-absence];
  - 错误行为/版本拒绝:未知 [assumed]。
- **未知项**:`qq_magic_napi_register` 签名与语义;从外部加载的 DLL 获取有效 `napi_env` 的途径;AddLinkedBinding 在启动完成后是否仍可安全调用。

### E-2 业务 JS 模块(major.node)

- **职责** [inferred]:核心业务(账号、会话、消息)所在的 JS 原生模块;与公开资料(S13)描述一致,但**职责本身尚需实测确认**。
- **关键静态事实** [verified]:**PE 导出表为空**(0 named / 0 total)。
- **§6.1 契约字段**:
  - 账号状态/会话取得/消息订阅/发送/结果五类入口:**静态不可见** [verified-by-absence]。注册只能发生在运行时(经 E-1 的 `qq_magic_napi_register` 或 node_module 构造链) [assumed];
  - 其余字段全部未知 [assumed]。
- **结论**:A1 的五个入口契约在 E-2 上**无一可静态闭合**;必须通过运行时观察(JS 内省、napi 注册拦截、或 E-1 实验通道)取得候选名,再回填本契约。

### E-3 会话壳 C++ 导出(wrapper.node)

- **职责** [inferred]:NT 会话壳/业务包装层,与渲染层桥接。
- **可静态确认的导出** [verified](66 个):
  - `wrapper::nt::INTSessionShell` / `wrapper::nt::IGProSessionShell` 类:构造、析构、vtable;
  - 静态工厂 `CreateNTSessionShell(const __qq::std::string&) -> __qq::std::shared_ptr<nt::ntc::INTCSessionShellBase>`;
  - OpenCV `cv::Mat` 部分方法;llhttp_* 全套 HTTP 解析器。
- **§6.1 契约字段**:
  - **ABI 警戒** [verified]:导出签名使用腾讯自建 STL 命名空间 `__qq::std`(如 `__qq::std::shared_ptr`、`__qq::std::basic_string`),不是 MSVC 标准 STL 布局;任何跨语言调用都要先确认其 string/shared_ptr 的内存布局与构造方式 [assumed];
  - 对象拥有者/有效期:未知 [assumed];
  - 调用线程:未知 [assumed]。
- **结论**:ABI 与对象生命周期未确认 → 按计划纪律**不得**进入发送调用。仅登记为候选。

### E-4 跨进程 IPC 模块(ipc.node)

- **可静态确认的导出** [verified](32 个):djinni 生成接口 `gen::djinni::{ChildIpcInterface, ParentIpcInterface, IpcListener, EchoCallback, LaunchProcessCallback, IOperationCallback}` 等。
- **职责** [inferred]:QQNT 主/渲染进程间 IPC 桥。
- **§6.1**:序列化格式、附着方式、线程上下文均未知 [assumed]。作为 A2(协议边界)调查线索登记。

### E-5 其余 .node(qq-proton.node、initIpc_x64.node)

- **qq-proton.node** [verified]:标准 Node-API 注册形态 —— 导出 `napi_register_module_v1` + `node_api_module_get_api_version_v1`。这证明本版本 QQ 的 addons 走标准 napi 注册通道的**存在性**;
- **initIpc_x64.node** [verified]:导出表为空(同 major.node 形态)。
- 均未确认与消息链路的关系 [assumed]。

## 3. 五类入口(账号/会话/接收/发送/结果)的覆盖状态

| 计划要求的入口 | 当前候选 | 状态 |
|---|---|---|
| 账号状态 | E-2(JS 绑定,名字未知)/ E-1 qq_magic_napi_register | 候选未验证 |
| 会话取得 | E-3 INTSessionShell(C++ ABI,未确认)/ E-2 | 候选未验证 |
| 消息订阅 | E-2 / E-4 | 候选未验证 |
| 发送 | E-2 / E-3 | 候选未验证;E-3 被 ABI 警戒阻断 |
| 发送结果关联 | 无候选 | 空缺 |

**A1 关卡判定(G1)**:不通过 —— 五类入口契约无一实测闭合。缺的不是"能不能加载"(加载机制已就绪,见 loading-route-decision),而是"加载后调用什么"。下一步是运行时观察实验,不是写发送代码。

## 4. 下一步实测计划(回填本契约的唯一途径)

1. 在指定测试实例上加载 K1 probe(只读),验证加载链与握手(补齐 §2 E-1 的线程/生命周期字段);
2. 观测实验:在 probe 内经 E-1 导出(优先尝试 `qq_magic_napi_register` 语义确认;不行则 `napi_*` 环境探测)取得 JS 运行时可见面,枚举 major.node 已注册的绑定名 —— 只枚举,不调用;
3. 以枚举结果回填 §3 表格,再逐个填 §6.1 契约;ABI/线程/生命周期闭合前不触碰发送。

## 5. 公开资料线索登记(S13,全部未验证)

| 来源 | 声称 | 本轮核对结果 |
|---|---|---|
| [go-cqhttp issue #2471](https://github.com/Mrs4s/go-cqhttp/issues/2471)(抓取 2026-10-06) | wrapper.node 在 major.node 内加载;hook require 不可行,需经 process 方法 | 未验证;与"E-2/E-3 均在主进程"观察不冲突 |
| [解析NTQQ数据库(lengyue.me)](https://lengyue.me)(抓取 2026-10-06) | wrapper.node 含 `nt_sqlite3_key_v2` 等数据库密钥函数 | **本版本(9.9.33-52230)导出表中不存在该符号** [verified-by-absence];该文针对旧版本,不得按旧版本外推 |
| [NapCat 文档](https://napneko.github.io/guide/napcat)(抓取 2026-10-06) | NapCat 调用 Electron IPC 之下的 Node 原生模块接口 | 方向与 E-2 判断一致 [inferred];实现细节不复用、不采信 |
