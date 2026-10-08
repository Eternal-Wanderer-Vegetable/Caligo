# QQ 原生线程合同（G-THREAD）——固定 9.9.33-52230 静态证据 R3

> **R3.1 增补（2026-10-08 现场观测）**：§10 记录了在真实 QQ 实例（PID 47524，创建 2026-10-08T11:58:14Z UTC）上的外部只读观测结果——本文 §6 的 `this+0x60` 未知已被**现场+静态交叉闭合**，§3 的 TLS dispatcher 身份已定性。§1–§9 为静态首轮内容，其中与 §10 冲突的表述以 §10 为准。

日期：2026-10-08。对象：QQNT `9.9.33-52230/resources/app/wrapper.node`，115,118,632 字节，SHA-256 `63112ab9161e127f5f7e17998a7196e143808923fb54cbbf7b4e21426187a5f0`，x64 PE，image base `0x180000000`。**下文全部地址为 RVA**。方法：只读文件级扫描（导入表/异常表/字节形态）+ Ghidra 12.1.4 对既有 `QqWrapper52230` 项目的定向 `-noanalysis` 反编译（phaseA–J，共 ~180 个函数），证据树：`E:/stella/_reference/qq-native-r3-p1-20261008/`。未执行目标、未注入、未触碰 QQ 进程。Ghidra 伪代码不是符号事实；ABI 结论以汇编为凭。

本文是 R2（`qq-send-thread-contract.md` / `qq-receive-thread-contract.md`）的增量，不重复其已证结论。状态标记：`[verified]` = 本轮汇编/反编译/文件证据；`[corrected]` = 更正 R2 结论；`[unknown]` = 未闭合。

## 1. 结论摘要

1. `D32138` 的语义需要更正：它不是"执行器派发"，而是**通用任务调用原语**——对传入对象的 vtable **slot0** 做一次 4 参间接调用 `[verified]`。
2. 任务提交的目标对象来自**每线程 TLS 槽**（index 单例 RVA `6753454`），槽内是 `{对象, 控制块}` pair，安装时对控制块 `+8` 做原子强引用递增 `[verified]`。
3. 该 TLS 目标对象的安装点已定位到 worker 线程上下文初始化（`217D8CC → 217DA24 → D32E5A`），对象本体是 worker 上下文 `+0x30` 成员 pair 的首 qword `[verified]`；**其具体类别与 slot0 函数体仍未闭合** `[unknown]`。
4. R2 的 Core executor 单例（`9C0C68` 构造、静态存储 `750088`、vtable `3FAD868`/`3FAD8A8`）被证实存在且结构完整，但**它与 TLS 安装的 dispatcher 是否同一对象尚无证据**；R2"共同调用 D32138 不构成同一线程模型"的告诫被进一步强化 `[corrected]`。
5. 发送主链 `732A80` 在 `732EB9` 处 `this+0x60` 的 vtable+0x38 调用，其安装来源在本轮有界扩展（ctor、getter 全部调用方、77 个 MSF vtable 槽、executor 辅助链、login 链）内**仍未找到写入者**；候选淘汰表见 §6，按计划失败分支转受控观测方案 `[unknown]`。
6. login 观察者接口形状已闭合：`LoginRequestImpl`（RTTI 名 `nt::login::LoginRequestImpl`）vtable slot1 = 成功记录通知、slot2 = 状态码通知，均经 `this+0x20` 弱 pair 锁定后调用 `[verified]`。**session-ready 的充分条件仍未知** `[unknown]`。

## 2. D32138 = 通用任务调用原语 [corrected]

汇编（[R2 phase2/assembly/d32138.txt](E:/stella/_reference/qq-native-r2-20261008/phase2/assembly/d32138.txt)）：

```
180d32167 CALL 0x18031c5f8        ; 移动 callback 管理结构到局部(31C5F8)
180d3216c MOV RAX,qword ptr [RDI] ; RDI = param_1
180d3217d CALL qword ptr [RAX]    ; slot0(RCX=param_1, RDX=param_2, R8=&mgr副本, R9=&{0,0})
180d32185 CALL 0x180002632        ; 释放源 callback 管理(RCX=原 param_3)
```

- 调用的是 `*param_1` 的 **slot0**（字节偏移 0），参数 `(self, param_2, 移动后的 callback 管理副本, 指向 {0,0} 局部的指针)`。
- 返回值 int32 透传给调用方。
- R2 将 `param_1` 称作"executor 对象"。汇编只证明"param_1 的 slot0 被调用"；**param_1 的类别由每个调用点的实参决定**，不是单一全局执行器。

## 3. TLS 槽：每线程提交目标与安装链 [verified]

