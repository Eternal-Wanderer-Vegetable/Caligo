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

### E-2 业务 JS 模块(major.node)— **2026-10-06 EXP-K1-02 实测更新**

- **注册形态 [verified]**:以 linked binding 注册进 node_module 进程级链表——name=**"major"**,flags=2(NM_F_LINKED),version=-1(NODE_MODULE_VERSION 语义的 -1,即 context-aware 注册);其余 44 个链表节点全部为 Electron 内置绑定(version=143)与 `QQNT`。
- **JS 获取路径 [inferred]**:主进程 JS 经 `process._linkedBinding('major')` 取得其导出面(Node/Electron 对 NM_F_LINKED 模块的标准访问通道;`get_linked_module("major")` 可命中已实证)。
- **构建溯源 [verified]**:链表节点 filename 字段暴露 `E:\data\landun\workspace\rx64\v8-bytecode-unified\node\electron_loader.cpp` —— major 由腾讯蓝盾 CI 构建,内置进 Electron loader 流程(v8-bytecode-unified 与 package.json `isByteCodeShell:true` 相互印证)。
- **wrapper/qq-proton/initIpc 的差异 [verified]**:三者已加载但**不在** linked 链表 → 走 per-Environment DLOpen 注册,进程级内置面只有 QQNT + major + Electron。
- **五类入口的服务面已枚举 [verified 2026-10-06 补充]**:对 major.node 做标识符字符串挖掘(229,199 条,
  local-evidence/k1-02-major-identifiers.txt),得到 QQNT 内核 JS 服务面清单:
  - 服务:`nodeIKernelMsgService`(165 处引用,消息收发)、`nodeIKernelLoginService`(账号)、
    `nodeIKernelProfileService`、`nodeIKernelBuddyService`(好友)、`nodeIKernelGroupService`(群)、
    `nodeIKernelSessionService` 对应的 `nodeIKernelSessionListener`、`nodeIKernelRecentContactService`、
    `nodeIKernelMSFService` 等 30+ 服务;
  - 监听:`nodeIKernelMsgListener`(56 处,消息推送)、`registerNTListener` 等;
  - 发送相关内部事件名出现在 wrapper.node(C++ 层):`kNTOnAddSendMsg`、`kNTNotifyOnRecallRichFileAfterSendMsg`。
- **分层结论 [inferred,多项实证支撑]**:JS(字节码)→ major.node 绑定(nodeIKernel* 服务访问面)→
  C++ IKernel 实现(wrapper.node / QQNT.dll,MSVC ABI,`__qq::std`)→ NT 内核。五类入口的
  **服务粒度候选**由此确定:账号=LoginService/ProfileService;会话=Session/RecentContact;
  订阅=MsgListener(+registerNTListener);发送=MsgService;结果=MsgService 回调(内部事件 kNTOnAddSendMsg 链)。
- **仍未知 [assumed]**:各服务对象在 JS 侧的获取方式与方法的参数/返回/线程上下文(需要 JS 上下文内枚举,属 K2);
  本轮所有证据均为静态字符串/结构证据,未调用任何服务。
- **证据**:local-evidence/k1-02-pid49148-obs.json(运行时观测)、k1-02-static-get-linked-module.txt / k1-02-static-qq-magic-napi-register.txt(静态解码,链表头 RVA 0x0C7092F0 与运行时推导一致)。

### E-3 会话壳 C++ 导出(wrapper.node)— **2026-10-06 F-1 首轮解码更新**

- **调用约定 [verified]**:MSVC x64;返回 `__qq::std::shared_ptr` 的函数使用隐藏返回槽
  (`rcx`=sret,实际首参顺移至 `rdx`),实证于 CreateNTSessionShell(失败路径向 sret 写 16 字节零 =
  空 shared_ptr)。
- **`__qq::std::shared_ptr` 布局 [verified]**:`{T* ptr@0, control*@8}`;引用计数位于 `control+0x10`,
  原子递增(`lock inc dword [rax+10h]`)。定制 STL 但 shared_ptr 与主流布局同构。
- **会话对象布局 [verified]**:构造函数将 `{vtable@0, field8@8=0, field10@0x10=0}` 写入对象
  (0x18 字节对象);**vtable @ RVA 0x3EBEA48**。
