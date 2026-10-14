# QQ 原生寿命/释放合同（G-LIFE / G-ABI）——固定 9.9.33-52230 静态证据 R3

日期：2026-10-08。对象与纪律同 [qq-native-thread-contract.md](qq-native-thread-contract.md)（RVA 寻址、只读静态证据、证据树 `E:/stella/_reference/qq-native-r3-p1-20261008/`）。本文覆盖计划 §7-P1 第 3 步（服务 pair 的完整 retain/release、析构辅助/allocator 对、callback operation 与 disposer）与第 4 步的寿命观察，是 [R2 qq-object-resolver.md](E:/stella/_reference/qq-native-r2-20261008/reports/qq-object-resolver.md) §7 的增量。

## 1. 控制块布局（`73CFA6` 构造）[verified]

```
offset 0x00 : vptr → vtable RVA 3F6F4D8（MSFService 块;写入点 0x73CFD8）
offset 0x08 : int32 强引用计数（uses）   —— getter/安装路径 LOCK 递增
offset 0x10 : int32 弱引用计数（weaks）  —— 弱成员/观察者 LOCK 递增
offset 0x18 : qword 托管对象指针（73CFA6 写入 param_2）
0x28 字节总量，分配器 B75000(0x28)
```

证据：[R2 phase4/functions/73cfa6.c](E:/stella/_reference/qq-native-r2-20261008/phase4/functions/73cfa6.c)、[R2 phase4/functions/1687c.c](E:/stella/_reference/qq-native-r2-20261008/phase4/functions/1687c.c)。

注意：这不是可直接跨 ABI 复用的 `std::shared_ptr` 声明——vtable 有 4 个槽（见 §2），与 MSVC 标准形态不同；Rust 层必须按" opaque 控制块 + 成对辅助函数"处理。

## 2. 控制块 vtable（`3F6F4D8`）四槽 [verified 形状，语义部分推断]

| 槽 | 目标 | 观察到的行为 |
|---|---|---|
| slot0 (`+0x00`) | `1F280` | `{ 调 28F0（purecall 垫片）; flag≠0 → B75029(self) }` —— 删除析构包装形态，基类实现为纯虚 |
| slot1 (`+0x08`) | `59F40` | （phaseA 导出为空体/极短，详见证据文件；未单独闭合） |
| slot2 (`+0x10`) | `73D040` | 按类型描述符 `185D8D590` 做 RTTI 匹配，命中返回 `self+0x20` —— "查询/转型到具体接口"形态 |
| slot3 (`+0x18`) | `1F30E` | 调 `1F348(out, self, 1)` —— 克隆/复制形态；**也是析构尾调用的槽**（见 §3） |

证据：[phaseA/functions/1f280.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseA/functions/1f280.c)、[phaseA/functions/73d040.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseA/functions/73d040.c)、[phaseD/functions/1f30e.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/1f30e.c)。

## 3. 析构/释放链 [verified]

### 3.1 MSFService 单例对象

- vtable slot0 `7378A4` → `737A2E`（析构体；flag≠0 → `B75029`）。CoreService 对应 `74A156 → 74A220`（R2 已证，复核一致）。
- **观察到的递减模式**（以执行器单例析构 `9C15D6` 尾部为代表，[phaseD/functions/9c15d6.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/9c15d6.c)）：
  1. 重置对象全部 vptr（析构签名动作）；
  2. 逐成员清理（含 `1DB2` 对 pair 成员的释放）；
  3. 取对象 `+0x18` 存的控制块指针；`LOCK` 递减 `*(int*)(ctrlblk+0x10)`（**弱计数**）；
  4. 旧值为 0 → 调用控制块 vtable **slot3**（`+0x18`）= 控制块自删除。

即：**弱计数归零 → 控制块自删**。强计数递减由 `1DB2` 完成（§3.4）；其前值为零时调用 control-block vtable `+0x08` 并进入 QQ deallocator。该虚函数与偏置/拥有者计数如何共同销毁托管对象仍未闭合；`2632` 是另一条 callback-manager dispose 路径。

### 3.2 获取（递增）位点全集 [verified]

| 位点 | 计数 | 证据 |
|---|---|---|
| getter `72DE38` / `74203C` 控制块非零时 | `+8` 强 | R2 §4（复核一致） |
| `1687C` 绑定（对象 `+0x40/+0x48` 内嵌句柄） | 新控制块 `+8` 强 | R2 §4 |
| `LoginRequestImpl` ctor `6E5B80` 两个 pair 成员 | `+0x10` **弱** | [phaseD/functions/6e5b80.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/6e5b80.c) |
| TLS 安装器 `D32E5A` | `+8` 强 | [phaseD/functions/d32e5a.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/d32e5a.c) |
| `5781C/57834` 替换式释放（发送路径请求对象） | 释放旧值 | R2 send contract §2 |

### 3.3 pair 移动/复制语义 [verified]

- `31C5F8(dst, src)`：非逐字段复制——调用 src 的 manager（`+0x10`）operation=0，然后把 src `+0x18` 改为空 `236A`、src `+0x10` 改为空 `2338`（move-like 管理协议；R2 §3 已证）。
- `D32E5A` 是 pair 的**强引用复制**（copy + 源清理）；`1DB2` 的强计数释放 ABI 见 §3.4；`2632` 是 callback manager operation-3 dispose，二者不是同一 pair-release 家族。

### 3.4 `1DB2` getter pair release [fixed-hash disassembly]

固定文件 `D:\Program Files\Tencent\QQNT\versions\9.9.33-52230\resources\app\wrapper.node` SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读复核的关键字节为：

```
RVA 0x1DB2: 48 8B 49 08 48 85 C9 0F 85 BD 08 00 00 C3
RVA 0x267C: ... B8 FF FF FF FF F0 0F C1 41 08 85 C0 74 06 ...
RVA 0x2695: 48 89 CE 48 8B 01 FF 50 08 ... E9 E5 86 75 00
```

`0x1DB2` 读取 `[pair+8]` 作为 control block；非空时进入 `0x267C`，对 `[ctrl+8]` 执行原子 `xadd(-1)`。`test eax,eax; je 0x2695` 的前值为零分支调用 control-block vtable `+0x08`，随后跳到 QQ deallocator `0xB75AD90`。因此 `1DB2` 的入参是 `{obj,ctrl}` pair 地址，签名可绑定为 `void(pair*)`；这是 getter pair 的释放辅助，不是 `0x2632` 的 manager operation-3 disposer。

构造点 `73CFA6` 将 `ctrl+8/+0x10` 初始化为零，getter `72DE38` 对 `ctrl+8` 加一。由此只能按 observed counter operation 成对调用 `1DB2`，不能把其末支简单解释为“本次 getter release 必然析构服务对象”，也不能读写控制块计数。固定 hash 上的 disassembly 与 getter/call-site 一致性支持 ABI 形状；它不证明 service shutdown 与异步 callback 已静止。

## 4. 执行器单例的寿命观察 [verified]

`9C0C68`（构造，静态存储 `750088`）：

- 构造即与控制块机制纠缠：成功路径 `D31AC0(*puStack_50, &handle, 3FAD8D0)` + `2EC8(puStack_90, ...)` 把内部 pair 存入对象成员；`9C13B5/9C13EF/9C142A/9C1562` 为成员 pair 的安装/初始化。
- **虚函数自调用**：`(**(code **)(*puVar4 + 8))()`（成员 vtable slot1）与 `(**(code **)(*plVar5 + 0x28))(plVar5, 9C2CB7(...))`（slot5）出现在构造流程中——对象在构造期即被注册进运行时。
- 失败分支：`4E91A6(puStack_50, 0)` 复位 + 日志——构造可失败，失败后存储被显式清零。
- 释放链：`9C2C6C`（删除包装）→ `9C15D6`（析构体，重置双 vptr → 逐成员 → §3.1 的弱减/自删尾）。

**含义**：任何持有该对象 pair 的自有代码必须能执行 §3 的完整对称释放；从 QQ 取回的 pair 若只丢弃控制块（SnowLuma 三处模式），对象永不析构且控制块泄漏（R2 §7 的静态契约缺口，本节给出其机制基础）。

## 5. LoginRequestImpl 寿命形状 [verified]

- 三 vptr 对象；`+0x18`、`+0x170` 为弱引用 pair 成员（弱计数获取）。
- 观察者通知（slot1 成功/slot2 状态码）都经 `3F70` 把 `this+0x20` 弱 pair **锁定为局部 strong**（lock 操作本身是控制块强递增），用完即 `1DB2` 释放。
- 通知在**当前线程同步发起**（无排队证据）；观察者实现与注册方未闭合（thread contract §7）。
- `6E5F3E`（第二处 vtable B 引用）= 析构侧候选 [verified 位置，语义待读]。

## 6. SnowLuma 调用侧缺口的机制定位（复核 R2 §7）

R2 的三条 getter 使用审计（`1760`/`15C0`/`4C40` 只取首 qword、不释放）在本轮机制下即：**取得 `+8` 强引用后永不递减**。对象侧析构链完整存在（§3/§4），因此缺口确在 SnowLuma 调用侧纪律，而非 QQ 对象不可释放。Caligo 自有实现取 pair 后必须：
1. 保存完整 `{obj, ctrlblk}`；
2. 已证 getter pair 可通过 `1DB2(pair*)` 成对释放；`2632` 属于 callback manager op3，不可替代 pair release；
3. 不以自有 allocator 释放 QQ 分配的控制块（`B75029` 是 QQ 的 free 包装）。

## 7. 已闭合 / 仍开放

**闭合**：控制块布局与 0x28 几何；强/弱两位点分离；五类获取位点；删除析构包装形态（`7378A4`/`74A156`/`9C2C6C`/`1F280` 同构）；析构体的 vptr 重置签名动作；弱归零→slot3 自删；LoginRequestImpl 弱成员与锁定式通知；TLS pair 强引用安装。

**开放（下一增量，均列计划未决问题）**：
1. control-block vtable slot1（`59F40`）的完整托管对象析构/弱计数关系，以及 pair counter 的完整拥有者模型；
2. callback manager `op=0` 的 clone、move wrapper 源清空及 `2632→op=3` dispose 有静态调用证据；`7328EA` 会销毁 nested callback 并递减 metadata strong count。op=1 的调用者/准确语义、其它 op 与重入/失败分支尚未穷尽；
3. `73BD6C` 到用户 invoker 是同步直接调用；固定样本上的用户 invoker ABI 为五参（见 §7.3），thread contract §10 将现场 callback 归于 QQ worker/dispatcher；目标实例的当前线程身份仍需按新会话核实；
4. 观察者注册方（谁构造 LoginRequestImpl、以何观察者 pair）——session-ready 充分条件；
5. dispatcher consumer 的 stop/cancel/drain、join/quiescence；没有证明断开/销毁后不再迟到回调。

### 7.1 Caligo callback code 的驻留边界（2026-10-11 离线实现）

- 在任何 Caligo callback context 可发布前，`consumer_probe::leak_ctx` 通过 `GetModuleHandleExW(FROM_ADDRESS | PIN, consumer_invoke)` 固定包含 delegate 函数指针的 bridge image；失败则返回不可用，调用方在 native submit 前停止。
- PIN 由 Windows loader 保证模块保持映射直到宿主进程终止，即使收到 `FreeLibrary`；调用不保存或释放返回句柄。[Microsoft API contract](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandleexw)
- `caligo_native_resident_run` 与 `caligo_g3_send_run` 都先经该 context gate 再构造 callback delegate。常驻 generation 停止时先封闭回调准入，等待已进入的回调离开；之后的迟到 callback 只增加 `late_callbacks_rejected` 审计计数，不读取错误诊断指针，也不进入 256 项响应槽。没有已登记 generation 的回调按 orphan 计数，也不进入共享槽。对应离线测试验证上述隔离行为。
- `native_resident_run` 保留 pump、receive 与 daemon worker 句柄；stop 后有界 join，超时写失败阶段并返回非零。此项只证明 Caligo 自有 worker 的停止结果，不能证明 QQ consumer 已停止、不能取消 QQ 已排队工作，也不证明最终结果已持久交付。
- 这些改动只消除了 bridge 被卸载后 callback 指针悬空、停止后的持续迟到调用占满共享响应槽，以及把固定 sleep 当作自有 worker 已退出的风险。等待的是 Caligo 已准入回调并 join 自有 worker，不是 QQ dispatcher quiescence；owner thread、op 完整性、原生 stop/cancel/drain/join/quiescence 与历史 AV 继续开放。不得把 PIN/本地准入栅栏解释成 G-THREAD 或现场准入通过。

### 7.2 Callback manager operation dispatch 静态补充（2026-10-13）

针对本机冻结样本 `E:\stella\_reference\qq-native-r2-20261008\inputs\wrapper.node`，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Ghidra 12.1.4 `-noanalysis`，由固定哈希脚本定向导出 `73C2AA`、`73BC5C` 两个 manager；再读取它们的相对跳转表并反汇编八个 operation 入口。该过程只处理磁盘文件，不加载或调用 QQ 进程。

| manager 布局 | op | 跳转目标 | 静态可见行为 |
|---|---:|---:|---|
| inline `73BC5C` | 0 | `73BCBE` | 经 `4D3588`、`73BBF8` closure 路径处理并调用 `7328EA`；与 `31C5F8` 的 op=0 调用及源 pair 清空相容，准确调用语义仍以已见 move-like 用法为限。 |
| inline `73BC5C` | 1 | `73BCF3` | 以 `R8/R9` 调 `4D3588` 后返回；该路径没有在此函数内改写 manager/invoker 字段。准确调用者与请求语义未定。 |
| inline `73BC5C` | 2 / 3 | `73BC7F` | 调 `4D3588` 与 `7328EA`；仅 op=2 随后把 manager/invoker 改为空函数 `72D596/72D5C8`，op=3 不做该改写。op=3 的已知外层调用仍是 `2632`。 |
| heap `73C2AA` | 0 | `73C30F` | 将 `R8` 指向的 holder 内容写入输出，并设置 heap manager/invoker `73C2AA/73C34C`；`31C5F8` 随后清空源 pair。 |
| heap `73C2AA` | 1 | `73C32A` | 直接函数尾返回，没有在该分支内读写 closure 字段；调用语义未定。 |
| heap `73C2AA` | 2 / 3 | `73C2DA` | 经 `7328EA` 清理 closure，随后以 closure 指针与 `0x40` 参数调用 `1FD0 → 1FE0 → B75029`；与下述 `24C0 → B75000` 的 0x40 heap fallback 分配配对。仅 op=2 随后重置 manager/invoker，op=3 不重置。 |

