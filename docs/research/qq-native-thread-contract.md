# QQ 原生线程合同（G-THREAD）——固定 9.9.33-52230 静态证据 R3

日期：2026-10-08。对象：QQNT `9.9.33-52230/resources/app/wrapper.node`，115,118,632 字节，SHA-256 `63112ab9161e127f5f7e17998a7196e143808923fb54cbbf7b4e21426187a5f0`，x64 PE，image base `0x180000000`。**下文全部地址为 RVA**。方法：只读文件级扫描（导入表/异常表/字节形态）+ Ghidra 12.1.4 对既有 `QqWrapper52230` 项目的定向 `-noanalysis` 反编译（phaseA–G，共 165 个函数），证据树：`E:/stella/_reference/qq-native-r3-p1-20261008/`。未执行目标、未注入、未触碰 QQ 进程。Ghidra 伪代码不是符号事实；ABI 结论以汇编为凭。

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