### 3.1 TLS index 与访问族

| 函数 | 作用 | 证据 |
|---|---|---|
| `D32DAD` | index 单例 guard（index 存 RVA `6753454`，guard `675345c`；TlsAlloc 经 `C72C36`） | [R2 phase2/functions/d32dad.c](E:/stella/_reference/qq-native-r2-20261008/phase2/functions/d32dad.c) |
| `D32D98` | 取当前线程 TLS 槽值；未设置返回 0 | [R2 phase2/functions/d32d98.c](E:/stella/_reference/qq-native-r2-20261008/phase2/functions/d32d98.c) |
| `D32E3E` | 判 TLS 槽非空 | [phaseD/functions/d32e3e.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/d32e3e.c) |
| `D32E5A` | **安装器**：`{obj, ctrlblk}` 复制入新 0x10 pair，`LOCK inc ctrlblk+8`（强引用获取），`TlsSetValue`（thunk `104C1C8`），释放源 pair | [phaseD/functions/d32e5a.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/d32e5a.c) |

- 直接调用 `D32DAD` 的只有 3 个函数（D32D98/D32E3E/D32E5A）——访问族封闭 `[verified]`（字节级 E8 扫描，`p1-caller-xrefs.json`）。
- `D32D98` 全进程约 450 个直接调用方（通用设施，非 MSF 专属）`[verified]`。
- 生命周期含义：安装即强引用获取；getter **不增计数**（借用语义，仅在所属线程内有效）。

### 3.2 安装点链

```
217D8CC (worker 线程上下文收尾/初始化)
  ├─ TlsSetValue(217D40D 的 index, worker 上下文自身)     ; 另一个 TLS 槽(工作上下文)
  ├─ 63EC4(worker+0x30, out)                             ; 取 worker 的 dispatcher pair
  └─ 217DA24(&newpair, dispatcher_pair)
        └─ D32E5A(newpair, dispatcher_pair)              ; 写入 D32DAD 的 TLS 槽
```

证据：[phaseE/functions/217da24.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseE/functions/217da24.c)、[phaseF/functions/217d8cc.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseF/functions/217d8cc.c)。

**推断（[inferred]）**：每个 worker 线程启动时把"自己的 dispatcher"装进自己的 TLS 槽；任务提交者（任意线程）读取 TLS 槽得到的是**本线程**的 dispatcher。该推断若成立，意味着"提交发生在哪条线程"决定哪个 dispatcher 执行——这正是 Caligo 合同必须显式约束的点；但 dispatcher 类别/slot0 体未闭合前，不得据此宣布任何线程合法。

## 4. MSFService 接收链的目标对象 [verified 结构, unknown 主体]

`1B3F740`（[R2 phase2/functions/1b3f740.c:39-52](E:/stella/_reference/qq-native-r2-20261008/phase2/functions/1b3f740.c)）：

```c
puVar3 = FUN_180d32d98();      // 取本线程 TLS pair
uVar1 = *puVar3;               // pair[0] = 目标对象
...                            // 把 request-holder/packet-holder 移动进新闭包 auStack_68
FUN_180d32138(uVar1, auStack_68, auStack_a0);   // 目标.slot0(目标, 闭包, callback管理)
```

即：接收结果处理把"后续工作闭包"提交给**当前线程 TLS 槽中的对象**。若该线程没有安装（TLS 空），`D32D98` 返回 0，`*0` 解引用崩溃——**在没有 worker 上下文的线程上跑 `1B3F740` 是崩溃路径** `[inferred，未经现场复现]`。

## 5. Core executor 单例（R2 遗产）的边界 [verified, 关系 unknown]

- 单例对象静态存储 RVA `750088`（getter `9C1928`，guard `7501E8`，构造 `9C0C68`）。
- 构造写双 vptr：`+0 = 3FAD868`、`+8 = 3FAD8A8`（文件级验证 + rip 引用扫描 `0x9C0CA1/0x9C0CAB`）；第二处构造点 `9C15D6`（即析构，重置同两个 vptr）位于 `9C15D6..9C16E9`。
- vtable `3FAD868` slot0 = `9C2C6C` = 标量删除析构包装（`{ 调 9C15D6; flag≠0 → B75029 释放 }`，[phaseA/functions/9c2c6c.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseA/functions/9c2c6c.c)）。
- 构造参数形状：`0x32`(50) 与 `200` 两个数值进入调度设置；`9C11DA/D32508/D32512/D3251C/D31686` 为启动辅助；失败路径日志后 `4E91A6(puStack_50,0)` 复位。
- **不能**把该单例与 §3/§4 的 TLS dispatcher 混同：目前没有证据表明 `750088` 对象被装入任何线程的 TLS 槽（安装链源是 worker 上下文 `+0x30`，`63EC4` 的返回类别尚未还原）。