两 manager 都直接用 operation 值索引跳转表；入口没有看到范围检查。这里确认的是固定样本中 0–3 的机器码分支，不证明其它值可用，也不证明所有调用点只传这些值。`op=0` 的 move-like 用法、`op=3` 的 dispose 用法有可定位包装调用；`op=1` 的调用者和完整语义、op=2 的调用合同及所有调用点仍未闭合。下一步不得主动调用未知 operation。

2026-10-13 allocator follow-up：`73BBF8` 在 `4D3588` 未返回 arena 地址时调用 `24C0(0x40)`，将 closure 存入返回的 heap block，并安装 heap manager/invoker；`24C0` 跳到 `B75000`。`73C2AA` 的 op=2/3 分支从 holder 取同一 closure 指针，先调用 `7328EA` 清理内容，再以 `(closure, 0x40)` 调 `1FD0`；`1FD0 → 1FE0 → B75029`。这闭合固定样本中 heap-fallback closure 的同尺寸 QQ wrapper 分配/释放配对。它不证明 wrapper 内部 allocator 细节、arena 路径寿命、op=1/op=2 外部调用合同、op=3 可重复调用，也不闭合线程/quiescence。定向导出及 hash 在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\allocator-helper-analysis\followup-20261013\`；只反汇编固定磁盘样本，未加载或调用 QQ。

原始本机 manager 输出位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-dispatch\`：`dispatch.tsv`、`range_73bc5c.asm`、`range_73c2aa.asm`、对应反编译文件及 `target-hash.txt`。allocator follow-up 的跳板反汇编与 hash 位于上述 `allocator-helper-analysis\followup-20261013\`。这些静态增量不闭合 op=1/op=2 完整调用者与语义、owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence、historical AV 或现场准入。

### 7.3 `73BD6C` 用户 invoker 五参 ABI 与 Rust 对齐（2026-10-13）

对象仍是 §7.2 固定磁盘样本及相同 SHA-256。Ghidra 12.1.4 `-noanalysis` 导出位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\callback-contract\assembly\73bd6c.txt`。在调用 `[RSI+0x30]` 前，机器码将 `RDI` 写入调用者 `[RSP+0x20]`，并设置 `RCX=RSI+0x18`、`RDX=0x10`、`R8D=EBX`、`R9=RBP-0x28` 后 `CALL RAX`；调用者在此 call 前未设置 `[RSP+0x28]`。按 Windows x64 ABI，这是一条五参调用：`(inner, 0x10, errCode, errStr, responseUid)`，没有被证明的第六参。该局部 string-like 值在回调返回后由 `0x290A` 清理，与 `response-consumer-scope.md` 记录的 `resp+0x38` msgUid 形状相容，但本段不证明其编码细节或与 Caligo request cookie 的身份对应。

因此原 `consumer_invoke` 六参声明中的 `extra` 没有对应的已传参；继续读取它会把未定义的调用者栈内容误作 span。Rust 入口现按五参声明，保留第五参为 opaque response UID pointer，不解引用；错误码与错误文案仍进入既有有界槽，span-shape 猜测字段/读取已移除。测试用函数指针类型锁定五参 ABI，并以不可读第五参验证捕获路径不依赖它。离线编译/测试不证明 QQ 现场回调已发生，也不证明 response UID 与发送 cookie 相等。

本节关闭的是固定样本 `73BD6C` 用户 invoker 参数个数与源码声明不匹配这一 ABI 子项。callback manager op 0–3 的完整调用合同、owner/consumer thread、原生 stop/cancel/drain/quiescence、稳定 native identity 及历史 `0xC0000005` 仍开放，R4 准入不变。

### 7.4 Callback manager consumer callers follow-up（2026-10-13，静态）

继续针对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）做 Ghidra 12.1.4 `-noanalysis` 定向反汇编/反编译，并将直接调用点限定到 PE `.pdata` 函数范围。全程只读磁盘样本，没有启动、读取或操作 QQ 进程。

- `0x54CE00` 是 16 字节包装对象的 assignment helper：先对目标调用 op=3，再对来源调用 op=1，参数包括 size `0x10` 与目标上下文。
- `0x9B31C8` 先对 `source + 0x10` 调用 op=4 输出查询 helper `0x3FCE5E`（其“callback presence”语义见 §7.6 勘误），然后经 `0x54CE00` 复制至目标，最后通过已知 wrapper `0x2632` 对 source 调用 op=3。这直接支持该共享 callback-manager 接口的复制/assignment 与来源释放用法。`0x744D7E` 也在 assignment 后对 source 调用 op=3；`0x31FA9D4` 在复制较大对象时两次调用 helper，处理相邻的两个 16 字节成员。
- `0x3BC545C` 以 op=1 将 size `0x10` 的 source wrapper 复制到局部存储，再把副本传给 `0x3BC6298`。其上游 `0x3BC4C18`、`0x3BC4D84` 从请求对象 `+0xA0` 字段取该对象，并将副本传入下游 callback/request helper；这是强 callback-field 候选，但具体 manager table 仍未解析。
- 排除 `0x970E2A`（其调用方把 op=2 返回字节当布尔值）、`0x982F4E`（op=1/op=2 返回字节进入配置序列化字段）和 `0x340CA75`（把 op=1 返回值当布尔值）。这些返回形状不符合已知 callback manager op=1/op=2 的 void 路径。`0x3225B8A`、`0x33FCBA4` 是另一 `+0x68` property manager 的属性序列化调用；`0x34A156E` 的 0x20 步长数组 op=1 copy 仍因容器/成员所有权未知而未定类。

本次有界 `E8 rel32` 扫描在 `.pdata` 函数范围内找到 42 个目标匹配；它不含完整间接引用或数据流引用，不能据此称为完整 xref，也不能从缺项推出“没有调用”。原始输出和固定哈希记录位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\` 下的 `manager-callers-followup-20261013\`、`caller-refs-20261013\`、`candidate-context-20261013\` 与 `candidate-context-followup-20261013\`，未入库。

这补强了共享接口的 op=1/op=3 consumer 用法；仍未证明这些对象确实使用 inline 表 `0x3F7052C` 或 heap 表 `0x3F70540`，也未穷尽 op=1/op=2 调用者或解析 op=2 外部合同。arena 分支、owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence 和历史 AV 根因继续开放；R4 不准入。

### 7.5 Callback manager table owner 与 op=2 bounded caller screen（2026-10-14，静态）

再次核验安装磁盘样本 `D:\Program Files\Tencent\QQNT\versions\9.9.33-52230\resources\app\wrapper.node`，SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。筛查器只读取 PE 可执行节与 `.pdata` 函数范围，不启动或读取 QQ 进程、不调用函数、不操作 SnowLuma；脚本与地址级输出留在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-caller-screen-20261014\`。

- 限定 `.pdata` 中的直接 `E8 rel32` 调用后，冻结目标集合共 41 个匹配；`73BBF8` 仅见外部直接调用 `0x732A80 → 0x73BBF8`，另一个是 inline manager 的 op=0 路径 `0x73BC5C+0x81 → 0x73BBF8`。`73BBF8` 在 `4D3588` 返回 arena 地址时选择 `{manager=73BC5C, invoker=73BD08}`；未返回时以 `24C0(0x40)` 分配并选择 `{manager=73C2AA, invoker=73C34C}`。这把两种 manager table 形状连接到共同的 builder，但不证明 arena 分支对象寿命或下游 consumer 的停止/排空。
- 在全部可执行节内筛选精确 `mov edx,2` 编码并映入 `.pdata`，得到 5,496 个候选指令、4,801 个函数；限定同一基本块里的首个调用且调用形状为 `[reg+0x10]` 后有 12 个候选。它们显示为对象虚表调用形状（其中 `0x970E2A`、`0x982F4E` 的返回字节被保存/序列化），没有一个能以已知 manager slot 调用形状或具体对象字段，绑定到 `73BC5C/73C2AA` op=2。原有排除仍适用。
- 该筛查仅覆盖固定 immediate 编码、有限指令形状与直接调用；间接引用、不同参数准备/编码、未识别 wrapper 及数据流路径仍可能存在。结果只支持“本次 bounded screen 未找到可归属的 op=2 caller”，不支持“没有 op=2 caller”，也不闭合完整调用者集合或外部合同。

可复现材料：`r2-op2-caller-screen.py` 与 `r2-op2-caller-screen.txt`（同目录）。新证据仍不闭合 op=1/op=2 全调用者、arena 寿命、callback owner/consumer thread、QQ stop/cancel/drain/quiescence、历史 `0xC0000005` 或现场准入；R4 继续不准入。

### 7.6 Callback manager op=4 与 op=2 ABI 调用点补充（2026-10-14，静态）

本节复核与 §7.5 相同的冻结磁盘 `wrapper.node`，SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；只读 PE、`.pdata` 和反汇编，没有访问 QQ 进程或操作 SnowLuma。

- 两张已知 manager 相对跳表地址分别是 `0x3F7052C`、`0x3F70540`，相差 `0x14`。按已验证的相对位移解码 `table+0x10`，inline `0x73BC5C` 的 op=4 指向 `0x73BCB0`，heap `0x73C2AA` 的 op=4 指向 `0x73C309`；前者执行 `and qword ptr [rbx],0`，后者执行 `and qword ptr [rax],0`，二者都把第五参数指向的 qword 清零。故已知两张表的可证分支范围是 op=0–4，而不是只到 op=3；分支没有范围检查，不能把 op=5 或其它越界值当成有效操作。
- `0x3FCE5E` 加载传入 pair 的首 qword 并以 `(RCX=0, EDX=4, R8=0, R9=0, fifth=&out)` 调 manager，之后返回 `out != 0`。对上述两张表，op=4 把 `out` 清零，因此 helper 返回 false。此前 §7.4 依调用位置把它称为“presence helper”，语义命名过强；本节将其校正为 manager op=4 输出查询 helper。它是否表示 callback 是否存在，不能从该 helper/两张表的 op=4 实现推出。
- 对全部可执行节中的精确 `mov edx,2`、`.pdata` 函数范围内的同基本块首个间接调用，加入轻量寄存器来源归一化；要求 RCX 等于间接 manager 函数指针槽地址、R9D=`0x10`，且 op=2 返回值在紧邻指令内未被观察。此 bounded ABI 形状筛选得到 9 个调用点、分布于 7 个函数：`0x9C50FF/0x9C511A/0x9C5135`、`0x9E13A7`、`0x3456CE6`、`0x34576E5`、`0x35140D5`、`0x3B36599`、`0x3DD3ECD`。其调用均把 RCX 指向与间接目标相同的 pair/manager slot，并传 `R9=0x10`；局部上下文与清理/收尾路径相容。两个调用点 `0x35140D5` 和 `0x9E13A7` 还会在同一字段上调用 `0x3FCE5E` 的包装（后者经 `0x3FBAB2` 转调），然后在各自条件分支中 op=2。
- 这 9 个是高置信 manager-ABI-shaped op=2 caller 候选，较 §7.5 的 `[reg+0x10]` 过滤有所扩展；结合 pair-slot、16-byte size 与相邻 op=4 调用，足以把 op=2 调查推进到具体静态调用位置。但间接函数指针的动态值未解析，尚不能逐一断言它们运行时指向 `0x73BC5C` 或 `0x73C2AA`；寄存器归一化与近邻返回值检查也不是全路径数据流/完整 xref。不得据此推断调用时机、manager 外层 owner、线程、closure 寿命或 dispatcher quiescence。

可复现筛查器及地址级输出仅存本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\r2-op2-manager-shape-screen.py` / `.txt`。此增量没有改变 R2/R3/R4 准入：consumer/owner thread、manager 外层 teardown、QQ stop/cancel/drain/quiescence、历史 `0xC0000005`、R3 stable identity 仍开放；R4 继续不准入。

### 7.7 op=2 候选代码引用的 RTTI/vtable 复核（2026-10-14，静态）

对 §7.6 中 `0x9C509C`、`0x345763E`、`0x3456C1C` 三个函数，复核此前记录的 `.rdata` qword 代码地址，并沿 RTTI/vtable 与 vptr 写入路径追查。样本仍为磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；无 QQ 进程读取、函数调用或 SnowLuma 操作。最终脚本与原始输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\r2-op2-vtable-membership-20261014.py/.txt` 和同目录 `r2-op2-owner-method-context-20261014.py/.txt`。

- 扫描 `.rdata` 中满足 image-relative signature/self-RVA、有效 type descriptor 与 class hierarchy descriptor 的 COL，并以 COL 指针定位 vtable，再按下一个有效 header 与连续 `.text` 函数地址边界限制 slot。总计发现 15,412 个有效 COL / 15,410 个 vtable header；已知 `ThreadTaskManagerImpl` vtable `0x4260448` 被正确识别为 parser 正向对照。此前三个 qword 位于 vtable 中间，前一 qword 是上一 virtual slot，而不是 COL；只检查“前一 qword 是否 COL”会误判 vtable 起点。

| op=2-shaped 函数 | 有效 vtable / slot | RTTI class descriptor | vptr 写入点 |
|---|---|---|---|
| `0x9C509C` | RVA `0x3FADD38`, slot 5 | `.?AVFileDragManagerWin@nt@@`，COL RVA `0x3FADD70` | `0x9C47E6` 计算表地址，`0x9C47ED` 写入 `[rcx]` |
| `0x345763E` | RVA `0x43E7528`, slot 3 | `.?AVIgnoreBodyHttpRequestHandler@httpx@@`，COL RVA `0x43E75B0` | `0x345747D` 计算表地址，`0x3457484` 写入 `[rcx+8]` |
| `0x3456C1C` | RVA `0x43E73B8`, slot 3 | `.?AVSaveToStringHttpRequestHandler@httpx@@`，COL RVA `0x43E74E0` | `0x34569A6/0x3456D4C` 计算表地址，后续写入 `[rcx+8]` |

- 这些 RTTI/vtable 结果确定的是 op=2-shaped 函数所属的 enclosing C++ class/method slot，不是间接 manager field 的目标类型。`0x9C509C` 对 `[this+0x30]`、`[this+0x50]`、`[this+0x70]` 分别执行 op=2；`0x345763E` 与 `0x3456C1C` 则从 `0x123528` 返回对象的 `+0x38/+0x48` manager slot 调 op=2。动态调用值仍未逐一绑定到 `0x73BC5C/0x73C2AA`。
- `0x345745A` 的 vptr 初始化路径还从输入对象 `+0x10` 取间接 manager target，以 `EDX=1`、`R9=0x10` 调用，目标 pair 地址在 `this+0x20`；这是一个新的 op=1-shaped copy/initialization 点，但其动态 target 仍未知。`0x3456986 → 0x3456A0A` 的 base 初始化另以 op=0 调输入 manager；这些用法不能充当三个 op=2 slot 的具体动态 table 证明。候选 handler 的构造/析构 vptr 写入已找到，manager/pair 初始化所有权仍未闭合。
- 可复现脚本/输出：本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\r2-op2-vtable-membership-20261014.py/.txt` 与 `r2-op2-owner-method-context-20261014.py/.txt`。脚本在读样本前校验固定 hash。COL 扫描和常见 RIP-relative xref 仍有编码/解析边界，不构成全 binary xref、调用时序、manager 动态值或寿命/线程/quiescence 证明。

