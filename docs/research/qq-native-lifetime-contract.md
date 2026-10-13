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
- 尚未定位运行时把该字段从 empty 哨兵替换为 inline/heap manager 的赋值入口；也没有将其动态 manager 指针绑定到 `0x73BC5C/0x73C2AA`。op=1/op=2 外部 ABI 与完整 caller 集合、arena 分支寿命、manager/consumer owner thread、外层 teardown、QQ dispatcher stop/cancel/drain/quiescence、历史 `0xC0000005` 和 R3 stable identity 继续开放，R4 仍不准入。

可复现脚本/文本输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-httpconnection-callback-field-owner-20261014.py/.txt`。该脚本在读取前校验固定 hash；只含本机静态地址与 RTTI，不入库。

## 8. 对实现的硬约束

1. Rust FFI 声明只能引用本文 `[verified]` 的几何与函数地址；`[inferred]` 项进入实现前必须先闭合。
2. 一切从 QQ 返回的 pair 按 opaque 处理：只在 FFI 边界用两个 pointer-width words 保存 `{obj,ctrl}` 并原样传给 `1DB2`；禁止解引用/修改控制块内部计数或把其布局当作稳定 C++ 类型。
3. 释放必须走 QQ 侧函数；禁止跨 allocator。
4. `732A80` 同步深复制 `[begin,end)` body；本侧 body input 可在函数返回后经配对 `B75029` 释放。callback reason/body 仍只在 callback 调用期内有效，进入 Rust 前完成拥有复制。
5. 已取 pair 在尚未 submit 且 attach 失败时可走 `1DB2`；尝试过任何 native submit 后，不在 adapter Drop 中释放，直到 callback/dispatcher quiescence 有证据。
6. 任何向 QQ 注册 Caligo callback 的路线都必须先 pin bridge image 到进程终止；pin 失败时在 native submit 前拒绝。没有 callback quiescence 证据时，不恢复热卸载。
7. 本文件全部结论基于静态固定样本与所列离线实现；不构成任何现场准入门（G1–G4）的通过证据。
