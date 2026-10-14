# QQ 原生线程合同（G-THREAD）——固定 9.9.33-52230 静态证据 R3

> **R3.1 增补（2026-10-08 现场观测）**：§10 记录了在真实 QQ 实例（PID 47524，创建 2026-10-08T11:58:14Z UTC）上的外部只读观测结果——本文 §6 的 `this+0x60` 未知已被**现场+静态交叉闭合**，§3 的 TLS dispatcher 身份已定性。§1–§9 为静态首轮内容，其中与 §10 冲突的表述以 §10 为准。

日期：2026-10-08。对象：QQNT `9.9.33-52230/resources/app/wrapper.node`，115,118,632 字节，SHA-256 `63112ab9161e127f5f7e17998a7196e143808923fb54cbbf7b4e21426187a5f0`，x64 PE，image base `0x180000000`。**下文全部地址为 RVA**。方法：只读文件级扫描（导入表/异常表/字节形态）+ Ghidra 12.1.4 对既有 `QqWrapper52230` 项目的定向 `-noanalysis` 反编译（phaseA–J，共 ~180 个函数），证据树：`E:/stella/_reference/qq-native-r3-p1-20261008/`。未执行目标、未注入、未触碰 QQ 进程。Ghidra 伪代码不是符号事实；ABI 结论以汇编为凭。

本文是 R2（`qq-send-thread-contract.md` / `qq-receive-thread-contract.md`）的增量，不重复其已证结论。状态标记：`[verified]` = 本轮汇编/反编译/文件证据；`[corrected]` = 更正 R2 结论；`[unknown]` = 未闭合。

## 1. 结论摘要

1. `D32138` 的语义需要更正：它不是"执行器派发"，而是**通用任务调用原语**——对传入对象的 vtable **slot0** 做一次 4 参间接调用 `[verified]`。
2. 任务提交的目标对象来自**每线程 TLS 槽**（index 单例 RVA `6753454`），槽内是 `{对象, 控制块}` pair，安装时对控制块 `+8` 做原子强引用递增 `[verified]`。
3. 该 TLS 目标对象的安装点位于 `217D8CC → 217DA24 → D32E5A`。2026-10-14 的字段复核更正早期来源简写：slot2 从 `ThreadTaskManagerImpl+0x30` 取 pointee，再复制该 pointee 自身 `+0x38/+0x40` pair 到新 holder 并安装；不是已证的 manager `+0x30` pair 首 qword。**该 pointee 的具体类别、TLS slot0 实际对象类别与函数体仍未闭合** `[unknown]`，详见 §13。
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
217D8CC (ThreadTaskManagerImpl slot 2)
  ├─ 104C1C8(217D40D 的 TLS context, manager this)       ; context TLS 槽
  ├─ 63EC4([manager+0x30], out)                          ; 复制该 pointee 的 +0x38/+0x40 pair
  └─ 217DA24(&holder_ptr, source_pair)                   ; 分配 0x10 pair holder
        └─ D32E5A(holder, source_pair)                   ; clone + strong incref，安装到 D32DAD TLS
  └─ manager+0x40 = holder                               ; manager 保存同一 holder pointer