### 7.8 `HttpConnection+0xA0` callback wrapper owner（2026-10-14，静态）

重新核验磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 PE、`.pdata` 与目标函数反汇编；没有启动/读取 QQ 进程、调用原生函数、发送消息或操作 SnowLuma。

- `0x3BC2C3A` 与 `0x3BC3082` 中的 RIP-relative vptr 都指向 `.rdata` RVA `0x458D3B8`；前置 COL RVA `0x458D4C0` 的 type descriptor 是 `.?AVHttpConnection@httpx@@`。vtable slot 0 的 `0x3BC550C` 调用 `0x3BC3082` 清理主体。由此将该字段 owner 从此前的 request-object 候选准确收敛为 `httpx::HttpConnection`。
- 构造函数 `0x3BC2C3A` 将识别为空 wrapper 哨兵的 manager/invoker 地址 `0x2338` / `0x236A` 写入 `this+0xB0/+0xB8`，并把 `this+0xA0` 传给该 manager 的 op=3、`R9=0x10` 路径。析构主体 `0x3BC3082` 对 `this+0xA0` 调用 `0x2632` dispose，构成字段初始化与释放的配对证据。
- `0x3BC456A` 的状态分发分别进入 `0x3BC4C18`（设置状态 6）和 `0x3BC4D84`（设置状态 8）；两者都把 `this+0xA0` 交给 `0x3BC545C`。后者从该 wrapper 的 `+0x10` manager slot 发起 op=1、16-byte copy，再把副本传给下游 HTTP request/callback helper。因此，现可确认 `HttpConnection+0xA0` 是 manager-managed callback wrapper storage；这不表示任一运行实例当前持有非空 callback。
- 在本节初次追查时尚未定位 empty→non-empty writer；后续固定-hash 复核已在 §7.9 找到 ctor 赋值路径及该字段局部 inline/heap manager/invoker 目标。此结论不将局部 helper 误绑到 `0x73BC5C/0x73C2AA` 全局表；全局表的完整 caller/ABI、arena 分支寿命、manager/consumer owner thread、外层 teardown、QQ dispatcher stop/cancel/drain/quiescence、历史 `0xC0000005` 和 R3 stable identity 继续开放，R4 仍不准入。

可复现脚本/文本输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-httpconnection-callback-field-owner-20261014.py/.txt`。该脚本在读取前校验固定 hash；只含本机静态地址与 RTTI，不入库。

### 7.9 `HttpConnection+0xA0` callback capture 安装路径（2026-10-14，静态）

继续使用 §7.8 的冻结磁盘 `wrapper.node` 和 SHA-256；脚本先验 hash，只读 PE、`.pdata`、反汇编与 RTTI，没有访问 QQ 进程或操作 SnowLuma。

- 构造路径可沿直接调用串复核：`0x3458A18` 在 `R8D==0` 分支调用 `0x3458B00`（`0x3458A5D`）；该工厂分配 `0x168` 对象并调用 `0x3BC2C3A`（`0x3458B61`）。有界 `.text` rel32 / `.pdata` 扫描只发现这一个直接 ctor call site；间接调用、取址调用及扫描解析边界不在此结论内。
- `0x3BC2C3A` 先把 empty manager/invoker sentinel 放入 `this+0xB0/+0xB8`，再以 op=3、size `0x10` 初始化 `this+0xA0`。随后 `0x3BC3008(this+0x158, this)` 经 `0x3BC66B5 → 0x3BC66EF` 构造 capture pair。`0x3BC66EF` 分配并初始化 RTTI 为 `__shared_ptr_emplace<WeakWrapper<HttpConnection>>` 的对象；其 vptr RVA `0x458D5F8` 的 COL RVA 为 `0x458D620`。helper 清零 control-block 计数，把 `this` 写到对象 `+0x18`，输出 `{object+0x18, object}`；ctor 对 control block `+0x10` 做强引用递增。因此已见成功构造路径生成的是指向该 `HttpConnection` 的 weak-wrapper capture pair。
- ctor 从 `this+0x158/+0x160` 复制该 pair 到临时 16-byte wrapper并增加 control-block 强计数，然后调用 `0x3BC678A(temp, &this->manager_invoker, this+0xA0, 0x10)`。该 helper 在 `0x40634` 的 inline-storage 路径成功时安装 manager `0x3BC67F8` / invoker `0x3BC6436`；失败时经 `0x24C0(0x10)` 分配 heap fallback 并安装 `0x3BC692A` / `0x3BC6576`。两条路径都把 pair 复制进目标存储并清空临时 pair。由此，成功完成该 ctor 的路径会把 empty `+0xA0` wrapper 填成持有 HttpConnection weak-wrapper capture 的非空 wrapper；其动态 manager/invoker 目标已绑定到这组本地 inline/heap helper，不是 `0x73BC5C/0x73C2AA` 两张全局 manager 表。
- 状态 6 与状态 8 路径经 `0x3BC545C` clone 此 wrapper，再通过 `this+0x38` handler 的虚表 `+0x10` 调用转交 callback wrapper 和状态数据。当前只确定调用形状；具体 handler 实现、实际 consumer 线程、外层对象 teardown，以及 QQ dispatcher stop/cancel/drain/quiescence 仍未证。该字段路径不证明某个现场实例已构造、callback 已执行或结果已送达。
- 上游直接调用点筛查还找到 `0x3458736→0x3458A18` 与 `0x34592DC→0x3458A18`。前一 caller 在 `[this+0x18]==0` 分支 move 出自己的 `this+0x38` 对象并以 `EDX=0/R8D=0` 进入创建分支；该对象经 `0x3458B00` 被转交为新 `HttpConnection+0x38` handler。另一 caller `0x345927C` 复制一组 owner pair，再调用 `0x5CD68C`；该 helper 将输入 pair 交给 `0x3F70`，复制临时 pair 中的对象指针后释放临时与输入副本，形状与 weak-reference lock 一致，但 pair 的具体静态类型尚无 RTTI 证明。非空返回指针被传入 `0x3458A18`；该返回对象的动态类与第二条 `HttpConnection+0x38` handler 类型仍未闭合。

可复现脚本与地址级文本保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`，包括 `r2-httpconnection-write-sites-20261014.py/.txt`、`r2-httpconnection-owner-call-context-20261014.py/.txt`、`r2-httpconnection-nested-manager-helpers-20261014.py/.txt`、`r2-httpconnection-inner-lambda-20261014.py/.txt`、`r2-httpconnection-parent-callers-20261014.py/.txt` 与 `r2-httpconnection-parent-callsite-context-20261014.py/.txt`。全映像扫描的 immediate/xref 覆盖和反汇编边界有限，不能据未命中推断无其它动态构造路径。

### 7.10 `HttpConnection+0x38` 的已接受 socket 回调路径（2026-10-14，静态）

重新对冻结磁盘 `wrapper.node` 做单次 hash 校验，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 `.pdata`、反汇编与 RTTI/vtable；没有访问 QQ 进程、调用原生函数、发送消息或操作 SnowLuma。

- 创建与接受链可静态相连：`0x2A0690A → 0x2A08062` 分配 0x30 字节 listener 并经 `0xCADD64 → 0xCADD8A` 构造；随后 `0x2A080BD` 把对象交给 `HttpServer` ctor `0x3457D10`，该 ctor 将它写入 `HttpServer+0x10`。构造/vptr 与 RTTI 确认 listener 是 `TCPServerSocket@xpng`（vtable RVA `0x3FE48E8`、COL `0x3FE4920`）。`0x3458614` 从该字段取对象，经 vtable `+0x28` 发起 accept，并把输出 pair 指向 `HttpServer+0x38`；成功时进入 `0x3458736`。`TCPServerSocket` slot 5 (`+0x28`) 是 `0xCADF0A`，转调 slot 4 (`+0x20`, `0xCADF7E`)；该方法以 `TCPServerSocket+0x8` 的 vtable `+0x20` 接受底层连接。
- `TCPServerSocket+0x8` 的对象由 `0x217C624` 分配并经 `0x31F66EC` 构造；vptr RVA `0x43B5E18`、COL `0x43B5EB0` 的 RTTI 为 `TCPSocketLibuv@xpng`。其 slot 4 (`+0x20`) 为 accept 方法 `0x31F6CF6`。成功分支通过 `0x31F6B91 → 0x31F66EC` 构造另一个同类 0x38 字节对象，再经 `0x31F6BBC` 组成 shared pair 返回给 `TCPServerSocket+0x10`。`0xCAE0DC` 随后消费该 pair，分配 0xA0 字节对象并调用 `0xCACD38`；该 ctor 写入 `TCPClientSocket@xpng` vtable（RVA `0x3FE46D8`、COL `0x3FE4740`），将底层 pair 存入自己的 `+0x8`，并把新对象交回 `HttpServer+0x38`。该 pair 经 `0x3458736 → 0x3458A18 → 0x3458B00 → 0x3BC2C3A` 后成为 `HttpConnection+0x38`。因此本条 accepted-connection 路径上的 `HttpConnection+0x38` 动态类型为 `TCPClientSocket@xpng`，其 `+0x8` 为 `TCPSocketLibuv@xpng`。
- `HttpConnection` 状态 6/8 (`0x3BC4C18` / `0x3BC4D84`) 均以 `this+0x38` 的 vtable `+0x10` 转交复制的 callback wrapper 与状态上下文。`TCPClientSocket` slot 2 (`+0x10`) 为 `0xCAD3B8`；它调用 `+0x8` 对象 vtable `+0x40`，即 `TCPSocketLibuv` slot 8 `0x31F729E`，在内层调用返回 `-1` 时走 `0x3FBB22` 辅助路径。`0x31F729E` 检查对象状态并传入本地 callback entry 地址至 helper `0x3EB5090`；仅能确认这个静态调用形状，函数名/回调线程/结果合同均未确定。此为通用 xpng/libuv socket 层的回调转交，不是 QQ callback manager 的语义证据。
- 勘误（2026-10-14 follow-up）：上述 direct `E8 rel32` / `.rdata` absolute-qword 初筛漏掉 RIP-relative 引用；后续按有效 `.pdata` 函数边界复扫，在 `0x3459127`、`0x3459144`、`0x34593EF` 找到两个 invoker 的入口引用。`0x42F2B` helper 的 RTTI 语义仍未绑定；替代 owner 与分支证据见 §7.11–§7.12。初筛不能据此推断无 caller。
- 路径边界：`0x34592DC`（caller `0x345927C`）是另一个 `0x3458A18` 调用点；后续 §7.11 已将 captured owner 的静态类型收敛到 `WeakWrapper<HttpServer@httpx>`。factory 以 `R8D` 作零/非零路由 gate；寄存器级分支续查见 §7.12，业务语义与节点类型仍 unknown。本节已接受路径属于通用 `httpx`/`xpng` HTTP socket 层，不证明它就是 QQ 发送响应消费者，也不绑定到全局 manager tables `0x73BC5C/0x73C2AA`；没有现场 callback 执行证据。

复现脚本与地址级导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-httpconnection-tcpclient-chain-20261014.py/.txt`、`r2-tcpserver-inner-acceptor-rtti-20261014.py/.txt`、`r2-httpserver-listener-ctor-xrefs-20261014.py/.txt` 与替代 wrapper 筛查 `r2-alternative-parent-xrefs-20261014.py/.txt`、`r2-alternative-wrapper-xref-screen-20261014.py/.txt`。脚本读取样本前核验 hash；地址范围与 RTTI/vtable 输出包含 factory/ctor、accept 和 callback-forwarding slots。这里只闭合已接受连接的通用 HTTP/socket 路径；实际 QQ callback consumer/全局 manager 身份、owner/consumer 线程、外层 teardown、QQ stop/cancel/drain/quiescence 和历史 AV 根因仍开放。

### 7.11 `HttpServer` 替代 callback wrapper 的弱 owner（2026-10-14，静态）

重新核对同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本节仅为静态 `.pdata`、RIP-relative xref、ctor 字段与 RTTI 证据，不访问运行中的 QQ。

- `HttpServer` ctor `0x3457D10` 在 `this` 与 `this+0x8` 写入的两个 vptr 分别指向 RVA `0x43E75D0`/`0x43E75E8`；各自 COL 的 signature 为 1、self RVA 与 COL 相等，RTTI 均为 `.?AVHttpServer@httpx@@`。ctor 以 `rcx=this+0xE0, rdx=this` 调 `0x3457FDA` 建立该字段的 owner pair。
- `0x3457FDA → 0x345947A → 0x34594B4` 建立控制块；`0x34594B4` 在 `0x34594E0` 写入的 vtable RVA `0x43E7738`，其有效 COL RVA `0x43E7760` RTTI 是 `__shared_ptr_emplace<WeakWrapper<HttpServer@httpx>>`。构造体把源 `HttpServer this` 写到 control block `+0x18`，形成 `{control+0x18, control}` 并安装到 `HttpServer+0xE0/+0xE8`。这是 wrapper 的静态 captured owner type 证据；它不排除运行实例来自派生类型。
- accept 方法 `0x3458614` 以同一 `this` 调 `0x3458736`。后者复制 `this+0xE0/+0xE8` pair、附加 `+0x5C` 生成的 DWORD，再调用 `0x3459100`。该 helper 按 inline/heap 分支分别安装 local manager/invoker `0x3459174/0x3459220` 与 `0x3459384/0x3459426`，并复制捕获 pair 与 DWORD；它们不是全局 manager tables `0x73BC5C/0x73C2AA`。
- 全 `.pdata` 边界 RIP-relative scan 在 `0x3459127` 找到 wrapper `0x3459220` 的地址引用，在 `0x3459144`/`0x34593EF` 找到 `0x3459426` 的地址引用；两个 invoker 都进入 `0x345927C`。该函数用 `0x5CD68C` 对 captured weak owner 做 lock-shaped 操作后，把返回的 HttpServer owner、state 和 pair 传给共同 factory `0x3458A18`。该 factory 的 state 0 路径进入 `0x3458B00`；非零 state 走 `0x3459746`，不同分支不合并成一个 handler 类型结论。
- 因而前节 `0x34592DC` 路径的 captured owner 已静态归属 `HttpServer@httpx`，不再是无类型 weak-return 候选；但本结论只解释 generic HTTP Server 的 callback owner/状态分派，不证明 TCP callback 的实际执行线程、任一 `HttpConnection+0x38` 在所有状态下的动态 handler、QQ response consumer、原生 manager 线程/释放合同或 teardown/quiescence。

可复现脚本与导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`：`r2-alternative-pdata-rip-xrefs-20261014.py/.txt`、`r2-alternative-wrapper-registration-context-20261014.py/.txt`、`r2-httpserver-class-rtti-validated-20261014.py/.txt`、`r2-httpserver-weak-capture-and-state-route-20261014.py/.txt`、`r2-httpserver-weak-pair-construction-20261014.py/.txt` 与 `r2-httpserver-weak-wrapper-rtti-20261014.py`。每个读取脚本先核验固定 SHA-256；RIP-relative 筛查以有效 `.pdata` 函数边界为解码输入。静态命中仍不替代现场 callback 或 R4 准入。

