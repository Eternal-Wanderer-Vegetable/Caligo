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

即：**弱计数归零 → 控制块自删**。强计数归零 → 销毁托管对象的调用点**不在析构体内**，而是发生在 pair 释放辅助（`1DB2`/`2632` 家族）内部 [inferred，辅助函数本体未逐一闭合]。

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
- `D32E5A` 是 pair 的**强引用复制**（copy + 源清理）；`3FAA`/`1DB2`/`2632` 是释放侧家族（出现频率极高，本帧内未逐一闭合其强/弱分支 [unknown，列为本合同下一增量]）。

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
2. 用 QQ 侧的释放家族（`1DB2`/`2632`，具体函数在 P4 ABI 冻结时逐一定证）释放；
3. 不以自有 allocator 释放 QQ 分配的控制块（`B75029` 是 QQ 的 free 包装）。

## 7. 已闭合 / 仍开放

**闭合**：控制块布局与 0x28 几何；强/弱两位点分离；五类获取位点；删除析构包装形态（`7378A4`/`74A156`/`9C2C6C`/`1F280` 同构）；析构体的 vptr 重置签名动作；弱归零→slot3 自删；LoginRequestImpl 弱成员与锁定式通知；TLS pair 强引用安装。

**开放（下一增量，均列计划未决问题）**：
1. `1DB2`/`2632`/`2632` 家族的强/弱分支与"强归零→销毁托管对象"的确切调用点（P4 冻结 ABI 时闭合）；
2. 控制块 vtable slot1（`59F40`）与 slot2 转型目标的运行时语义；
3. callback manager 的 `2338/236A` 空 manager/invoker 对与销毁路径（`7328EA` 计数递减）完整表（R2 §3 未竟部分）；
4. 观察者注册方（谁构造 LoginRequestImpl、以何观察者 pair）——session-ready 充分条件；
5. `this+0x60` 安装来源（thread contract §6 候选淘汰表 + 受控观测方案）。

## 8. 对实现的硬约束

1. Rust FFI 声明只能引用本文 `[verified]` 的几何与函数地址；`[inferred]` 项进入实现前必须先闭合。
2. 一切从 QQ 返回的 pair 按不可透传的 opaque 处理：禁止解引用控制块内部计数、禁止复制布局到 Rust 结构。
3. 释放必须走 QQ 侧函数；禁止跨 allocator。
4. callback 内复制的截止点维持 R2 结论：reason/body 只在回调调用期内有效，进入 Rust 前完成拥有复制。
5. 本文件全部结论基于静态固定样本；不构成任何现场准入门（G1–G4）的通过证据。