## 6. `this+0x60` transport 安装来源：候选淘汰表 [unknown]

| 候选集 | 结果 | 证据 |
|---|---|---|
| MSFService ctor `72DF56` | 仅初始化，无写入（R2 已证，本轮复核） | R2 send contract §4 |
| getter `72DE38` 全部 5 个调用方 | 无 `+0x60` 指针写入 | phaseA（`p1-caller-seeds.json` 派生） |
| MSFService/CoreService 全部 vtable 槽（77 个唯一 `.text` 函数，phaseB） | 无写入；`7318E2` 是最深的消费者（读 `+0x60` 判空 → 用其 slot2/slot8，还写 `+0x1F0` 命令串） | [phaseB/functions/7318e2.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseB/functions/7318e2.c) |
| executor 辅助链（`D31686` 等） | 命中均为 4 字节 dword 域（`+0x5F/+0x60/+0x61/+0x62` 连续 dword 数组），非 qword 指针——计划预警的" dword 误读"实锤 | [phaseA/functions/d31686.c:23](E:/stella/_reference/qq-native-r3-p1-20261008/phaseA/functions/d31686.c)、`5bc28.c:200` |
| login 链（`6E65E8` 及 LoginRequestImpl 构造/槽） | 无 | phaseA/phaseD |

**按计划 §7-P1 失败分支处理**：不试猜偏移、不扩大现场注入。后续闭合路径为受控观测方案——在指定测试实例上，对 `732A80` 入口做只读观测（记录 `this`、`*(this+0x60)`、该指针 vtable 前 8 slot、线程 TID、对象代次），一次实验只解决"安装来源 + slot0 actual target"一个未知。该方案需要执行者指定实例与窗口，本文不附带任何现场命令。

## 7. login 观察者接口（session-ready 信号的形状）[verified]

RTTI（文件级 COL 解码，`dump-p1-vtables.py` 输出 `p1-vtable-dump.json`）：

- vtable B（COL `3F67E10`）类名 = **`nt::login::LoginRequestImpl`**；slot0 `6E7F80`、**slot1 `6E65E8`（成功处理）**、slot2 `6E71F4`（状态码通知）、slot3 `6E72F0`、slot4 `6E7F52`。
- 构造 `6E5B80`（[phaseD/functions/6e5b80.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/6e5b80.c)）：三 vptr（A `3F67B58` / B `3F67BB8` / C `3F67BE8`）；`+0x18` 与 `+0x170` 两个 `{obj, ctrlblk}` pair 均以 **`LOCK inc ctrlblk+0x10`**（弱计数）获取——弱引用成员；`+0x180` 容器由 `6C49D4/6C4A20` 从 param_3 构造。
- slot1（R2 §6 已证）：锁 `this+0x20` 弱 pair → strong，复制 record，调用观察者 vtable **+8**（slot1）传 `(obs, 0, record)`。
- slot2 `6E71F4`（本轮新增）：带 32 位状态码 param_3，锁同一弱 pair，调用观察者 vtable **+0x10**（slot2）传 `(obs, 0, code)`。

**观察者接口形状**（被通知方）：slot1 = 登录成功记录；slot2 = 登录状态码。观察者实现与注册方未闭合 `[unknown]`（下一步：`6E5B80` 的调用方 → 谁以什么观察者 pair 构造 LoginRequestImpl）。**非零 UIN / SSO 层可读 / 观察者被调用，任一单独都不构成 session-ready**（R2 结论维持）。

## 8. 对 Caligo 合同的约束（可执行）

1. **禁止**把"D32138 存在"或"TLS executor 存在"当作"任意线程可调用"的证明。提交目标对象随线程而变（[inferred]），在无 worker 上下文的线程上走 `1B3F740` 型路径是崩溃/未定义路径。
2. 自有 native adapter 的调度合同必须绑定**具体对象 + 具体 slot0 函数体**（即 P1 下一闭合项），不得按"QQ 有线程池"抽象通过。
3. 在 §6 闭合前，发送路径（`732A80`）维持**拒绝写**状态；G3 不解封。
4. TLS pair 是强引用语义：自有实现对任何从 QQ 取回的 pair 必须成对释放（见 [qq-native-lifetime-contract.md](qq-native-lifetime-contract.md) §3），不得复刻 SnowLuma 的三处丢弃模式。
5. 观测点若被安装在 `1B41AE6`/`1B3F740` 链，必须假设回调可能出现在"安装时不可预期的 worker 线程"，记录 TID 与对象代次（§6 受控观测方案的输出字段）。