### 7.13 `HttpServer+0x20` 注册 key 的写入与回流（2026-10-14，静态）

继续读取同一冻结磁盘 `wrapper.node`，SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本节仅描述 generic `HttpServer@httpx` 的机器级 key 路径，不把它映射成 Caligo request ID、QQ native callback 或业务回执。

- `0x3458614` 在 listener 虚槽 `+0x28` 返回非 `-1` 后调用 `0x3458736`。该方法先按 `EDX` 符号位分支；成功路径从 `HttpServer+0x18` 取源对象，递增 `DWORD [this+0x5C]`，并将新 DWORD 作为 key 交给 `0x34589B8`，其容器参数是 `this+0x20`。因此 map key 的静态来源已收敛为 HttpServer 自增值；它不是调用时传入的 `EDX` 错误/状态值。
- `0x34589B8` 调通用 helper `0x245ADDE`，helper 先查找既有 key，未命中时构造并挂入 node；插入包装返回 node `+0x28`，随后 `0x1EF48` 将源对象写入该槽。若槽原有非空对象，`0x1EF48` 通过其 vtable slot 0 传入值 1；该对象的具体 C++ 类型与完整 dispose 语义仍未知。全 `.text` 直接 `E8 rel32` 屏幕在有效 `.pdata` 函数范围内找到 helper 的两个调用点，其中 HttpServer 一侧通过 `0x34589B8` 到达；此屏幕不覆盖间接调用或所有控制流。
- 同一成功路径把 `HttpServer+0xE0/+0xE8` owner pair 与刚生成的 key 放入 wrapper capture，key 位于捕获对象 `+0x10`。§7.11 已用 RTTI 将 captured owner 静态归属为 `WeakWrapper<HttpServer@httpx>`；inline/heap invoker `0x3459220/0x3459426` 均进入 `0x345927C`。该 helper 从捕获对象 `+0x10` 取 key，weak-lock-shaped helper 成功后将 key 作为 `EDX`、调用方 DWORD 作为 `R8D` 送入共用 factory `0x3458A18`。
- factory 的 `R8D=0` 路径经 `0x3458B00` 构造连接，再用同一个 `0x34589B8` 包装把 `HttpConnection+0x18` key 插入 `HttpServer+0x60`；`R8D!=0` 路径对 `HttpServer+0x20` 按输入对象 `+0x10` key 精确查找并删除命中 node。结合本节写入点与 §7.12 删除点，已闭合这个 generic HTTP 路径中 `+0x5C` 生成 key → wrapper capture → `+0x20` 精确删除的局部关联；`+0x20` 与 `+0x60` 是不同 header，前者不应与连接表合并。
- 这仍不能证明 `+0x28` payload 在何处被调用、factory 的非零 gate 在真实运行中的业务意义、key 与 native response/request 的身份对应、实际 callback thread，或该表与全局 manager tables `0x73BC5C/0x73C2AA` 有关。它不是 QQ response-consumer 或 R4 现场证据，R2 owner/thread/manager ABI/teardown/quiescence 与 R3 stable identity 继续开放。

脚本先校验固定样本 hash，均保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`：`r2-httpserver-tree-registration-consumer-detail-20261014.py/.txt`、`r2-httpserver-tree-insert-detail-20261014.py/.txt`、`r2-httpserver-map-payload-helper-raw-20261014.py/.txt`、`r2-httpserver-tree-direct-call-xrefs-20261014.py/.txt`，并与 `r2-alternative-parent-xrefs-20261014.py/.txt` 及 §7.12 的 branch/erase 导出交叉核对。无 QQ/SnowLuma 操作、native 函数调用、消息发送或 Cargo 测试。

### 7.17 ThreadTaskManager index-0 task 的动态类型（2026-10-14，静态）

继续核对冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Capstone 5.0.6 按该映像 `.pdata` 有效范围解码构造/调用函数，并解析 MSVC x64 COL/RTTI；没有启动或读取 QQ 进程、调用 native 函数、发送消息或操作 SnowLuma。

- 与 §7.15 的 index-0 读取链相接：`D313D6` 把字节索引 0 交给 `ThreadTaskManagerImpl` vtable slot 1 `0x217D860`。`ThreadTaskManagerImpl+0x48` vector 项是 `TaskQueue` shared pair；slot 1 经 `0x31F83DC` 取出被索引的 pair，再调用 `0x63EC4` 从选中 `TaskQueue+0x38/+0x40` 复制嵌套 pair。因此 `D32138` 收到的是嵌套 `TaskRunner` pair，不是 vector 中的 `TaskQueue` pair。构造端 `0x217D58C → 0x217D6D8 → 0x217DEEA → 0x217DF64` 创建 `TaskQueue` shared pair；`0x31F8F5E` 在 `TaskQueue+0x38/+0x40` 安装每队列 pair，随后 manager build helper `0x31F8378` 收到 `TaskQueue` pair 及 `+0x48` vector 地址。安装 pair 的路径 `0x31F9016 → 0x31F9B36 → 0x31F9BBA` 创建 `shared_ptr_emplace<TaskRunner@TaskQueue>` 控制块；对象 ctor `0x31F9C3C → 0x31F878C` 将输入 pair 保存到对象 `+0x8/+0x10` 并写入 TaskRunner vptr。
- 固定映像 RTTI 交叉验证两层类型：控制块 vtable RVA `0x43B6248` 的 COL/TD 名称为 `__shared_ptr_emplace<TaskRunner@TaskQueue@internal@xpng>`；对象 vtable RVA `0x43B6088` 的 COL RVA `0x43B60B0` 指向 `.?AVTaskRunner@TaskQueue@internal@xpng@@`。因此可把 manager index-0 所取对象命名为 `xpng::internal::TaskQueue::TaskRunner`，不再把其直接称为未知类，也不误认为 `TaskQueue` 本体。
- `TaskRunner+0x8/+0x10` 的 pointee 由固定-hash 构造链闭合。`0x217D242` 在新建 manager 时先将 `ThreadTaskManagerImpl+0x10` 清零并安装 primary vptr；该 vtable RVA `0x4260448` 的 COL/RTTI 名为 `.?AVThreadTaskManagerImpl@internal@xpng@@`。Factory `0x217D35E` 构造指向 manager 的输出 pair/control block，再以 `(out_pair, manager+0x10, manager)` 调用 `0x1687C`；新对象的空字段路径经 `0x168CE` 将同一 `{manager, control}` pair 写到 `manager+0x10`。随后 `0x217D58C` 将该 pair 交给 `0x46448` clone，`0x31F878C` 把 object/control 两 qword 复制到 `TaskRunner+0x8/+0x10`。因此固定样本的 pair pointee 是 `ThreadTaskManagerImpl` 自身；这识别了 owner/context 对象，不识别其关联 task payload。
- `TaskRunner+0x8/+0x10` pair 的 owner 类型现已识别；`0x31F9228` 的业务效果、manager slot-2 mismatch 分支从自身 `+0x38/+0x40` 作出的 pair-shaped projection（字段边界见 §7.18）、slot-3 `manager+0x30` pointee/返回 pair、manager 其它项是否均被停止、child 最后 owner/析构时机以及 QQ dispatcher 全局排空仍 unknown。INFINITE wait 的返回值依旧未检查；R3 stable identity 与历史 AV 根因也未关闭，R4 仍不准入。上述均为固定-hash 离线静态结论，不证明运行时 owner 关系或 quiescence。

复核脚本与地址级导出在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-taskrunner-dynamic-type-20261014.py/.txt`；SHA-256 分别为 `DB2946623D6E62F2F8F123F9A8F2FBC88C8E5104709C7D9D0FF0793EF993EB54` / `70CA4E6B1A5D335A7A0A73C9FA16D380E82F13B97A4456D33F5BE8ACA27FC3E4`。脚本读取前校验样本 hash，导出有效 `.pdata` 函数范围与两组 RTTI；任务构造调用链由同目录 `r2-nte-manager-task-init-20261014.py/.txt` 与 `r2-nte-taskqueue-item-transform-20261014.py/.txt` 补充；`0x63EC4` 的有界 helper 导出由 `r2-nte-bounded-helper-targets-20261014.py/.txt` 交叉复核，SHA-256 `2A8A96FA0897D454E4D16BEB8ED80ED2972E4B05B57D936F94A9EFC7E92DD90D` / `F99FAAF2F77E42DC76D9A8F4E9AC655B8BC98FAF0A1A48DD8B4C0860C2FB32CC`。该静态类型识别只闭合 index-0 wrapper 的类名，不证明队列语义、callback 执行或停机安全。
本次 pair-owner 追查的固定-hash Capstone 5.0.6 导出保存在同一 `_reference` 目录：`r2-nte-manager-inner-pair-followup-20261014.py/.txt` SHA-256 `22E344C4AE6BC4049D93575C86ACC37CE8EC05929B8D5FD70CB800AE5DB641BF` / `AF0D57B43EBBB398EA97A575437C2536BD6A08966650A442E8CD9A3FB772C4D5`；`r2-nte-weak-pair-helper-followup-20261014.py/.txt` `A89494EFF657CFF366B2AA472449F7C816561380D0C8B6A0725202088D70DCE2` / `AA2B75BCC7E72DEE005A65B82137593394B1BD15ECFCEC1C4441C39E4EBC182D`；`r2-nte-taskrunner-vtable-methods-20261014.py/.txt` `AE1CDCDD96D40429FF37044AFE83128A28A9394148F80A247E9920A49067666D` / `009DA7443DD0A4534F99932524AE657CD396F1C49F82BBFDA9CE5070F8ED5300`。每个脚本先验证同一 `wrapper.node` SHA-256，并限于该映像 `.pdata` 函数、pair-copy helper 与 RTTI/vtable 数据。此前 `TaskRunner+0x58` 的归属描述根据本次函数级复核更正；这不改变未知任务语义和现场准入状态。

### 7.18 ThreadTaskManager TLS pair 来源与字段边界更正（2026-10-14，固定样本静态）

继续核验同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.6 按 `.pdata` 边界解码 `0x217D242/0x217D4FA/0x217D8CC/0x217DA24/0x217DA96` 与 helper `0x63EC4/0x1DB2/0xD32E5A/0xD32EDA`；没有启动或读取 QQ、调用原生函数、发送消息或操作 SnowLuma。

- `ThreadTaskManagerImpl` ctor `0x217D242` 以 16-byte 零写初始化 `this+0x30/+0x38`，另清零 `this+0x40`；dtor `0x217D4FA` 分别对 `this+0x40` 调 `0x217D4F2`（空值替换并释放旧 holder）及对 `this+0x30` 调 `0x1DB2`（释放 pair）。结合 `0x217D556` 清理从 `+0x48` 开始的 vector，字段边界应读作 `+0x30/+0x38` pair、独立 `+0x40` holder pointer、`+0x48` task vector；不由这些字段操作推出 runtime pointee 类型。
- manager slot 2 `0x217D8CC` 将 `this` 写入一个 TLS context 槽后，加载 `[this+0x30]` 作为 pointee，再调用 `0x63EC4` 复制该 pointee 自身 `+0x38/+0x40` 的 16-byte pair。`0x217DA24` 分配 `0x10` 字节；`0xD32E5A` 将该 pair 复制到分配块、对 source control block `+8` 增加强引用、调用 `0xD32DAD → 0x104C1C8` 安装到另一 TLS 槽，并释放临时 source pair。返回的 holder pointer 随后经 `0x217DEB0` 写入 manager `+0x40`；替换旧值时 `0x217DEC8 → 0xD32EDA` 释放其中 pair，再经 `0xB750029` 释放 16-byte block。由此更正 thread contract §3 的早期简写：安装目标不是已证的 manager `+0x30` pair 首 qword，而是从该字段 pointee 的 `+0x38/+0x40` 复制出的 pair holder；其 pointee RTTI 仍未知。
- manager slot 3 `0x217DA96` 同样先加载 `[this+0x30]`，再由 `0x63EC4` 投影该 pointee 的 `+0x38/+0x40` pair。它与 manager mismatch 路径不是同一地址来源。
- `0x31F88EA` 的 TID 不相等路径则把 manager 本身交给 `0x63EC4`，机械读取 manager `+0x38/+0x40` 并把复制结果首 qword 传给 `D32138`。但前述 ctor/dtor/slot-2 写入显示 `manager+0x38` 属于 `+0x30` pair，而 `manager+0x40` 是指向 heap pair holder 的独立 pointer；因此这里不是已识别的 `ThreadTaskManagerImpl` 自有 `{object, control}` pair。保留指令级 projection，不认定其 nested pointee、pair 有效性或故障原因，也不把 slot-3 对 manager `+0x30` pointee 的合法投影混为一谈。
- 本地固定-hash 脚本/地址级导出：`E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-manager-field-owner-clarification-20261014.py/.txt`，SHA-256 `ED299ABF97A4351292ED7370A941421D8F794B30D59F3E8147CEA17EA2ABE2BA` / `134996C57B11C70B644F97CC5AC827688AA5CC6A22E2D5307304B27D9CC10C27`。脚本先验证冻结样本 hash；pair helper 的机器行为与对象语义分开记录。manager `+0x30` pointee 来源、TLS slot0 实际目标类/方法、mismatch projection 的运行时合法性、thread stop/drain/quiescence、历史 AV 根因和 R3 identity 仍 unknown。本文更正不提升 R2/R3 gate，R4 仍不准入。