- **vtable/接口表内容 [verified,语义未定]**:0x3EBEA48 处 4 个函数指针
  (RVA 0x44078 / 0x1FD70 / 0x63EC0 / 0x4409E,前两者与构造器 0x43FAC 相邻,疑似 dtor 对),
  后接自引用 RVA 元数据块;+0x90 处存在第二组 4 方法表(0x44998 / 0x449BE / 0x63EC0 / 0x49D2)。
  结构形态像 QQ 自定义接口描述符,不是裸 C++ vtable —— 完整语义待解码。
- **构造链 [verified]**:CreateNTSessionShell(0x275FA)→ 内层工厂 0x27773 → 实际构造 0x43F02
  → 池句柄绑定(0x43F8A/0x4404E,全局对象池步长 0x9D0,句柄标签=1)→ 大会话壳初始化 0x29252
  (对象 0x530+ 字节,6 个接口 vtable:0x3EBD7B8/0x3EBDA98/0x3EBDAB8/0x3EBDAE8/0x3EBDB58/0x3EBDB88,
  浮点字段 +0x40/+0x68=1.0f,成员含多个 24 字节子对象)。
- **类层次与接口表 [verified]**:INTSessionShell 派生 vtable 0x3EBD728;基类 INTCSessionShellBase
  **接口表 @ RVA 0x3EBE2B8**,方法槽:0x3BE64、0x3E6D5CC×7(同一指针=未实现桩)、0x3BE6E、0x3BECC、
  0x3C0C4、0x3C110;第二表 +0x80 起:0x3E89A、0x3E8AA、0x3E8C0、0x3E906、0x3E916。表尾为
  自引用 RVA 元数据块 + 运行期 cookie(0x05BEC430 类,加载时解析)。
- **string 布局 [inferred-strong]**:成员初始化模式(16 字节清零 + 单字节 0,24 字节跨度,如 +0x98 处)
  与 libc++ 短形态空串吻合(byte0=size<<1,内联缓冲 +1);`__qq::std` 疑为运行时 libc++(`__Cr::std`)
  的同源分支。**SSO 判定位尚需一条 data()/append 模式的直接实证**。
- **方法签名(F-1 三轮,2026-10-06)[verified]**:
  - 表1:槽0 = 转发器(`jmp 0x28384(rdx,0)`);槽8 = 业务方法(操作 `this->member8`,经 0x56C3E/0x168CE 链,
    尾调 0x3FAA 释放);0x3BECC = 4 参大方法(this,rdx,r8,r9,栈帧 0x98);0x3C0C4 = check-then-act
    (3 参,先 `0x57046(member8,r8)→bool`);**0x3C110 = `string method(this)` —— rdx 为 24 字节 string 的
    sret,初始化为 24 字节全零 → libc++ 短空串的直接实证(SSO 布局确认)**。
- **F-1 五轮(活内存转储,2026-10-06)[verified — 修正前轮结论]**:
  - 活 vtable(基类/派生,重定位后)与文件**逐槽一致**(槽0=0x3BE64、槽1-7=0x3E6D5CC 桩、槽8-10=
    0x3BE6E/0x3BECC/0x3C0C4);0x3E8AA 活字节 = 文件字节(此前"内存≠文件"的判断系我方诊断标注
    笔误,已更正——wrapper.node 无保护层改写证据);
  - **表2 五个转发器是运行时死代码**:其 .data 间接槽位(0x49100A8 等)所在页为 **MEM_FREE**
    (State 0x10000)——运行时未映射,调用即 AV;QQ 加载后未提交该尾部区域,这些方法不可达;
  - **基类接口的真实可用面 = 11 槽**:1 转发 + 7 桩 + 5 实方法(槽8/9/10 + slot0 转发)。
    INTCSessionShellBase 上不存在消息收发级业务方法——它只是会话壳生命周期接口;
  - 工具链:obs v3 code_dumps(活内存 → .bin → 离线反汇编,页校验全程零崩溃)。
- **SSO 布局 [verified]**:0x3C110 的 24 字节零初始化 + 0x29252 中 24 字节成员的短形态初始化模式,
  确认 `__qq::std::string` = libc++ 同源 24 字节布局(byte0=size<<1 短形态,长形态 {cap|1@0, size@8, data*@0x10})。