## 9. 本轮新增证据索引

全部位于 `E:/stella/_reference/qq-native-r3-p1-20261008/`：

- 脚本：`scan-p1-caller-xrefs.py`（E8/E9 调用方扫描）、`dump-p1-vtables.py`（vtable/RTTI COL 文件级解码）、`scan-p1-ripxref.py`（RIP-relative 引用 + PE 导入表 TLS IAT 定位）、`scan-*.json` 输出。
- Ghidra 导出：`phaseA/`(90) `phaseB/`(77) `phaseC/`(33) `phaseD/`(20) `phaseE/`(12) `phaseF/`(5) `phaseG/`(5)，每 phase 含 `functions/*.c`、`assembly/*.txt`、`functions.jsonl`、`summary.json`；种子 `seeds-phase{A..G}.json`。
- 关键单文件：`phaseD/functions/d32e5a.c`（TLS 安装器）、`phaseF/functions/217d8cc.c`（安装链源）、`phaseA/functions/9c2c6c.c` 与 `phaseD/functions/9c15d6.c`（单例删除链）、`phaseD/functions/6e5b80.c`（LoginRequestImpl ctor）、`phaseB/functions/7318e2.c`（+0x60 消费者）。

## 10. R3.1 现场闭合：发送目标、dispatcher 身份与两链拓扑（2026-10-08）

观测工具 `caligo-cli observe-msf`（外部只读 RPM：`QUERY|VM_READ`，零注入/零写入/零 QQ 函数调用，不消耗实例 bootstrap）。目标实例：**PID 47524**（创建 2026-10-08T11:58:14Z UTC，QQNT 9.9.33-52230），登录完成后 t1/t2/t3 三次采样（间隔 ≥2 分钟），证据：`docs/execution/2026-10-08-native-contract-recovery/evidence/p1-observe-field-47524-t{1,2,3}.json`。

### 10.1 `this+0x60` transport：身份与实际目标 [live+static 交叉闭合]

- MSFService 单例（`+0x674D2C0` 槽）**已构造**：obj vptr RVA `+0x3F6DED8` **锚点命中**（确认加载二进制 = 固定样本）；strong 149→244→501（活跃使用），weak=14 恒定。
- **`this+0x60` 已安装**（三次采样同址）：transport obj vtable RVA **`0x41403B8`**（wrapper.node 内），恰 10 个虚槽后接非指针数据：

| slot | RVA | 备注 |
|---|---|---|
| 0 | `0x1B4E8B8` | 函数起点（.pdata） |
| 1 | `0x1B4E118` | 叶函数（无 unwind） |
| 2 | `0x1B4E134` | |
| 3 | `0x784A80` | 公共基类方法（他区） |
| 4 | `0x1B4E180` | |
| 5 | `0x1B4E384` | |
| 6 | `0x1B4E19E` | |
| **7 (+0x38)** | **`0x1B4E4EC`** | **发送路径实际目标（732EB9 调用点）** |
| 8 | `0x1B4E6A6` | |
| 9 | `0x1B4E846` | |

- vtable `0x41403B8` 的全部 rip 引用仅两处：**ctor `0x1B4DFF8`** 与 dtor `0x1B4E0DA`（§6 的"无写入者"就此终结——写入者此前不在任何已反编译集合里）。
- **`0x1B4E4EC` 定性**（phaseH 反编译，真函数起点 `0x1B4E4EC..0x1B4E619`）：锁 transport `+0x30` 的 pair 取**内联连接对象**（空 → `*out_token=0` 且返回 0 —— **未连接即同步失败，不排队**）；否则取 token（`0xB78D06`）、复制 command（经自身 slot3）、包装 callback（`0xB7F6F6`），转交 **`0xB7CE8A`**。
- **`0xB7CE8A` 定性**：校验内联对象 vtable `+0x30` 状态（`(state & 0xFFFFFFFE) != 4` → 失败分支）；组装闭包 {command 副本, request 接管, callback 包装, token}，经 **`D32138` 提交到 `0xB816E8` 全局对象的 `+8` 成员**（静态存储 RVA **`0x67510A8`**，guard `0x67510B8`；getter 的 init 桩 `0xB81748` 仅清零，真实构造经 `0x4E91A6` store 路径）。
- transport 对象出处：`0xB7C6C0`（0x70 字节分配 → ctor → `0xB7C89C` 挂释放钩子 `0xB7827E` → 存入 owner `+0x110`）；`0xB7C6C0` 本身经 manager 类 vtable（.rdata `0x3FD3E70` 槽）间接派发。**最后把该指针写入 MSFService `+0x60` 的那条 store 仍未单独定位**——但对合同不再是必要项：对象身份、slot 语义、状态检查、失败路径均已闭合。