### 7.19 ThreadTaskManager `+0x30` 的条件写入与源 pair（2026-10-14，固定样本静态）

继续核验固定磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.6 按 `.pdata` 边界复核 `0x217D58C`、`0x217D6D8`、`0x217DEEA`、`0x217DF64`、`0x106E6` 与构造路径；没有启动或读取 QQ、调用 native、发送消息或操作 SnowLuma。

- §7.18 已确认 `ThreadTaskManagerImpl+0x30/+0x38` 的字段边界，但当时其 writer 未知。本轮在 manager 初始化方法 `0x217D58C` 找到条件 writer：`r15b` 从 0 递增，循环条件为 `r15b < byte[this+0x2C]`；每轮先把生成的 task pair 送入 `this+0x48` vector helper `0x31F8378`，随后只有 `r15b == byte[this+0x2D]` 才调用 `0x106E6(&this+0x30, &local_pair)`。这两处 byte 仅按机器码作用称为 loop bound 与 selector，不为其业务配置命名。
- `0x106E6` 对 source control word 非空时先增加强引用，把 source 的两个 qword 复制到 destination `+0x30/+0x38`，再以 `0x1DB2` 释放被替换的旧 pair。因而已闭合一条真实的 conditional pair assignment；它不表明每条初始化路径都满足 selector。若循环不进入或没有相等的 byte index，本方法不会写该 pair；只有在不存在其它 writer 的路径上，构造函数零值才会保留。本节没有对整个映像的其它潜在写者做否定扫描。`0x217D6D8 → 0x217DEEA → 0x217DF64/0x1676E` 生成 source local pair；helper `0x2509E` 的产物类型、source object RTTI/用途及 TLS/dispatcher 身份仍未识别。
- 现有 slot2 `0x217D8CC` 与 slot3 `0x217DA96` 会从 `[this+0x30]` 取 pointee，再用 `0x63EC4` 读取该 pointee `+0x38/+0x40`。`0x63EC4` 对传入 pointee没有空指针检查。本轮已为两个精确构造点复原 loop 配置并证明其 selector 命中，见 §7.20；但这只覆盖 30 个 `0xD31686` 直接调用点中的两条代码路径，未将任一静态路径绑定到 T1–T5 的被观测 manager，也未封闭 slot2/slot3 的全局调用者顺序。`0x31F88EA` mismatch route 对 manager 本体作出的跨字段 projection 仍与此合法 field writer 分开，不能据此命名其 nested target或归因历史 AV。
- 固定-hash 复核脚本/输出位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-manager-writer-source-context-20261014.py/.txt` SHA-256 `0F47D5E0E51DBA9432C4AF2AA4CF37EBFB25F48B3929112D84BDB36C9F989BC7` / `C4D82FE82F24900CE03EC003E024786694240FBF41C2D10A18C723614346877C`。脚本先校验冻结样本 hash，导出限于所列 `.pdata` 函数。此项不关闭 manager owner/thread/teardown、全局 callback drain/quiescence、R3 stable identity 或历史 AV；R4 仍不准入，产品 NOT READY。

### 7.20 ThreadTaskManager 配置字节、TaskQueue RTTI 与两个命中路径（2026-10-14，固定样本静态）

继续核验磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.7 对 `.pdata` 函数作有界解码；`0xD32508`/`0xD32512` 是不带 `.pdata` 的短 helper，报告只解码其相邻有界窗口。没有启动/读取 QQ、调用 native、发送消息或操作 SnowLuma。

- `0xD32508` 将 `DL/R8B` 写到目标相邻两字节，`0xD32512` 复制一个 WORD；`0xD324EC` 用 `{1,0}` 调前者；`0x4E6E5A` 先清零 config 首 DWORD、再调用 `0xD324EC`。所以输入给 `0xD31686` 的 6-byte config 布局可按实际写入追踪到 `ThreadTaskManagerImpl+0x28..+0x2D`，其中 `+0x2C` 是 loop bound byte、`+0x2D` 是 selector byte。
- 固定构造路径 `0x9C0C68 → 0xD31686` 在 `0x9C0E42` 清零 config 首 DWORD；`0x9C12A2` 读取其首 DWORD 作为值传递给下游，函数体没有把源地址再传递或向源写入；随后 `0x4E6E5A` 重置 config，`0xD32508(...,3,1)` 与 `0xD32512` 将尾部两字节改为 `{3,1}`；`0xD3251C` 将首 DWORD 与尾部 WORD 复制给 `0xD31686`。`0xD31686 → 0x217D16C → 0x217D242` 保持该 6-byte 布局并复制到 manager `+0x28`。该路径的 `+0x2C=3, +0x2D=1`，故 `0x217D58C` 遍历 index 0、1、2，并在 index 1 执行 pair writer。
- 第二个直接调用点 `0xB81758 → 0xD31686` 先清零 8-byte 栈槽，再由 `0x4E6E5A` 设置尾部 `{1,0}`，传入 config 的可读 6 bytes 为 `{0,0,0,0,1,0}`。对应 `+0x2C=1, +0x2D=0`，初始化循环进入一次并在 index 0 写入 pair。两个 callsite 均在 `0xD31686` 中先把 manager shared holder 安装到外层 `+0x110`，再同步调用 `0x217D58C`；之后才调用线程创建 helper `0xD33418`。这是具体 setup 顺序，不证明所有方法或调用者都遵循同一时序。
- RTTI/xref 复核确认 `0x217DEEA → 0x217DF64` 构造的控制块类型为 `std::__shared_ptr_emplace<TaskQueue, allocator<TaskQueue>>`，pair 的 object 指向控制块内嵌 `+0x18` 对象；source 因此可命名为 `TaskQueue` shared pair。两个已追踪输入路径会命中 writer，缩小了此类实例的空 pair 风险；全映像 30 个 `0xD31686` 直接调用点中其余路径、异常/失败边界，以及该 manager 与 T1–T5 动态 dispatcher 的对应关系仍未闭合。不能推断 slot2/slot3 所有调用都晚于该初始化，也不能把此结果归因为历史 AV。

固定-hash 复核文件在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-manager-config-producer-20261014.py/.txt` SHA-256 `FF9E4C0D8EEACCB05A6E5079E00592495DE53426BC8E5A5A782D519FEFD04FBF` / `0056F41CCFC68695173E6C051E1B08CB7EB80EF7AC7B993DC0FBE2649B66DBB1`；`r2-manager-init-dispatch-rtti-20261014.py/.txt` `AD02A313D5336241E0012DCAE643E0CF6C738A0CDAA7D422C16405167B8F908A` / `DC964B06DDF2237C9CD6207ABC3DA95D021B56923DCD78A2A3301FCEA78F1930`。脚本对样本 hash 作校验；报告不提升 R2/R3 gate，R4 仍不准入，产品 NOT READY。

## 8. 对实现的硬约束

1. Rust FFI 声明只能引用本文 `[verified]` 的几何与函数地址；`[inferred]` 项进入实现前必须先闭合。
2. 一切从 QQ 返回的 pair 按 opaque 处理：只在 FFI 边界用两个 pointer-width words 保存 `{obj,ctrl}` 并原样传给 `1DB2`；禁止解引用/修改控制块内部计数或把其布局当作稳定 C++ 类型。
3. 释放必须走 QQ 侧函数；禁止跨 allocator。
4. `732A80` 同步深复制 `[begin,end)` body；本侧 body input 可在函数返回后经配对 `B75029` 释放。callback reason/body 仍只在 callback 调用期内有效，进入 Rust 前完成拥有复制。
5. 已取 pair 在尚未 submit 且 attach 失败时可走 `1DB2`；尝试过任何 native submit 后，不在 adapter Drop 中释放，直到 callback/dispatcher quiescence 有证据。
6. 任何向 QQ 注册 Caligo callback 的路线都必须先 pin bridge image 到进程终止；pin 失败时在 native submit 前拒绝。没有 callback quiescence 证据时，不恢复热卸载。
7. 本文件全部结论基于静态固定样本与所列离线实现；不构成任何现场准入门（G1–G4）的通过证据。

### 7.12 `HttpServer` 替代 factory 的 R8D 分支与候选查找（2026-10-14，静态）

本节复核 §7.11 的两个分支，仍只读取冻结磁盘样本 `wrapper.node`，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。这里把机器级分支值与业务语义分开：`0x3458A18` 保存 `EDX`，实际以 `R8D` 作零/非零分支判断；目前没有证据把该 DWORD 命名为业务状态或 callback 结果。

- `0x345927C` 将 `dword [rdx]` 传到 factory 的 `R8D`，并将其输入对象 `dword [rdi+0x10]` 传到 `EDX`。直接 fallback caller `0x3458736` 在 `0x3458899` 传 `EDX=0`、`R8D=0`，同时传入从 `HttpServer+0x38` 移出的 pair。零分支随后递增 `DWORD [HttpServer+0x58]` 并进入 `0x3458B00`；这里只记录写入与路由，不给 `+0x58` 指派 generation/业务 ID 语义。
- `HttpServer` ctor `0x3457D10` 对 `this+0x20` 与 `this+0x60` 调初始化 helper `0x3BA6→0x3CEA`；`0x3CEA` 将 header 首 qword 指向 `base+8` 并清零后续 16 字节。dtor `0x345808E` 对这两个字段调用 `0x3FBAC6`，递归遍历 node `+0/+8` links，析构 node `+0x28` 的 shared-pair，再经 `0x4C5C8` 将 factor 1 与 node 指针传入 sized helper。`0x6AB34/0x6AB9C` 显示 `HttpServer+0x20` 是 tree-shaped header、`+0x28` 是 sentinel；`0x6AB9C` 按 node `+0x20` DWORD 做有序下界搜索，`0x6AB34` 再检查查询键是否等于候选键，不等时返回 sentinel。键源追到 `0x345927C`：factory 收到的查找键来自其输入对象 `+0x10` DWORD；命中的 node pointer 随后作为 `R8` 传入 `0x5D763A`。故这里静态证明的是按该 DWORD 精确命中后删除树节点，不只是一条可能 erase 的结构形状。键的产品意义仍未绑定，也没有 RTTI 证明具体 C++ 容器模板。
- 命中时，`0x5D763A` 以搜索返回的 node pointer 调 `0x1086E`：该 helper 取得 successor、在删除根节点时更新 root、递减 tree size 并调 `0x108CC` 执行链接/颜色重平衡。随后 `0x5D763A` 析构 `node+0x28` 的 pair，并将 node 指针与因子 1 交给 `0x4C5C8`；后者把 `0x30` 大小交给 `0x1FD0`。这证明精确键命中后的 RB-tree erase 和节点 payload 清理；pair pointee 类型、key 的业务含义以及这一操作在上层 callback 流程中的目的仍 unknown。factory 丢弃该 helper 的返回值，不能把它称为 QQ 响应 handler。
- §7.11 的“state 0/非零 state”仅描述机器分支。精确表述是 factory 的 `R8D` 零/非零 gate；其来源只追到 invoker 经 `0x345927C` 的寄存器传递，调用时业务意义仍 unknown。查找键的机器来源是同一输入对象 `+0x10` 的 DWORD，RB-tree 按此键精确选择节点；该键是否为 callback/request/business ID 未知。替代路径仍是通用 `HttpServer@httpx`；不证明 QQ response consumer、回调线程、global manager ABI、teardown 或 quiescence。