```

证据：[phaseE/functions/217da24.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseE/functions/217da24.c)、[phaseF/functions/217d8cc.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseF/functions/217d8cc.c)。

**推断（[inferred]）**：每个 worker 线程启动时把一个从 `ThreadTaskManagerImpl+0x30` pointee 派生的 pair holder 装进该线程 TLS 槽；TLS 中对象与该 worker 的调度关系尚未闭合。若提交目标确实随调用线程而变，则“提交发生在哪条线程”会决定目标对象——这是 Caligo 合同必须显式约束的点；在 slot0 实际目标类/函数体未闭合前，不得据此宣布任何线程合法。

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
- **`0x1B4E4EC` 定性**（phaseH 反编译，真函数起点 `0x1B4E4EC..0x1B4E619`）：锁 transport `+0x60` 的 pair 取**内联连接对象**（空 → `*out_token=0` 且返回 0 —— **未连接即同步失败，不排队**）；否则取 token（`0xB78D06`）、复制 command（经自身 slot3）、包装 callback（`0xB7F6F6`），转交 **`0xB7CE8A`**。
- **`0xB7CE8A` 定性**：校验内联对象 vtable `+0x30` 状态（`(state & 0xFFFFFFFE) != 4` → 失败分支）；组装闭包 {command 副本, request 接管, callback 包装, token}，经 **`D32138` 提交到 `0xB816E8` 全局对象的 `+8` 成员**（静态存储 RVA **`0x67510A8`**，guard `0x67510B8`；getter 的 init 桩 `0xB81748` 仅清零，真实构造经 `0x4E91A6` store 路径）。
- transport 对象出处：`0xB7C6C0`（0x70 字节分配 → ctor → `0xB7C89C` 挂释放钩子 `0xB7827E` → 存入 owner `+0x110`）；`0xB7C6C0` 本身经 manager 类 vtable（.rdata `0x3FD3E70` 槽）间接派发。**最后把该指针写入 MSFService `+0x60` 的那条 store 仍未单独定位**——但对合同不再是必要项：对象身份、slot 语义、状态检查、失败路径均已闭合。

### 10.2 dispatcher 类：两类提交目标同源 [live+static 交叉闭合]

- **TLS 提交目标**（index=44，D32DAD 持有）：145–150 线程中 **41 个已安装**，全部指向**同一 vtable RVA `0x43B6088`**；slot0 = **`0x31F8CEE`**（真函数起点）。
- **发送链全局 dispatcher**（静态 shared pair 位于 `0x67510A0`，对象 qword 是 `+8 = 0x67510A8`）：**同一 vtable `0x43B6088`**，slot0 同为 `0x31F8CEE`。发送路径经全局 singleton；接收路径代码读取调用线程 TLS 中的同类 dispatcher。T1–T5 五次现场报告里，全局对象地址都与一条 TLS dispatcher 项完全相同，因此本 profile 中不能再断言两条路径必为不同实例。该 TLS 项是否就是回调 consumer 仍未知。
- 类合同（phaseH/I 反编译）：ctor `0x31F878C`（写 vtable，`+8` 存 {内部队列对象, 弱 pair}，弱计数获取）；slot0 `0x31F8CEE`（锁 `+8` pair → 转交 `0x31F88EA`）；`0x31F88EA` = 入队 + **owner TID 自检**（当前 TID vs 对象 `+0x58`，跨线程走显式唤醒路径 `0x217DE40`）。
- **执行器单例 `0x750088` 现场未构造**（三次采样 vptr=None）——R2 §5 的 Core executor 单例不在本实例运行路径上；41 个 TLS dispatcher 与它无关（§5 的"关系 unknown"就此关闭：无关系）。
- **MSFCoreService getter `0x74203C` 未被调用**（`+0x674D3E8` 槽为空）——运行时 switch 实际选择了 MSFService。首个运行时分支证据。

### 10.3 发送/接收静态拓扑与现场边界

```
发送: 732A80 ─ this+0x60 (transport vtbl 0x41403B8)
        └ slot7 0x1B4E4EC ─ 锁 +0x60 pair(内联连接; 空即失败)
            └ 0xB7CE8A ─ 状态检查(vtbl+0x30) ─ D32138 → 全局 dispatcher(0x67510A8, vtbl 0x43B6088)
                └ slot0 0x31F8CEE ─ 0x31F88EA 入队(+8 pair) + 跨线程唤醒
接收: 1B41AE6 ─ 1B3F740 ─ D32D98(本线程 TLS, index 44) ─ 同类 dispatcher(0x43B6088)
                └ slot0 0x31F8CEE ─ 同上入队
回调: 静态调用路径为 dispatcher consumer → 73BD6C → 用户 invoker；现场尚无 native
      callback invocation，具体 consumer TID、该 TLS 项角色与恰一次性均未确认。