### 10.2 dispatcher 类：两类提交目标同源 [live+static 交叉闭合]

- **TLS 提交目标**（index=44，D32DAD 持有）：145–150 线程中 **41 个已安装**，全部指向**同一 vtable RVA `0x43B6088`**；slot0 = **`0x31F8CEE`**（真函数起点）。
- **发送链全局 dispatcher**（`0x67510A8`）：**同一 vtable `0x43B6088`**，slot0 同为 `0x31F8CEE`。**发送与接收提交的是同一个类的两个实例**：发送用全局单例，接收结果处理用调用线程自己的 TLS 实例（§3/§4 的推断就此定版）。
- 类合同（phaseH/I 反编译）：ctor `0x31F878C`（写 vtable，`+8` 存 {内部队列对象, 弱 pair}，弱计数获取）；slot0 `0x31F8CEE`（锁 `+8` pair → 转交 `0x31F88EA`）；`0x31F88EA` = 入队 + **owner TID 自检**（当前 TID vs 对象 `+0x58`，跨线程走显式唤醒路径 `0x217DE40`）。
- **执行器单例 `0x750088` 现场未构造**（三次采样 vptr=None）——R2 §5 的 Core executor 单例不在本实例运行路径上；41 个 TLS dispatcher 与它无关（§5 的"关系 unknown"就此关闭：无关系）。
- **MSFCoreService getter `0x74203C` 未被调用**（`+0x674D3E8` 槽为空）——运行时 switch 实际选择了 MSFService。首个运行时分支证据。

### 10.3 发送/接收完整拓扑（闭合版）

```
发送: 732A80 ─ this+0x60 (transport vtbl 0x41403B8)
        └ slot7 0x1B4E4EC ─ 锁 +0x30 pair(内联连接; 空即失败)
            └ 0xB7CE8A ─ 状态检查(vtbl+0x30) ─ D32138 → 全局 dispatcher(0x67510A8, vtbl 0x43B6088)
                └ slot0 0x31F8CEE ─ 0x31F88EA 入队(+8 pair) + 跨线程唤醒
接收: 1B41AE6 ─ 1B3F740 ─ D32D98(本线程 TLS, index 44) ─ 同类 dispatcher(0x43B6088)
                └ slot0 0x31F8CEE ─ 同上入队
回调: 出队线程执行 → 73bd6c → 用户 invoker —— 即"QQ 自己的 worker 线程",
      不是提交线程,不保证恰一次;与提交者并发。
```

### 10.4 对 Caligo 合同的修订约束（取代 §8 中相应条款）

1. [原 §8.1] 提交目标不再是纯推断：**发送 → 全局 dispatcher 实例；接收 → 提交线程 TLS 实例**。自有 adapter 在 QQ 拥有的线程上执行时，两条路径的语义已可核对；"任意线程可调用"仍然不被证明——worker 上下文（安装 TLS 的那 41 线程）之外的提交行为仍未证。
2. [新] transport slot7 在**未连接时同步返回 0 且 token=0**——这是可依赖的失败信号（原生层可探测"会话未就绪"而无需发送）。
3. [新] 状态检查点：内联连接对象 vtable `+0x30` 的状态值（`4/5` 为可提交态）可作为原生 ready 证据的一部分（仍需与 session-ready 门联合判定）。
4. [新] callback 线程 = dispatcher 消费线程（QQ worker 池），与提交者并发——R2 §3"复制后异步"的约束维持并升级为实证背景。
5. 仍未闭合（记录在案，不阻塞 P2 的合同冻结）：dispatcher 消费侧（出队→执行的线程池关系）；MSFService+0x60 的最后一条 store；stop/cancel/drain 路径；b78d06 token 的语义（回执关联候选）。

### 10.5 R3.1 证据索引

- 观测报告：`docs/execution/2026-10-08-native-contract-recovery/evidence/p1-observe-field-47524-t{1,2,3}.json`（工具 `crates/caligo-cli/src/observe_msf.rs`，提交 6f85350+）。
- 静态导出：phaseH（30 函数：1b4e4ec/b7ce8a/b7c6c0/31f8cee/31f878c 等）、phaseI（b816e8/31f88ea 等）、phaseJ（b81748）；`E:/stella/_reference/qq-native-r3-p1-20261008/`。
- 字节级：transport/dispatcher vtable rip-scan（构造/析构唯一性）、`.pdata` 函数起点验证、`b7c6c0` 的 .rdata vtable 归属。