可复现脚本及地址级导出仍在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`：`r2-httpserver-weak-capture-and-state-route-20261014.py/.txt`、`r2-httpserver-branch-helper-disasm-20261014.py/.txt`、`r2-httpserver-branch-helper-raw-20261014.py/.txt`、`r2-httpserver-owner-container-disasm-20261014.py/.txt`、`r2-httpserver-container-lifecycle-20261014.py/.txt`、`r2-httpserver-container-helper-raw-20261014.py/.txt`、`r2-httpserver-tree-update-disasm-20261014.py/.txt`、`r2-httpserver-sized-node-helper-raw-20261014.py/.txt`、`r2-httpserver-tree-erase-helper-disasm-20261014.py/.txt`。两个 branch-helper 脚本读取前校验 SHA-256；`0x6AB9C` 报告明确标为非 `.pdata` 函数起点的有界指令解码。

### 7.14 `NTEngine` parent field release 与 slot-1 边界（2026-10-14，静态）

继续核对冻结磁盘 `wrapper.node`，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 `.pdata`、RTTI、反汇编；没有启动/读取 QQ 进程、调用原生函数或操作 SnowLuma。

- RTTI 将主 vtable `0x3FAD868` 归属 `.?AVNTEngine@nt@@`；slot 0 deleting-destructor thunk `0x9C2C6C` 调析构主体 `0x9C15D6`。构造路径 `0x9C0C68` 初始化 parent 字段对 `+0xE0/+0xE8` 与 `+0xF8/+0x100`；从这些字段取得的子对象指针会进入 `0xD31686`。该 helper 在其输入子对象基址 `+0x110` 安装 ThreadTaskManager holder 并调用启动方法。
- NTEngine 析构主体先对 `this+0x150` 执行虚槽 `+0x10`，随后调用 `0x9C1700`，再分别处理 parent 的 `+0x150/+0x140/+0x130/+0x120/+0x110/+0x100/+0xF8/+0xE8/+0xE0` 字段。机器码显示 NTEngine 自身 `+0x110` 走 `1DB2` pair release；`+0xE0/+0xE8` 与 `+0xF8/+0x100` 通过既有 owner/pair helper 拆分清理。由此只证明 parent 析构路径会释放其持有的字段引用。
- 偏移必须按基址区分：`NTEngine this+0x110` 是 parent 自身字段；`D31686(input_child)` 写入的是输入子对象的 `child+0x110`。当前静态材料未证明后者的最后一个 owner 何时释放、child destructor 是否运行、holder 何时销毁，也未证明 dispatcher 停止或排空。析构中对 `this+0x150` 的虚调用及 `0x9C1700` 的完整业务语义/时序合同仍未解析。
- NTEngine vtable slot 1 `0x9C2458` 将 `[this+0x110]` 作为第一个参数传给 `0x173D042`，但该有界 leaf 不读取 RCX，只在第二参数所指输出位置写入 `{self,self,0}` 形状后返回；这不是 teardown 证据，类型与语义仍 unknown。
- 构造 helper `0x9C12A2` 在 `0x9C1309` 对绝对地址 `0x180D3111E2` 发起 direct call。该 VA 在 `wrapper.node` 与精确版本磁盘 `QQNT.dll` 的 preferred-image 范围之外；把假设 load base `0x180D000000` 代入会得到 QQNT RVA `0x3111E2`，但没有当次模块基址/重定位证据，且该 RVA 落在函数 `0x3111A0–0x3113F3` 的分支中段。因此不将目标归属给 QQNT，也不据此命名子对象或归纳其 owner 合同。

可复核导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：基础固定-hash 脚本/输出 `r2-nte-engine-child-owner-dtor-20261014.py/.txt`，补充 parent release 记录 `r2-nte-engine-outer-release-20261014.py/.txt`（补充脚本/输出 SHA-256 分别为 `74DFE345F4CA226D4303688DFCBA1ADB80375CE5361E1084CA49BDFB0647237A` 与 `3C50FC314BB039D3731A45C9392DC408B9AC1A9D61D0AB6CEFE903CDBBBC98D6`）。补充脚本校验基础脚本 hash；基础脚本再校验 `wrapper.node` 与磁盘 `QQNT.dll` hash。固定映像只读证据只收窄 NTEngine parent 字段释放路径，不闭合 child last-owner teardown、全局 manager ABI、callback/consumer thread、QQ stop/cancel/drain/quiescence、历史 AV 或 R3 stable identity；R4 准入及 NOT READY 状态不变。

### 7.15 child `run_loop_thread.cc` stop 与 wait-handle 路径（2026-10-14，静态）

复核同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Ghidra 12.1.4 `-noanalysis` 对精确 `.pdata` 范围定向反汇编/反编译，并用 Capstone 5.0.6 复核指令边界；没有启动/读取 QQ 进程、调用原生函数、发送消息或操作 SnowLuma。

- shared-control release path `0x1B643C0` 取出 control `+0x8` 的 child 指针、先清空该槽，再调用 child dtor `0xD31356`，最后释放 child；`0xD31356` 先直接调用 `0xD313D6`，随后才对 `child+0x110` 调 `1DB2`。因此，**当且仅当该控制块释放路径实际到达**，child dtor 会先执行下述 pre-cleanup，再释放其 manager holder。何时到达该释放路径、是否为最后 owner，仍未证明。
- `0xD313D6` 的日志源字符串指向 `..\foundation\xplatform-ng\xpng\task\run_loop_thread.cc`，固定样本还含 `Stop Thread Begin`、`Stop Thread Waiting Join`、`Stop Thread Join Done` 与 `Stop Thread thread is null, return`。函数先检查 `child+0x29` guard 与 `child+0x110`，设置 guard 后经 `0xD31AC0` 从该 holder 取 pair。
- `ThreadTaskManagerImpl` 有效 RTTI vtable RVA `0x4260448` 的 slot 1 指向 `0x217D860`。该槽从 `ThreadTaskManagerImpl+0x48` pointer-backed task vector 依 byte index 复制一个 shared pair。`0xD313D6` 把 `R8` 设为 RVA `0x3FED904` 的地址；固定样本该地址首字节为 `0`，`0xD31AC0` 保留此寄存器，slot 1 读取 `[R8]`，故该静态路径取 vector index 0。随后 `0xD32138` 对所取对象调用 vtable slot 0，并传入当前 child 与一个 0x10 字节上下文。任务构造走 `0x217D58C → 0x217D6D8 → 0x217DEEA → 0x217DF64` 一带的类型擦除/helper 路径；该 entry 的动态类与 slot 0 的代码语义仍 unknown，不能命名为 manager 全局 stop 或 callback drain。
- 若 `child+0x10` 非空，`0xD313D6` 调 `0xD3355C`；Ghidra 反编译为 `WaitForSingleObject(*handle, 0xFFFFFFFF)` 后 `CloseHandle(*handle)`。wait 返回值没有被检查；调用者随后无条件写 `child+0x10=0` 并记录 Join Done。故静态可确认的是 stop 日志/虚调用路径和无超时 wait + close 的代码形状；日志本身不证明 wait 成功或线程已退出。空句柄分支记录 thread-is-null 并继续清理。
- 这比 §7.14 多闭合一个 child 局部 stop/wait 路径，不闭合 child last-owner / 析构是否触发、`+0x10` handle 的创建与对象类型、index-0 task 的动态类型、ThreadTaskManager 其它 entry 的停止、QQ callback queue drain 或 global dispatcher quiescence。不能将该证据映射成 QQ response consumer 已安全停止，也不能据此解释历史 AV 或放行 R4。

固定 hash 可复核脚本/输出位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-thread-stop-path-20261014.py/.txt`（脚本/输出 SHA-256 `2D62A6D711792A6277B9E855B78B408E90AB772E600518E5DB496B4E8A5FBE4C` / `CB8531753CDC53A97F6CAA149DB07EECF98F19B0E5D53E6C1365AC797F7ED069`），以及 `ghidra-script\DumpNteCleanup.java`、`DumpNteCleanup2.java`、`DumpNteManagerBuild.java`、`DumpNteTaskFactory.java`、`DumpNteTaskFactoryImpl.java` 和 `ghidra-cleanup-20261014\` 定向输出。Ghidra scripts 在读取前核对目标 SHA-256；`.pdata` 与函数导出是有限定界分析，不是全映像语义恢复。
### 7.16 child+0x10 HANDLE 的 `CreateThread` 写入来源（2026-10-14，静态）

继续核对同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。按有效 `.pdata` 边界导出 `D31686` / `D33418`，将 PE IAT 映射为 `KERNEL32.dll!CreateThread`，并用直接 callsite 与 bounded callback-target 导出交叉检查；不读取或操作运行中的 QQ。

- `D31686` 在调用 `D33418` 前于 `D31779` 计算 `rax = child + 0x10`，将其写入 Win64 第五参数槽 `[rsp+0x20]`；`D31782` 的 RIP-relative 地址是 callback thunk `D31958`，`R9` 保持 child 指针。直接调用位于 `D31791`。
- `D33418` 把 callback 与 child 放入线程启动 context，以固定 trampoline `D334B7` 调 `KERNEL32.dll!CreateThread`。非空返回值保存在 `rsi`；成功分支从本函数入口对应的第五参数槽取出 out-pointer，并在 `D3348F` 写入 `*out = handle`。因此在 `D31686` 这条固定样本路径上，`CreateThread` 成功返回的 HANDLE 写入 `child+0x10`。CreateThread 返回空时走错误分支并返回失败，不在该分支写入句柄。
- 线程 callback 指针 `D31958` 是跳转到 `D31B7E` 的 thunk；有界解码显示 `D31B7E` 接收同一 child、写入 `child+0x60` 当前线程 ID，并通过 `child+0x110` manager holder 作虚调用。它不能识别 manager 的完整任务语义，也没有证明 response callback queue 已排空。
- 与 §7.15 对照，析构前 `D313D6` 读取的正是该 `child+0x10`：非空时传入 `D3355C`，对句柄执行 `WaitForSingleObject(INFINITE)` 和 `CloseHandle`。等待结果仍未被检查，随后清槽和 `Join Done` 日志不证明等待成功或线程已退出。

这闭合的是固定样本中 child 局部线程 HANDLE 的成功创建写入点以及对应的 wait/close 读取点；不证明 child 的最后 owner 或析构时机、等待成功、ThreadTaskManager 所有 entry 均停止、global QQ callback queue drain/quiescence、R3 稳定身份、历史 AV 根因或真实响应交付。R1–R3 门仍未闭合，不能开始 R4。

本机可复核文件位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-thread-create-path-20261014.py/.txt` SHA-256 `291651009C1DC4F324D3B5A29696C6569DAD73D611D9DF9085552DFEFE0B6610` / `92EEC121172EF753FC7DB7B3760598391A8B952EA9A0D6CB82CE39ADB329540F`；`r2-nte-thread-create-xrefs-20261014.py/.txt` `0D064B7FD07D80B07EBB1B4A0B12103382C4CF0CF4F81359742DDB8CC77CADBE` / `DC151632F85148DEE445CFCE06EE5FAE23579B560390CC907A46E9FB33AE88CF`；`r2-nte-thread-callback-target-20261014.py/.txt` `D035913AAB71B8F7EFBC284F14488C2F66FCA5AF907C78564FAF004417941AA1` / `DACF2D290604E84A77CBA72F0FB9EEDD23434E0F8E7603A6D47FE83982E2A03F`；PE import/callsite inventory `r2-nte-handle-writer-scan-20261014.py/.txt` `58D147AF23E96AB389C67088448D2B26886ED6D36BB50D0F1D561C97228D973C` / `2567B63F946C700DC36AD465639ACDE95843AFB09D8645FC8AE278C1C58B9F96`。这些脚本均对样本 hash 作校验；扫描仅覆盖映像导入与 `.pdata` 有效函数边界，不能据缺失 callsite 推断不存在路径。

### 7.21 全部 30 个 `D31686` 直接调用点的 manager 配置来源（2026-10-14，固定样本静态）

对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）用 Capstone 5.0.7 按有效 `.pdata` 范围枚举并校验 rel32 `E8` 直接调用，找到 30 个 `0xD31686` callsite。逐点反汇编其配置指针来源后，30 个 callsite 归入以下三个静态模板：

- 28 个通过 `0x4E6E5A` 构造六字节配置；该 helper 清零首 DWORD，再在 `+4` 调用 `0xD324EC`，后者写入字节 `{1,0}`。调用点为 `0x004E69EB`、`0x0072001C`、`0x007200A3`、`0x007268C0`、`0x009C14BD`、`0x009C4951`、`0x009C5529`、`0x009DA908`、`0x00AFD53C`、`0x00AFF5BA`、`0x00B5C5AA`、`0x00B817C7`、`0x00CA955E`、`0x01739F3A`、`0x017730C2`、`0x0185856A`、`0x01858664`、`0x0185875E`、`0x01858EAE`、`0x01914351`、`0x019913DF`、`0x0208A9DE`、`0x0214DD92`、`0x0290C85C`、`0x030323A5`、`0x033D6045`、`0x033D8A15`、`0x033D93BB`。配置为 `{0,0,0,0,1,0}`，即 manager `+0x2C=1`、`+0x2D=0`。
- `0x01B63FF7` 先把六字节配置所在 qword 零化，再将 `0xD324EC` 直接用于配置基址 `+4`，得到同一 `{0,0,0,0,1,0}`。
- `0x009C0EE1` 先用默认构造器初始化配置，再由 `0xD32508` 形成临时字节 `{3,1}`、`0xD32512` 复制到配置 `+4`，最后经 `0xD3251C` 克隆后传给 `0xD31686`。其六字节输入为 `{0,0,0,0,3,1}`，即 bound 3 / selector 1。

因此，在这 30 个已找到的**直接 E8 setup 路径**各自被执行且 `0xD31686` 完成其 manager setup 的条件下，静态输入的 loop bound 均非零且 selector 均落在 loop 内：默认配置选择 index 0，唯一自定义配置选择 index 1；两者都会使 `0x217D58C` 的条件 `+0x30/+0x38` pair assignment 分支可达。这里的“分支可达”来自配置值和静态控制流，不证明任一现场调用实际发生、source pair 在现场非空、slot2/slot3 全局调用顺序，或这些 manager 对应 T1–T5 观察到的 dispatcher。枚举只覆盖映像中可识别的直接 rel32 `E8` xref，不把间接调用、条件路径或未解析边界外推为已覆盖。

逐调用点窗口与 helper 导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-d31686-callsite-windows-20261014.py/.txt` SHA-256 `D61918FE955A54A5A715AEFD44BD7BC2E4904D52DCC6A440015FEB025C1A678C` / `1E6A0E4544F26DBCA95C2DB17B3A409E131D37722D94190AA502B205595EDDB9`；配置 leaf `0x4E6E5A/0x4E6DAC/0x4E91A6/0xD32508/0xD32512/0xD3251C/0xD324EC` 的 bounded 解码 `r2-config-helper-leaves-20261014.py/.txt` SHA-256 `F3A047C4F7F5986FF0101B2562EF85E985F0C2C42E7A24938128BE6CCE4C55CE` / `7C34E8DA05BCA9FEFAC0E3FAC414803A05B56CEB4F862E418E36E00CDFD0EE1A`。两脚本均先校验冻结样本 hash；本项不识别任一 runtime manager 的身份、last owner、析构时机、stop/cancel/drain/quiescence、R3 identity 或历史 AV 根因，R4 仍未准入。

### 7.22 ThreadTaskManager 与 TaskQueue 间的弱引用边（2026-10-14，固定样本静态）

对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）按有效 `.pdata` 边界核对 `0x217D16C`、`0x217D35E`、`0x1687C`、`0x168CE`、`0x217D58C`、`0x46448`、`0x75ADB0` 与 `0x31F8F5E`；用 `.rdata` COL/RTTI 识别两种 shared control block。仅分析固定磁盘样本，没有读取/启动 QQ、调用 native 或操作 SnowLuma。

- `0x217D35E` 为 `ThreadTaskManagerImpl` 构造 shared holder：控制块 RTTI 是带 custom deleter 的 `shared_ptr<ThreadTaskManagerImpl>`，manager 自身作为 strong pair 返回。它随后把该 strong pair 转给 `0x1687C`，目标字段为 `manager+0x10`。`0x1687C` 对 weak 来源执行可存活检查/临时 strong lock，再由 `0x168CE` 增加控制块 weak 计数并写入 16-byte pair；`0x168CE` 的释放旧值路径与同组 pair helper 对应。因此 `ThreadTaskManagerImpl+0x10` 是 weak self-reference，不会单独维持 manager 的强生命周期。
- `0x217D58C` 的逐项初始化从 `manager+0x10` 经 `0x46448` 做 weak-lock 形状的 strong acquire；`0x75ADB0` 在 strong counter `+8` 非 `-1` 时原子递增，否则返回空。其结果交给 `0x31F8F5E` 写入 `TaskQueue+0x48`；该位置经 `0x168CE` 更新 weak counter `+0x10`。TaskQueue control block RTTI 为 `__shared_ptr_emplace<TaskQueue,...>`，故此反向字段也是 weak backreference。
- 同一逐项路径另把 `TaskQueue` strong pair（增加 `+8` 后）放入 `ThreadTaskManagerImpl+0x48` pointer-backed vector；命中 selector 时也把 strong pair 交给 `0x106E6`，写入 manager `+0x30/+0x38`。所以这里排除了特定的 `ThreadTaskManagerImpl → manager vector → TaskQueue → manager+0x48` 强引用环；它不识别全局 dispatcher，也不排除 manager 的其他强 owner 或其他类型的环。
- 该发现不证明 manager 的最后一个 strong owner、外层销毁时机、child+0x110 holder 何时释放、所有 TaskQueue/TaskRunner 的 stop、线程 wait 成功、QQ callback queue drain/quiescence、R3 stable identity 或历史 AV 原因。不能单凭弱回边把 child teardown 或 R4 准入判为已闭合。

本机可复核脚本/输出位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-manager-weak-selfref-closure-20261014.py/.txt` SHA-256 分别为 `0971FD81D5C8E6A08DEC2F9AE9E88C6F77C8756FE7A308BCA84AB8AF6CCB205D` / `301421BFB9A9C02119117742FA0E0FD135A1340FCFBC559429D4F03A25F0D4D7`。脚本在解析前校验冻结样本 hash；helper 的 counter 语义只按有界机器码得出，不代表现场对象状态。