```

T5 的精确对象地址同时出现在一条 TLS dispatcher 项中；worker collection 的归属需按其指针-backed vector 布局另行验证，不能从类身份或 TLS 命中推出。

### 10.4 对 Caligo 合同的修订约束（取代 §8 中相应条款）

1. [原 §8.1] 静态发送路径提交到全局 singleton；接收路径读取当前线程 TLS 中的同类 dispatcher。现场 T1–T5 显示全局对象也出现在一条 TLS 项里，因此对象是否分离及该项用途仍未知。自有 adapter 在 QQ 拥有的线程上执行时，两条代码路径可核对；worker 上下文之外的调用行为仍未证。
2. [新] transport slot7 在**未连接时同步返回 0 且 token=0**——这是可依赖的失败信号（原生层可探测"会话未就绪"而无需发送）。
3. [新] 状态检查点：内联连接对象 vtable `+0x30` 的状态值（`4/5` 为可提交态）可作为原生 ready 证据的一部分（仍需与 session-ready 门联合判定）。
4. [新] 静态 callback 路径从 dispatcher consumer 执行用户 invoker，可与提交者并发；本轮没有现场 callback invocation，具体消费线程仍未知。
5. 仍未闭合：dispatcher 消费侧（出队→执行的线程池关系）；MSFService+0x60 的最后一条 store；stop/cancel/drain 路径；b78d06 token 的语义（回执关联候选）。

### 10.5 R3.1 证据索引

- 观测报告：`docs/execution/2026-10-08-native-contract-recovery/evidence/p1-observe-field-47524-t{1,2,3}.json`（工具 `crates/caligo-cli/src/observe_msf.rs`，提交 6f85350+）。
- 静态导出：phaseH（30 函数：1b4e4ec/b7ce8a/b7c6c0/31f8cee/31f878c 等）、phaseI（b816e8/31f88ea 等）、phaseJ（b81748）；`E:/stella/_reference/qq-native-r3-p1-20261008/`。
- 字节级：transport/dispatcher vtable rip-scan（构造/析构唯一性）、`.pdata` 函数起点验证、`b7c6c0` 的 .rdata vtable 归属。

## 11. 2026-10-13 同一实例的外部只读 dispatcher 复采

用户授权后，对其指定的最新独立测试 QQ 实例做四次 `caligo-cli observe-msf --pid <local-only> --out <local-evidence>`。目标进程的父/创建时间、窗口句柄和 `wrapper.node` 已加载路径在首次采样前复核；第四次采样前再次核验 PID、进程创建时间、加载模块路径和磁盘 hash。四次均绑定同一代次，磁盘 SHA-256 与字节数匹配本 profile 冻结值。窗口最小化，未激活、未打开聊天或读取聊天正文。observer 源码合同只申请 `QUERY_INFORMATION | VM_READ`，所有输出写本机 local-evidence；没有注入、写入目标进程或调用 QQ 函数。

四次采样间隔为 219 秒、183 秒、1274.1 秒，均有以下结果：

| 字段 | 观测 | 边界 |
|---|---|---|
| MSFService | vptr 锚点命中；MSFCoreService 未构造 | 当前对象形状与既有 profile 相符，不证明业务 send ready |
| dispatcher | global vptr `0x43B6088`；TLS index 有效 | 与静态 R3.1 vtable 身份一致 |
| 线程/TLS | 每次观察 145 条目标线程、41 个已安装 TLS dispatcher，vptr 单一；global dispatcher 的对象地址四次都与一条线程的 TLS dispatcher 项完全相同 | 证明该对象在采样时安装于一个线程 TLS；不能据此认定该线程执行 callback 或代表 worker 集合成员 |
| manager | 每次遍历到 2 个链节点 | 不证明 manager table / op ABI / stop 行为 |
| global dispatcher `+0x58` | 四次读到相同 32 位位型；均不匹配目标进程线程。T4 将 observer 读取改为 `u32` 后，又与 8,874 个可访问系统线程 ID 比对，仍未匹配 | T1–T3 JSON 曾按 `i32` 显示负数；按原始 DWORD 位型重解释后与 T4 相同。字段当前 owner 语义及有效线程均未证，不能按 owner 已确认准入调用 |
| Core executor 单例 | 锚点未命中/未构造 | 与 profile `not_in_play` 记录一致 |

T4 采样前将 `GlobalDispatcherReport.field_58_tid` 从 `Option<i32>` 改为 `Option<u32>`，以保存 Windows DWORD 原值；`cargo build -p caligo-cli --locked --offline --target-dir target-harness` 通过。T1–T3 的原始 JSON 未改；本机脱敏汇总按同一 32 位位型对齐四次数据。四次都未发现 `field_58_tid` 与目标线程匹配；T4 的系统线程比对限定于当时可访问进程共 8,874 个线程 ID，不等同于完整 OS 内核视图。

没有 callback 进入、slot enqueue/drain 或 terminal ledger 事件，因为没有 native invocation。四次复采不能关闭 dispatcher stop/cancel/drain/quiescence，也不能确认 callback consumer TID；`+0x58` 的现场值与静态 writer 语义之间仍有矛盾，不能猜字段已失效或自动替代静态合同。原始报告、CLI 原始日志、目标标识与完整线程 ID 只保存在 `local-evidence/n4-r2-field-20261013-224510/`，不入库。R2 及 R3 identity 门仍 open；不构成 R4 发信准入。

针对该值的静态交叉检查找到一个 writer：`0x31F90B0` 调用 `0x7879D0` 取得当前 TID，并写入 `arg+0x58`。它的已导出调用者 `0x217D8CC` 在初始化 worker context 时循环 `worker+0x48` 集合（计数来自 `worker+0x2C`），对 `0x31F83DC` 返回的每个对象调用该 setter。`0x31F878C` 构造函数只写 dispatcher vptr 和 `+0x08/+0x10` pair；`0x31F88EA` 读取 `this+0x58` 并与当前 TID 比较，本身不写。现场发现 global dispatcher 对象与某线程 TLS dispatcher 项相同，证明对象进入过/处于 TLS 安装路径；尚未证明它是否属于 `worker+0x48` 集合，也未找到线程退出时清零/迁移该字段的路径。导出有限，不能推断全 binary 其它 writer 不存在。下一项仍是证明集合成员身份、查退出清理语义与 op=1/op=2 完整合同。

复核原有 PID 47524 的三份 P1 报告时，global dispatcher `+0x58` 在 t1/t2 为零，t3 为非零但也没有匹配报告中的线程 TID。该历史样本说明这个字段的可见值会变化；当时的报告没有闭合其 owner 生命周期。本轮三次同值非零和线程清单不匹配与之并列保留，不将两组进程样本拼成一条生命周期证明。

## 12. 2026-10-14 R2 global dispatcher owner 只读复核

同一指定 QQ 进程完成第五次 `observe-msf` 采样 T5；PID、创建时间、加载路径与 profile hash 在采样/复核期间匹配。SnowLuma 保持原连接状态。没有激活 QQ 窗口、打开聊天、注入、写目标进程、调用 QQ 函数或发送消息。

本节 RTTI/vtable 结果将此前反汇编注释中的 `worker+offset` 暂定变量名收敛为 `ThreadTaskManagerImpl this+offset`；相同地址与字段偏移不变，但不能据旧变量名推断另有一个 worker owner 对象。

- T5 再次得到 145 条目标线程、41 个同 vptr TLS 项；全局 dispatcher 对象在一条 TLS 项中精确命中。T1–T5 的 `+0x58` DWORD 位型一致，均不属于各次 145 条目标线程；T4 当时对 8,874 个可访问系统线程 ID 的枚举也无匹配。
- 固定 hash 的静态 import 表确认 `0x7879D0` 经 IAT 调用 `KERNEL32!GetCurrentThreadId`。函数起点 `.pdata` scan 覆盖 525,291 个函数范围、18,889,360 条指令；`ThreadTaskManagerImpl::0x217D8CC` 使用 `this+0x2C` 计数，调用 `0x31F83DC` 从 `this+0x48` 的 pointer-backed task vector 取 `{object, control}` 16-byte pair，再调用 `0x31F90B0` 写当前 TID。发送侧 `0xB7CE8A` 调用 singleton getter `0xB816E8`，使用静态 pair 的 `+8/+0x10` 并增加 control 强引用，然后交给 `D32138`。
- 曾按内联数组布局找到一条表面候选，随后检查 `0x31F83DC` 发现 manager `+0x48` 实际经首 qword 指向独立元素数组；内联候选已作废。按正确 stride 搜索全部可读提交映射后，未找到同时满足固定 manager 双 vptr、vector 元素地址、全局对象与相同 control block 的候选。扫描覆盖 2,274 个 region、每轮约 901 MB，出现 3 次局部读取不足；因此 global dispatcher 是否属于 manager task vector 仍 unknown，有限可读扫描不作为全内存否定证据。
- 静态构造后的 `0x217D58C` 按 `this+0x2C` 配置数构造任务，并通过 `0x31F8378` 放入 `this+0x48` vector；`0x217D8CC` 随后遍历同一 vector 并设置各 dispatcher 的 `+0x58` TID，析构 `0x217D4FA` 则经 `0x31F8372` 清理该 vector。由此确认 `+0x48` 是 `ThreadTaskManagerImpl` 的 dispatcher task vector；T5 仍未证实 global dispatcher 是其中成员。
- 该现场结果收窄为：全局对象在一个 TLS 项中；`+0x58` 是真实 Windows TID，五次采样都没有当前活动线程匹配；全局对象是否属于 `ThreadTaskManagerImpl+0x48` task vector、TLS 项是否承担 callback consumer、线程退出/迁移及 stop/cancel/drain/quiescence 仍未闭合。没有 native invocation，因此没有 callback-enter/enqueue/drain 证据。
- 原始 T5 observer、只读扫描和地址映射验证均保存在本机 `local-evidence/n4-r2-field-20261014-000619/`；脱敏计数见 `summary-t5-redacted.json`，新增 RTTI/manager caller 静态摘要为 `summary-r2-manager-static-redacted.json`。之前四次原始报告及现场范围记录留在 `local-evidence/n4-r2-field-20261013-224510/`。

R2 owner/consumer/lifetime 与历史 `0xC0000005` 根因仍开放；R3 stable identity 与 sender UID→UIN 仍未闭合。R4 的 R1–R3 准入门未满足，本次没有执行原生发送。产品状态继续 NOT READY。

- 固定 hash 的 `.rdata` 虚表在文件偏移 `0x425EC48`（RVA `0x4260448`）含 `0x217DD98` 函数项；前置 Complete Object Locator 的 TypeDescriptor 名为 `.?AVThreadTaskManagerImpl@internal@xpng@@`。该类析构路径为 `0x217DD98 → 0x217D4FA → 0x217D556 → 0x31F8372 → 0x14830`；末端遍历 shared-pair vector、逐项调用 `0x1DB2`，再以 16-byte 元素数调用 `0x1FD0` 释放缓冲区。这确认 manager 类存在该清理实现，但没有证明谁销毁对象、析构如何触发、线程退出关系、停机等待或异步 callback quiescence。
- `.pdata` 全函数范围 xref 进一步确认 `0x217D242` 写入该双 vptr，是 `ThreadTaskManagerImpl` 构造函数；唯一直接构造调用点为 `0x217D16C`。后者分配 `0xF0` 字节对象、调用构造函数，再经 `0x217D35E` 建立 shared holder。`0xD31686` 调用该 factory，将 holder 转入自身 `+0x110`，再调 `0x217D58C`；其多个直接调用者包括全局发送初始化邻近路径 `0xB81758`。这些路径把 manager 实例与 holder/外层对象字段联系起来，但没有定位其销毁者，也未证明退出时执行析构或 callback drain。

## 13. 2026-10-14 TLS pair 来源与 manager 字段边界更正（固定样本静态）

固定磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本节修正 §1.3 与 §3.2 的早期来源简写，细节及可复核地址导出见[寿命合同 §7.18](qq-native-lifetime-contract.md#718-threadtaskmanager-tls-pair-来源与字段边界更正-2026-10-14-固定样本静态)。没有访问 QQ/SnowLuma、调用 native 或发送消息。

- `ThreadTaskManagerImpl` 构造函数 `0x217D242` 将 `+0x30/+0x38` 作为 16-byte pair 清零，将 `+0x40` 单独清零；析构 `0x217D4FA` 分别对 `+0x40` 执行 holder 清理、对 `+0x30` 调 `0x1DB2` 释放 pair。
- `0x217D8CC` 的 TLS 安装源并非已证的 manager `+0x30` pair 首 qword。该函数加载 `[manager+0x30]` 的 pointee，`0x63EC4` 从 pointee `+0x38/+0x40` 复制 pair；`0x217DA24` 分配 16-byte holder，`0xD32E5A` clone pair 并安装到 TLS，同时将同一 holder pointer 写入 manager `+0x40`。该 pointee 的 RTTI/业务类别仍未知。
- `0x217DA96` 也从 `[manager+0x30]` 取 pointee并投影其 `+0x38/+0x40` pair。与此不同，`0x31F88EA` 的 TID 不相等路径把 manager 本身传给 `0x63EC4`，机械读取 manager `+0x38/+0x40`。这些偏移分别落在 manager `+0x30` pair 的控制块 qword与独立 `+0x40` holder pointer；把它们称作已识别的 manager-owned pair 已被撤回。该 projection 结果和 `D32138` 目标仍 unknown，不能由 slot3 路径外推。
- 这一静态更正不改变 T1–T5 的只读结果，也不确定 TLS 中对象的真实类别、当前 owner、线程退出迁移、callback consumer 或 quiescence；未对 QQ 进行复采。本项不是 AV 根因结论，R4 继续受 R1–R3 门控。

## 14. 2026-10-14 ThreadTaskManager `+0x30` 条件 writer（固定样本静态）

固定-hash `wrapper.node` 的 `0x217D58C` 在 `r15b == byte[this+0x2D]` 分支把 `0x217D6D8` 生成的 pair 交给 `0x106E6`，写入 `this+0x30/+0x38`；`0x106E6` 增加强引用、复制 source pair 并释放旧 pair。配置写入追踪确认两个具体 setup 路径分别给出 bound/selector `{3,1}` 与 `{1,0}`，且都在 loop 中命中 selector；生成 pair 的 shared control block RTTI 指向 `TaskQueue`。详见[寿命合同 §7.19–§7.20](qq-native-lifetime-contract.md#719-threadtaskmanager-0x30-的条件写入与源-pair2026-10-14固定样本静态)。这两条静态路径不覆盖 30 个 `0xD31686` direct callsites 的全部输入，也没有映射到 T1–T5 的动态 dispatcher；slot2/slot3 全局调用顺序、manager owner/teardown 和 callback quiescence 仍 unknown。本静态结果不解释历史 AV，也不改变 R1–R3 准入门或 R4 状态。

## 15. 2026-10-14 ThreadTaskManager 初始化配置与 TaskQueue pair（固定样本静态）

`0x9C0C68 → 0xD31686` 的 constructor path 经 `0xD32508/0xD32512/0xD3251C` 将 `{0,0,0,0,3,1}` 传给 manager；另一直接调用点 `0xB81758 → 0xD31686` 传 `{0,0,0,0,1,0}`。这两种输入都确保 `0x217D58C` 条件写入 index 命中；线程创建 helper `0xD33418` 在该初始化返回后才调用。shared control-block RTTI 是 `std::__shared_ptr_emplace<TaskQueue,...>`。证据哈希与地址路径见[寿命合同 §7.20](qq-native-lifetime-contract.md#720-threadtaskmanager-配置字节taskqueue-rtti-与两个命中路径2026-10-14固定样本静态)。其它 direct callsites、现场实例对应关系、slot2/slot3 全局调用顺序、owner teardown、stop/drain/quiescence 及历史 AV 根因仍开放；R4 不准入，产品 NOT READY。

## 16. 2026-10-14 全部直接 `D31686` caller 的配置覆盖（固定样本静态）

后续固定样本枚举找到 30 个 rel32 `E8` 直接 callsite，并逐点核对传入的六字节配置：28 个使用 `0x4E6E5A` 默认初始化器，1 个（`0x01B63FF7`）直接以 `0xD324EC` 写同一默认尾字节，均得到 `{0,0,0,0,1,0}`；1 个（`0x009C0EE1`）覆盖尾字节为 `{3,1}`，得到 `{0,0,0,0,3,1}`。这 30 个静态 setup 输入分别令 `0x217D58C` 的 loop 选择 index 0 或 index 1，因而 writer 分支在各调用代码路径中可达；它不证明 30 个 setup 均在运行时执行、source pair 的动态有效性或具体现场 manager 的身份。完整 callsite 列表、逐点窗口和脚本哈希见[寿命合同 §7.21](qq-native-lifetime-contract.md#721-全部-30-个-d31686-直接调用点的-manager-配置来源2026-10-14固定样本静态)。manager owner / dispatcher membership / global slot2-slot3 调用次序 / teardown / stop-drain-quiescence / R3 identity / 历史 AV 均继续开放；R4 不准入，产品 NOT READY。

## 17. 2026-10-14 manager/TaskQueue backreference ownership（固定样本静态）

- 固定-hash RTTI 将 `0x217D35E` 构造的 manager 控制块识别为 `shared_ptr<ThreadTaskManagerImpl>`；构造路径把 manager strong pair 转为 `ThreadTaskManagerImpl+0x10` weak self-reference。初始化各 `TaskQueue` 时，manager `+0x10` 经 weak-lock shaped helper 获取临时 strong pair，再由 `0x31F8F5E` 将 weak pair 写入 `TaskQueue+0x48`。TaskQueue 自身的 shared control block RTTI 为 `TaskQueue`。详见[寿命合同 §7.22](qq-native-lifetime-contract.md#722-threadtaskmanager-与-taskqueue-间的弱引用边-2026-10-14固定样本静态)。
- `ThreadTaskManagerImpl+0x48` vector 项和命中 selector 后的 manager `+0x30/+0x38` 都持有 TaskQueue strong pair；`TaskQueue+0x48` 回指 manager 的边为 weak。因此仅能排除这一条指定强引用环，不能推出 manager 无其他 owner、最后 owner/析构已知、所有 dispatcher 已停止或 callback 已排空。R2 teardown/quiescence、R3 stable identity 与历史 AV 仍未闭合；R4 未准入。
- 脚本和报告 hash、counter 解释边界见寿命合同 §7.22。只做固定磁盘静态分析；未操作 QQ/SnowLuma、未调用 native、未发送消息。

## 18. 2026-10-14 TaskRunner manager 弱引用与候选强环复核（固定样本静态）

- `0x31F878C` 将 manager/control 指针写入 `TaskRunner+0x8/+0x10`，递增控制块 weak counter `+0x10`；相同布局的 strong counter 是 `+0x8`，`0x168CE` 是已核对的 weak-copy helper。故 TaskRunner 对 `ThreadTaskManagerImpl` 的这条回指为 weak pair。TaskRunner slot 0/3 从该字段经 `0x3F70` 取临时对象再检查 manager TID；细节、机器码范围与文件 hash 见[寿命合同 §7.23](qq-native-lifetime-contract.md#723-taskrunner-的-manager-弱引用与-nte-engine-parent-release2026-10-14固定样本静态)。
- manager vector strong 持有 `TaskQueue`，TaskQueue strong 持有其 `TaskRunner`，但 `TaskRunner→manager` 为 weak；这排除了该三边上的强引用环，不代表没有其他 manager owner，也不识别 runtime 实例或其析构时机。
- NTEngine 的候选析构路径会对其 `+0xE8` runner pair 调 slot 0，随后释放 `+0xE8` 和 child `+0xE0`；dtor 实际触发者、manager last owner、所有任务停止及 callback drain/quiescence 仍未知。仅固定磁盘静态分析，无 QQ/SnowLuma 操作、native invocation 或消息发送；R2/R3 未闭合，R4 不准入。
## 19. 2026-10-14 `MSFThread_xpng` manager-holder owner path (fixed-sample static)

The fixed-hash lifetime evidence in [§7.28](qq-native-lifetime-contract.md#728-msfthread_xpng-owner-path-to-threadtaskmanagerimpl-2026-10-14-fixed-sample-static) identifies an additional path: `MSFThread_xpng` vtable slot 2 calls `D31686` for its inner payload at `+0x8`, where the manager holder is installed at payload `+0x110`; the wrapper's deleting destructor calls `D31356` on that payload. Three static construction call sites lead to the slot-2 call. The payload constructor target `0x180D30FF0`, final strong owner, actual destructor trigger, successful wait, full task stop, and dispatcher/callback quiescence remain unknown. This is disk-only static evidence and does not admit R4.

## 20. 2026-10-14 ThreadPoolImpl manager setup and conditional stop/release path (fixed-sample static)

The fixed-hash RTTI vtable maps `ThreadPoolImpl@internal@xpng` slot 2 to `0x290C746`, which stores a pair-shaped value at `this+0xA0` and invokes `D31686` on its first qword. Its slot-0 teardown route reaches `0x290C612`; only when its state check succeeds does it call `D313D6([this+0xA0])` and reset the pair. The helper clears the two field qwords before releasing its copied 16-byte value. This closes one class-specific static setup-to-stop/release edge, not the manager's final strong owner or global quiescence. The D313D6 wait result, actual destructor execution, callback consumer/thread, R3 identity, and historical AV remain unresolved; see lifetime contract §7.29 and its hash-pinned local artifact.

## 21. 2026-10-14 FileDragManagerWin child stop and holder release (fixed-sample static)

The `FileDragManagerWin@nt` RTTI vtable slot 2 `0x9C4852` installs a child pointer at `this+0xA8` and calls `D31686` on it. The slot-0 teardown chain reaches `0x9C47DE → 0x4E6872 → 0x4E91A6`; for a non-null old child, `0x4E91BE` invokes `D31356` before freeing the child, which reaches the established `D313D6` stop path and releases child `+0x110`. Actual object lifetime, manager last-owner status, successful wait, full dispatcher quiescence, and callback consumer ABI/thread remain unknown; see lifetime contract §7.30 and its hash-pinned artifact.

## 22. 2026-10-14 four manager fields with matching setup and stop/update paths (fixed-sample static)

Four setup methods populate `this+0x1D0`, `+0x188`, `+0x1B8`, and `+0x1A0` and each calls `D31686` on the corresponding field's first qword. A separate method conditionally calls `D313D6` on those same four offsets. Its owner type and runtime trigger are not identified; this is not proof of a destructor, final manager owner, successful wait, complete stop, or global callback quiescence. The full bounded record and artifact hashes are in lifetime contract §7.31; R2/R3 remain open and R4 stays not admitted.

## 23. 2026-10-14 FileBridgeHostServiceImpl outer pair and four-slot stop/update call (fixed-sample static)

The fixed RTTI vtable identifies `0xA18CB4` as slot 4 of `FileBridgeHostServiceImpl@wifiphoto@nt`; the same vtable's slot 13 is `0xA19830`, another direct caller of `0x1852D42`. In slot 4, a non-null pair at `this+0xD8` supplies its first qword to `0x1852D42`, then the pair is reset/released through `0x504AEC`. This identifies an outer typed caller/member edge for the four-slot stop/update path, but not the pointee's dynamic type, the final manager owner, or a destructor/finalizer trigger. Wait success, all-task stop, callback drain/quiescence, consumer ABI/thread, stable identity, and the historical AV remain open. See lifetime contract §7.32 and its fixed-hash artifacts; R2/R3 remain incomplete and R4 is not admitted.