- **路线结论(F-1 终局)**:wrapper.node 的 INTCSessionShell 壳接口已完整解码且**不含消息业务**;
  消息业务入口在 major.node 的 nodeIKernel* 服务面(内存域已实证其存在),但其 JS 绑定函数与
  C++ 实现的连接点仍需 EXP-K2-01(自建 env)或等价的 JS 上下文通道才能枚举。wrapper 侧的
  C++ 直连路线(绕过 JS)在本接口面上**无消息业务可接**。
- **工具**:caligo-cli `disasm` 子命令(iced-x86,Apache-2.0,已登记 source-register)。
  证据:local-evidence/f1-*.txt(全部本轮 dump)。
- **结论**:E-3 从"候选"升级为"部分契约"——调用约定、对象布局、接口方法表全部实证;
  剩余缺口 = 方法签名解码 + SSO 直接实证,均在纯离线范围。

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
| 账号状态 | `major` linked binding 内的绑定函数(名字待静态挖掘) | 位置已实证,候选名未取得 |
| 会话取得 | 同上(E-3 C++ 路线降级为备选) | 位置已实证,候选名未取得 |
| 消息订阅 | 同上(E-4 转为 A2 线索) | 位置已实证,候选名未取得 |
| 发送 | 同上 | 位置已实证,候选名未取得 |
| 发送结果关联 | 同上 | 空缺 |

**A1 关卡判定(G1)**:仍不通过,但已从"入口在哪都不知道"推进到"入口门已实证定位":
五类入口大概率全部位于 `process._linkedBinding('major')` 暴露的导出面之后。剩余缺口 = 绑定函数名清单 +
每个函数的 §6.1 契约字段。**仍不得调用任何 major 绑定函数**(ABI/线程/生命周期未闭合)。

## 4. 下一步实测计划(回填本契约的唯一途径)

1. ~~加载链与握手验证~~ → 已完成(EXP-K1-01,2026-10-06);
2. ~~运行时注册面观测~~ → 已完成(EXP-K1-02:major = linked binding 实证;注册链/thunk/napi_module 三级捕获,
   major 的真注册回调 = major.node RVA 0x22D50,napi_module 结构 = RVA 0x58000;wrapper.node 持有 "QQNT" 绑定);
3. ~~静态服务面挖掘~~ → 已完成(nodeIKernel* 30+ 服务 / 监听面清单,见 §2 E-2);
4. **当前步(K2 起点设计)**:确认 JS 上下文获取方式。候选方案(需先做方案评估再实验):
   a) 注册自有 context-aware linked binding(flags=2),等待新 Environment 创建时被回调,取得 napi_env;
   b) 复用 qq_magic_napi_register 语义注册自有 napi_module(需评估与 QQ 环境的兼容性);
   c) 对已运行 Environment 的枚举(内存扫描,侵入性强,次选)。
   取得 napi_env 后即可枚举 major 导出对象的真实方法名与签名,逐个填 §6.1 契约;
5. ABI/线程/生命周期闭合前不触碰发送。

## 5. 公开资料线索登记(S13,全部未验证)

| 来源 | 声称 | 本轮核对结果 |
|---|---|---|
| [go-cqhttp issue #2471](https://github.com/Mrs4s/go-cqhttp/issues/2471)(抓取 2026-10-06) | wrapper.node 在 major.node 内加载;hook require 不可行,需经 process 方法 | 未验证;与"E-2/E-3 均在主进程"观察不冲突 |
| [解析NTQQ数据库(lengyue.me)](https://lengyue.me)(抓取 2026-10-06) | wrapper.node 含 `nt_sqlite3_key_v2` 等数据库密钥函数 | **本版本(9.9.33-52230)导出表中不存在该符号** [verified-by-absence];该文针对旧版本,不得按旧版本外推 |
| [NapCat 文档](https://napneko.github.io/guide/napcat)(抓取 2026-10-06) | NapCat 调用 Electron IPC 之下的 Node 原生模块接口 | 方向与 E-2 判断一致 [inferred];实现细节不复用、不采信 |