### 7.23 TaskRunner 的 manager 弱引用与 NTEngine parent release（2026-10-14，固定样本静态）

继续核对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）。只读 PE、有效 `.pdata` 函数和 RTTI；没有读取/启动 QQ、操作 SnowLuma、调用 native 或发送消息。

- `0x217D58C` 从 `manager+0x10` weak-lock 得到临时 strong pair 后，将它传给 `0x31F878C` 初始化 `TaskRunner`。该函数将 pair 的对象/control 指针写入 `TaskRunner+0x8/+0x10`，然后对 control block `+0x10` 执行原子递增；同一控制块布局中 strong count 位于 `+0x8`，weak-copy helper `0x168CE` 也递增 `+0x10`。因此 `TaskRunner+0x8/+0x10` 是 `ThreadTaskManagerImpl` weak pair，不是 strong owner。TaskRunner slot 0/3 的代码从 `this+0x8` 经 `0x3F70` 获取临时可用对象后再读取 manager `+0x58`，与弱引用 lock 路径相符。
- 合并 §7.17/§7.22 的强弱边，已追出的路径是 `ThreadTaskManagerImpl+0x48 vector --strong--> TaskQueue --strong--> TaskRunner --weak--> ThreadTaskManagerImpl`；故这一条曾看似可能的三段环并非强引用环。此结论只覆盖这些已识别字段，不证明没有其它强持有路径、现场对象执行过构造，或 manager 的最后强 owner 已找到。
- `NTEngine` ctor `0x9C0C68` 建立 child holder 并经 `0xD31686` 完成 manager setup；另调用 `0xD31AC0`，其 manager slot 1 以样本 RVA `0x3FAD8D0` 的索引字节 `1` 选出 manager vector 中的 TaskRunner pair，再复制到 NTEngine `+0xE8`。这为该外层对象增加一条独立 TaskRunner 引用。RTTI vtable slot 0 的 thunk `0x9C2C6C` 转入 destructor body `0x9C15D6`；其 cleanup 调用 `0x9C1700`，在 child `+0x110` 与 `+0xE8` pair 非空时对该 runner 经 `0xD32138` 发起 slot-0 虚调用，随后释放临时 pair，并清理/释放 NTEngine `+0xE8` 与 `+0xE0` 字段。固定样本对 ctor 的有界直接 E8 扫描找到 `0x9C1928→0x9C0C68` 初始化路径；没有据此识别 dtor 的实际注册或触发者。
- 上述 parent release 与 runner slot-0 调用是候选 teardown 序列，不证明 NTEngine dtor 实际到达、该 runner/manager 与现场 dispatcher 对应、所有 manager entry 已停止、句柄等待成功或 callback queue 已 drain/quiescent。`TaskRunner` 弱边消除了这条候选 cycle，但 child/manager last-owner、外层 holder 释放时机、R2 consumer/ABI 与 R3 identity 仍未闭合；R4 不准入。

本机固定-hash复核位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-cycle-lifetime-20261014.py/.txt` SHA-256 `01F3881AB2F28E2B6F80CBA60F85AAC9A9C79CE1D93BCD18474A6A81F585E6AC` / `D037AFACF1D797ABE0F3D031D838C68B6A44A1744B63763A54652D08E840BD88`；ctor/dtor xrefs `r2-nte-engine-ctor-dtor-xrefs-20261014.py/.txt` `F95799643D1C66817539FE139A9AB5FC73F321EA7BD1990ABA53BE9D8E535C54` / `E8BCDF065491C96A565B1D7FD191A9DBEC4F77A09A32964AE420468D8A7905A7`；parent pair helpers `r2-nte-parent-field-helpers-20261014.py/.txt` `94A4F3F8FA7547982C343D99DA7D7CD554331EF99F7BF66B6B5D63D2A71087D6` / `C9D22F2D908069A3824FC8BD53A6BF1D4F4E122E8D5861A2D8F5D56A5A670A88`。脚本先校验固定 `wrapper.node` hash；函数/调用者筛查有界，缺失 xref 不能证明无间接路径。

### 7.24 NTEngine 静态访问器调用面与 finalizer 未知（2026-10-14，固定样本静态）

继续在同一冻结 `wrapper.node` 上做有效 `.pdata` 与已解码直接 E8/RIP-relative 扫描；SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`，不访问运行中的 QQ 或 SnowLuma。

- `0x9C1928` 读取 module 的线程初始化 epoch、TEB TLS 与 guard，并在构造路径将静态存储 RVA `0x6750088` 交给 `0x9C0C68`，完成后执行 `0x3E67E74` guard footer 并返回该存储地址。guard 位于 RVA `0x67501E8`。该函数表现为 NTEngine 的 guarded module-static accessor；有效 `.pdata` 中识别到 796 个直接 rel32 `E8` 调用点，说明它是广泛使用的外层对象入口，不证明每个调用发生或其 manager 与 T1–T5 dispatcher 对应。
- NTEngine vtable slot 0 的 dtor thunk 为 `0x9C2C6C`，bounded 直接 E8 scan 找到 0 个调用者；析构主体 `0x9C15D6` 的唯一直接 caller 是该 thunk。访问器反汇编显示 ctor 后调用 guard footer，但没有显式传入 dtor 指针或呈现 registration 调用；这不能排除函数指针/on-exit/runtime-finalizer 或模块退出路径。故 guarded static object 可列为 child manager holder 的外层 owner 候选，但对象最终化是否注册/执行、何时释放 manager last strong owner仍未知。
- 对整个冻结文件中的精确 preferred-VA qword 扫描，`0x18009C2C6C` 仅在 `.rdata` RVA `0x3FAD868` 的 NTEngine vtable slot 0 命中一次；dtor body `0x18009C15D6`、静态 object VA `0x186750088` 与 guard VA `0x1867501E8` 均无 qword 命中。这只约束精确绝对 qword 编码，不排除相对/包装指针、运行时 on-exit 注册或间接 finalizer。
- 本结果补充 §7.23 的 parent `+0xE0/+0xE8` 释放路径，不证明它在现场到达、析构序列成功、manager 所有 TaskRunner/TaskQueue 停止或 callback queue drain/quiescent。R2 ABI/consumer/teardown 与 R3 identity 仍开放，R4 不准入。

本机固定-hash脚本/输出均先校验输入 hash：`E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-static-owner-xrefs-20261014.py/.txt` SHA-256 `4157196D980BDEDC4161A4701FD9C51A306326CEC68F7205D3B8B4CAC2BEDF84` / `4830002B2C977B6ECDD832EAA3398DC2D8DB8228443B0F7AAD5CB265B1BC8F58`；guard helper `r2-nte-init-guard-followup-20261014.py/.txt` `825419BA062ED5C93145B98DE66D6A8FC968317E99F94E5B9A4B86F2147FCB8B` / `6E4D3838B605F7ACFA56300B1400A4359B373C98CB70BA69D1E79139C245C6A6`；精确 preferred-VA qword scan `r2-nte-static-pointer-table-20261014.py/.txt` `0E1193A77227C71963BE391B2305AF9F404FE3BB776D77C147C2AE427FB5BA33` / `EA52C144DEE55422A23BA348F812111B2500A3D3623FA356BD6EFA2B5532BE30`。调用者覆盖只含映像中解码的直接 E8、RIP-relative 代码引用与本次列明的精确 qword 值，不能据此排除间接生命周期路径。

### 7.25 PE TLS callbacks 与析构 RVA 命中分类（2026-10-14，固定样本静态）

继续核对同一冻结 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 PE 导入、TLS 目录、`.pdata` 与数据段；没有读取或操作 QQ/SnowLuma，也没有调用 native 或发送消息。

- PE TLS callback array RVA `0x490A2F8` 登记两个 callback：`0x3E68820` 与 `0x3E67F70`。按 Windows TLS callback reason，`0x3E68820` 只在 reason `2`（`DLL_THREAD_ATTACH`）进入；它计算的固定表 begin/end 均为 RVA `0x490A2A0`，区间为空，故此 callback 在该映像里不会调用表项。其间接 callback helper 位于 `0x3EB1DE0`，机器码仅 `jmp rax`。`0x3E67F70` 处理 reason `0`（`DLL_PROCESS_DETACH`）或 `3`（`DLL_THREAD_DETACH`），从当前线程 TLS 项 `+0xB0` 逆序取 callback 并逐项调用，再推进/释放列表；所以 process-detach 下存在一条间接 TLS callback-list 执行路径。该函数本身没有直接引用 NTEngine static RVA `0x6750088` 或调用 NTEngine dtor，但列表注册者尚未找到，不能排除 NTEngine dtor 被间接放入该列表。
- 导入表中没有名字匹配 `atexit`、`_onexit` 或 `__cxa_atexit` 的项目。此前识别的 guard footer `0x3E67E74` 的间接目标已解析为 `AcquireSRWLockExclusive`、`ReleaseSRWLockExclusive` 与 `WakeAllConditionVariable`，不能据此视为析构注册 API。PE 未单独保留 `.CRT` section；这只限制本次可见的常规注册线索，不证明没有内部/间接 finalizer。
- 对 `.rdata/.data/.tls/_RDATA` 做对齐 DWORD direct-RVA 与 rel32-from-next 搜索时，析构 body RVA `0x9C15D6` 唯一命中在 `.rdata` RVA `0x4C81BC8`。该位置通过析构 body 自身 `.pdata` 记录 `0x9C15D6–0x9C16E9`、UNWIND_INFO RVA `0x4C81B74` 连到 handler data；其中 EH func-info magic `0x19930522` 指向 unwind map RVA `0x4C81BC0`，该命中是析构自身异常/清理元数据中的 action RVA，并非退出注册表项。析构 thunk `0x9C2C6C` 没有在该对齐 RVA/rel32 扫描中命中；精确 VA qword 结果仍限于 §7.24 所列 vtable。
- 对 TLS index RVA `0x6755690` 的有界常见 RIP-relative 访问筛查，1499 个 `.pdata` 有效函数命中候选；按 `GS:[0x58] → TLS index → TLS block` 数据流和寄存器别名筛选后，仅 callback `0x3E67F70` 显式访问 TLS block `+0xB0`，且它在清理时读当前 head 并写回 next pointer。该 pass 未识别注册 writer；覆盖限于脚本列出的指令编码与直接 TLS-block 数据流，不能据此断言不存在经 wrapper/helper 传递的 producer。
- 因此进程退出 callback 的动态列表仍是候选间接 finalizer 通道；NTEngine module-static destructor 是否注册到该列表、是否经其他内部/包装机制触发、实际执行时机和 manager last-owner 仍 unknown。唯一静态析构 body RVA 命中已归类为自身异常处理元数据。TLS 观察不证明 teardown 安全，也不闭合 manager 全任务 stop、callback drain/quiescence、R2 consumer/ABI 与 R3 identity；R4 仍不准入。

本机复核脚本和输出均先校验固定输入 hash：`E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-finalizer-callbacks-20261014.py/.txt` SHA-256 `9F7ECEA08E80E93F9C0FDEA381D5ECE02EDFFA0E0FB308579FCD6B7197087415` / `7961CE0BD7A817ECD36CADBFEA2AA8DF8622EE80C3EC3E4B086470DCAD2EA50B`；对齐 RVA/rel32 扫描 `r2-nte-relative-finalizer-refs-20261014.py/.txt` `8ADECBB4A43BD670D184E71EEE2881AA7E78B9F20A72E87587CF04084ED672A2` / `91E49F940B139FB133D0F5BF2682DC23F88807DB4ADE74355E10DD3A1AAC3058`；析构异常元数据分类 `r2-nte-dtor-rva-eh-classification-20261014.py/.txt` `B70EF37673DF52362765D6B5B6AA08D540855BC9A347E6922108BC1EBC979CBF` / `52C38B6F5F67388E8054608AFA10A116271FA6B9498D9D6407217E13C31A7E26`；TLS index/block `+0xB0` 数据流筛查 `r2-nte-tls-list-dataflow-20261014.py/.txt` `9F941B1E03E9B99D92BDEC8D7B7981A47788C12BFFF5928CD207DDCB692DB812` / `7F77AB4D9ECB647F4049C2917315BBDC248A64BC5A6C8AA730CBA672E2F1141B`。扫描有明确 section 与编码边界，不覆盖未列出的包装指针或运行时状态。

### 7.26 TLS detach callback dispatch, node release 与 producer 有界追踪（2026-10-14，固定样本静态）

继续使用冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）做 PE/`.pdata`/Capstone 静态分析。未读取或操作 QQ/SnowLuma、未调用 native、未发送消息。

- `0x3E67F70` 在 `0x3E67FC5` 从当前 TLS 节点取 callback 地址到 `RAX`，随后 `0x3E67FCD` 通过 `.rdata` RVA `0x4909B18` 的 qword 调用 `0x3EB1DE0`；该 helper 的完整函数体只有 `jmp rax`。节点首 DWORD 控制逆序遍历的 callback 项，节点 `+0x8` 是随后读取的 next pointer。代码调用 `0x3E94F90` 释放已处理节点，再把 TLS `+0xB0` head 推进到 next node。
- `0x3E94F90` 没有独立 `.pdata` 项，机器码为跳转 thunk `jmp 0x3EA01D0`。有效函数 `0x3EA01D0–0x3EA020C` 将输入指针作为 `HeapFree` 的第三参，调用 KERNEL32 `HeapFree`；失败路径调用 `GetLastError` 并把结果写入 CRT 错误槽。这里确认了 detach list 的 callback dispatch 与节点释放路径，未给出节点注册者。
- 对 TLS index RVA `0x6755690` 的 1499 个 `.pdata` 候选扩展 `mov/lea`、寄存器别名、常数位移及 Win64 参数传播追踪。候选函数产生 37 个传递 TLS block/pointer 的直接调用边；继续沿 helper 追到 46 个直接转发边、50 个函数/参数状态。该范围仍只在 `0x3E67F70` 发现 TLS block `+0xB0` 读写，没有 helper 写入。8 个间接调用位点均解析到系统导入：`AcquireSRWLockExclusive`、`GetCommandLineA`、`GetLastError`、`SetLastError`。
- 本轮没有定位 TLS list producer。数据流范围受 TLS-index 指令模式、常见寄存器/`lea`/加减追踪和直接调用边限制；未覆盖栈 spill、任意计算地址或运行时改写函数指针形成的所有路径。不能据此证明不存在其他 writer，也不能证明 NTEngine 析构已注册到 detach list 或会在模块退出时执行。

固定输入 hash 校验与导出脚本/输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-finalizer-helper-detail-20261014.py/.txt` SHA-256 `4CEEAC3702F52718131C26F90F1CCFD6D33631E33B7CE7BF08651076790824EA` / `277211C674A7F6A846FBB18E8A3449329A3B867F61DF9C78CA0D0323FBC01FD9`；`r2-nte-indirect-targets-20261014.py/.txt` `C99B976C58FCDED904082D424D005601B0992DE297355E1B951A0D59167B8D6D` / `ED3854B333BC58CF7C5C91839F7341CEC06AAF2A198E47233163616DD0206116`；`r2-nte-tls-producer-dataflow-20261014.py/.txt` `61396632EBF203C51D5945CE4542D7B78D7696334244AB6FB8942367C9FA9994` / `356F1F9773EEF265CD0038F13E926A8D90526A8F7E602C20F5D1BD48065FB295`；`r2-nte-tls-helper-bfs-20261014.py/.txt` `968074BDE5A4751DA4C6568D2DE2DFFA8E6ED1C3757C58C44345E8C2E7779437` / `B3F72D88761F4AE7F667C1280C15890B3A796AB6CEC9B7D4D924750FD4329A6A`。分析结论不关闭 manager last-owner、`child+0x110` holder release、全任务停止/排空、R2 consumer ABI/thread、R3 stable identity 或历史 AV；R4 仍不准入。

### 7.27 NTEngine `+0xE0` raw owner 到 child destructor 与 manager-holder release（2026-10-14，固定样本静态）

继续核对同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Capstone 5.0.7 复核有效 `.pdata` 函数与无 `.pdata` 的 helper 基本块；没有启动/读取 QQ、操作 SnowLuma、调用 native 或发送消息。

- 构造 helper `0x9C12A2` 通过 `0xB75000` 请求 `0x120` 字节，并把返回地址经输出槽交给 `NTEngine` ctor `0x9C0C68`。随后 ctor 将该 raw pointer 写入 NTEngine `+0xE0`，并将它传给 `0xD31686` 建立 child manager。helper 还在 `0x9C1309` 调用绝对 VA `0x180D3111E2`；该地址在冻结 `wrapper.node` 映像外，当前证据不足以归属模块或解释其语义，不能由假设 load base 推出 QQNT 归属。
- NTEngine dtor body `0x9C15D6` 在 `0x9C16B8` 把 `this+0xE0` 交给 `0x4E6872`。该无 `.pdata` helper 清零 RDX 后转入 `0x4E91A6`；后者先清空 owner 槽，若旧指针非空则进入 `0x4E91BE`，调用 child dtor `0xD31356`，随后 tail-call `0xB75029` 释放旧对象。因此固定样本静态析构路径已把 NTEngine `+0xE0` raw owner 接到 child destructor。该路径仍以 NTEngine dtor 实际执行且 `+0xE0` 非空为条件。
- child dtor `0xD31356` 先调用 `0xD313D6` stop/wait 路径，然后在 `0xD31371` 对 `child+0x110` 调 `0x1DB2` 释放 manager shared holder。这个释放点现在已静态定位；结合 §7.15–§7.16 的限制，它不证明 wait 成功、manager 全部 task 已停止、QQ callback queue 已 drain/quiescent，或该 holder 是 manager 最后一个 strong owner。
- NTEngine `+0xE8` 是单独 pair 字段：dtor 在 `0x9C16AC` 直接调用 `0x1DB2`；不把它与 `+0xE0` raw owner 或 child 基址 `+0x110` manager pair 合并。此前 §7.23 的 runner teardown 路径仍是另一条独立证据。
- 因而已知的是“条件性 parent `+0xE0` 清理 → child dtor → child `+0x110` holder release”代码边；仍未知的是 guarded NTEngine static destructor 是否注册/实际触发、外层 owner 何时销毁、manager 最后一个 strong owner、R2 consumer ABI/thread 与全局 stop/drain/quiescence、R3 stable identity 和历史 AV 根因。R4 仍不准入，feat-005/006 与产品 NOT READY 状态不变。

本机固定-hash复核脚本/输出位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-parent-child-release-chain-20261014.py/.txt` SHA-256 `71AC9C97880E2CDB3A6C666008BAE2737516838A83098138D8F56B78D70CD000` / `60FC9433BC72619EE95924493AFFDECB6C9497E0CECB1F7F0839AB9333BEEFCF`。脚本先验证输入 hash；导出列出 `0x9C12A2` allocation/call、NTEngine ctor/dtor 窗口、完整 `0xD31356` child dtor 与 raw helper blocks。`.pdata` 定界和 helper 基本块分析均有限，不凭缺失调用者证明析构已触发或不存在其他 owner 路径。
### 7.28 `MSFThread_xpng` owner path to `ThreadTaskManagerImpl` (2026-10-14, fixed-sample static)

Using the same frozen disk `wrapper.node`, SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`. The bounded Capstone scans assert this exact hash; no QQ process or SnowLuma interaction occurred.

- The image has 525,295 valid `.pdata` function ranges. The earlier census found 30 direct rel32 `E8` calls to `0xD31686`; only the direct-call chain `0xD31686 → 0x217D16C → 0x217D242` was found for manager factory/constructor targets in that pass. It also found three direct `D31356` call sites; two belong to the `MSFThread_xpng` construction-replacement and deleting-destructor paths below. These are direct-code findings, not coverage of indirect calls.
- `0x1B64310` builds the shared pair for an inline `MSFThread_xpng` object. Its RTTI vtable at RVA `0x4141408` names `.?AVMSFThread_xpng@MSF@@`; slot 0 is `0x1B643C0`, slot 2 is `0x1B63FC0`. `0x1B63EC0` initializes the wrapper's `+0x8` payload pointer by allocating `0x120` bytes and calling the absolute target `0x180D30FF0`; that target lies outside the frozen wrapper, and its module identity and constructor semantics are unknown.
- Vtable slot 2 `0x1B63FC0` reads the payload at `this+0x8`, obtains the six-byte default manager config through `0xD324EC`, calls `0xD31686(payload, config)`, then calls `0xD31A9A(payload)`. The established `D31686` contract places the manager holder at `payload+0x110`; this is a second static owner shape beyond the NTEngine `+0xE0` raw-child path in §7.27.
- `0x1B64310` has three verified direct callers: `0xB90C40` at `0xB90DE8` and `0xB90E7E`, and `0x1BBF810` at `0x1BBF95B`. In the first function, the returned pairs are stored at global pair addresses `0x677A2D0` and `0x677A2E0`; the same function then calls vtable offset `+0x10` at `0xB90E5E` and `0xB90EF4`. The third caller stores the returned pair in its owner at `+0x8` and calls vtable offset `+0x10` at `0x1BBF9C8`. Those offsets match the RTTI-identified slot 2. This proves the static setup calls for these three constructed wrappers; it does not establish their runtime execution or lifetime.
- `0x1B643C0` is the `MSFThread_xpng` deleting-destructor path: it clears the wrapper's `+0x8` payload pointer, calls `0xD31356` when non-null, frees the payload, and conditionally frees the wrapper. The assignment helper `0x1B63EC0` likewise calls `D31356` before freeing an overwritten non-null payload. Thus the path from an outer `MSFThread_xpng` shared owner to `payload+0x110` manager release is statically present, conditional on the wrapper release/replacement path actually executing.
- This closes an additional manager-holder owner shape but not the last strong owner of any observed manager, finalizer registration/trigger, a successful wait, all-task stop, callback queue drain/quiescence, response-consumer ABI/thread, stable native identity, or the historical AV. The external constructor at `0x180D30FF0` remains unbound to an exact module hash; do not infer a QQNT base or constructor contract. R1–R3 remain open, R4 is not admitted, and product readiness stays NOT READY.

Reproducible local-only artifacts under `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\` (each script asserts the frozen wrapper hash):

- factory census `.py/.txt`: `5641C34341A52649AF35AA0006750AB8633576F3B5E7835D76B3DB342393AC72` / `010E96D89C8FD99531A71F1F090F64EF16F185D3D85CEED20B51A5320C679740`;
- D31356 callers `.py/.txt`: `48AAC286A4E61F722CB9EF97481F004ACE7AFF66386F3AA862013DBD01CD8D8F` / `41495D7904A4E1D0F94BD31144F7A01484AC744413CB17E824D62289D30ED3E6`;
- RTTI/control context `.py/.txt`: `ABEEC04D01D17AB18691EDFC7EA9C7A987EA163F9948CF9C3C07C6E880794E6B` / `86FAE7E168D8F26F6CA0772FDC57AD311D3EE48B61FD6F6BFD523691D61AC616`;
- factory direct callers `.py/.txt`: `75A0D4BD99ADBBB5C458FF75FD32F71390043C0D78C3B56810470E2D6488D631` / `E22B1A26E3B3C5DBB2CE1F5EE249D4F3E790AF3D0535355648F6950ACD03E672`;
- manager setup method `.py/.txt`: `FA211374C6B6F9B293F8D6CA4F9680CF1D5FB494CA45F57F58A460A5E5A0DED1` / `48AF450B0A11D0FE4884DE0DEF5A16030567A9556A2741FD4BD3636C14A77005`;
- vtable method references `.py/.txt`: `E8DB92387F93C5C5A084D2BA39CF8BA1A6BEF7750C92CC796AFB8C09CAF0B2DE` / `82D6B7981C49CE25F978A557B7C97D7DB4DEC8878D41D715768BF0C6B1B555C1`;
- vslot-2 callers `.py/.txt`: `AFB80F0AED9A8801721A074553E653A597004B06B644E1BDE9A9E392C868022A` / `03481711C4C31BD7B050DC612858E065D3FA926404C93C48CCD30EA924514098`.

### 7.29 `ThreadPoolImpl` callback-manager owner-stop path (2026-10-14, fixed-sample static)

The same frozen `wrapper.node` was rechecked by bounded Capstone decoding after validating SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`.

- RTTI identifies the vtable at RVA `0x4300918` as `.?AVThreadPoolImpl@internal@xpng@@`; slot 2 is `0x290C746` and slot 0 is `0x290DA46`. In slot 2, `0x290CC0F` produces a local pair-shaped value, `0x2EC8` assigns it to `this+0xA0`, and the method loads the field's first qword before direct call `0x290C85C → 0xD31686`. This is a type-specific manager setup edge; the pair pointee's dynamic identity is not established here.
- Slot 0 reaches `0x290C59C → 0x290C612`. If the `this+0x20` state check through `0x69A08(..., 2)` takes its cleanup branch, `0x290C612` passes `[this+0xA0]` to `0xD313D6`, then calls `0x504AEC` on the pair field. That helper copies 16 bytes, zeroes both source qwords, and releases the copied pair through `0x1DB2`; the outer cleanup wrapper also reaches `0x1DB2` for `this+0xA0` after the conditional base cleanup.
- This adds a concrete `ThreadPoolImpl` setup-to-stop/reset/release path. It remains conditional static code: it does not prove an instance reached setup or teardown, that `+0xA0` is the final strong owner, that `D313D6`'s wait succeeded, that all callback work drained, or that this relates to the historical AV. The manager payload RTTI, indirect consumer ABI/thread, dispatcher-wide quiescence, and stable native identity remain open; R1–R3 stay open and R4 is not admitted.

Local fixed-sample artifacts in `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\` (the script asserts the frozen image hash): `r2-threadpool-manager-owner-stop-chain-20261014.py/.txt`, SHA-256 `C5D69CE9ABCC3BD1EC657FB815F401478B105A60EDF1F92C029128EC9202F8EE` / `A0AE6A23EB7117F3A35F09443E96E22AB4A4F1477BC853FE148F86F86DFCC9BE`.

### 7.30 `FileDragManagerWin` owner-to-child destructor path (2026-10-14, fixed-sample static)

The same frozen `wrapper.node` hash was checked by a separate bounded Capstone/RTTI script: `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`.

- RTTI identifies the vtable at RVA `0x3FADD38` as `.?AVFileDragManagerWin@nt@@`; slot 2 is `0x9C4852`, slot 0 is `0x9C5462`. Slot 2 transfers the first qword from a local pair-shaped value into owner field `this+0xA8`, reloads that field, and calls `0x9C4951 → 0xD31686` with the default manager config. The manager holder is installed in the child payload at `+0x110`; this static path does not identify the payload's dynamic RTTI.
- Slot 0 reaches `0x9C47DE`, which clears the owner field through `0x4E6872 → 0x4E91A6`. If the old payload pointer is non-null, `0x4E91BE` calls `0xD31356` on it and tail-calls `0xB75029` to free it. The established `D31356` body reaches `D313D6` and releases the child `+0x110` manager holder.
- This is an additional static owner-to-child teardown path. It does not prove runtime construction or destructor execution, manager last-owner status, successful wait, all-task stop, callback drain/quiescence, response-consumer ABI/thread, stable native identity, or the historical AV cause. R1–R3 remain open and R4 remains not admitted.

Local artifacts in `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\` (the script asserts the frozen image hash): `r2-filedrag-manager-owner-stop-chain-20261014.py/.txt`, SHA-256 `EAE21E4CE8660D1C3852DF941D8C6517AE222F86DF27924A76450183E7B8A66A` / `FBF5CD676007342D1E0A3D2D97170A0D1BD56BFD262CA8D050E37DD85C7B837C`.
