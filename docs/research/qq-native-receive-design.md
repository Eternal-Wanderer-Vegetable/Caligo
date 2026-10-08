# G2 接收接线设计（push 监听器注册；qq-native-receive-design）

日期：2026-10-08。依据：R2 phase1 `functions/1b41ae6.c`（MSFService::OnRecv 主函数）、`45e5` 反汇编（链表 next）、capability profile 与 G1 现场证据。本文是 P6→G2 的实现设计基线；实现落地前每项标注 `[verified]`/`[待实现]`。

## 1. OnRecv 通知结构 `[verified]`（R2 phase1 反编译）

`1B41AE6(this, packet_holder, status)` 三条通知路径：

| 路径 | 监听器来源 | 回调 | 参数 |
|---|---|---|---|
| **push 消息**（G2 目标） | 侵入式链 `this+0x170`（哨兵 `this+0x178`） | 监听器 vtbl **slot6（+0x30）** | `(listener, msg_shared_pair)` |
| 请求完成 | 同一链 `this+0x170` | vtbl **slot5（+0x28）** | `(listener, request)` |
| 请求分派 | 1B3F740 内同链 | vtbl **slot9（+0x48）** | `(listener, request, ok)` |

另有一个独立单观察者 `this+0x140`（vtbl slot1，收到同一 msg_shared_pair）——**不触碰**。

链表机制 `[verified]`：节点 `+0x00` = next（`45e5` 反汇编：release 路径即 `[node]` 链；前段为 MSVC 迭代器调试校验）；节点 `+0x20` = 监听器对象指针。消息共享对由 `74D7BF→74DAAB` 构造（{ptr, ctrl}，回调前对 ctrl+8 强引用 +1，回调后调用方 `1DB2` 释放——**监听器只在调用期内持有借用**）。

msg_shared 对象构造输入 `[verified 形状, 字段语义待 G2 实证]`：`74D7BF(dst, packet, packet+0x18(seq), packet+0x20(command), packet+0x38(body), strings)` —— 与 R2 §1 的 packet 字段图一致。

## 2. 注册设计 `[待实现]`

1. 构造自有监听器对象：`[vptr → 我们的 vtable]`，vtable 至少覆盖 slot5/slot6/slot9；slot6 = Rust push 处理器（**边界内复制** msg 对象的必要字段后进入有界通道——R2 §1 借用纪律），slot5/slot9 = 无操作返回（请求完成通知，G2 不消费）。
2. 分配节点：`{next: 首节点, prev: &sentinel, listener: 我们的对象}`（+0x20 放指针；其余字段对齐 45e5 校验——**debug 校验只在 _ITERATOR_DEBUG_LEVEL>0 的 QQ 构建**，观察其是否启用，必要时补 prev 字段）。
3. 插入头部：`node.next = head.next; head.next = node;`（并发保护见 §3）。
4. 停止：从链中摘除自己的节点（前驱定位需遍历或保存 prev——插入在头部则 prev 恒为 sentinel）。

## 3. 风险与前置

1. **并发保护** `[未定证]`：注册/摘除与 QQ 遍历的锁（可能为服务内 mutex）未定位。实现前必须找到 +0x170 链的插入方（QQ 自己的监听器注册函数）并复核其加锁行为；找不到则该项阻塞。
2. **监听器 vtable 完整性**：QQ 是否在这些对象上调用其他槽（析构 RTTI 查询等）未证；vtable 需填充足够的合法槽位（观察 D8 时代 JS 监听器对象的槽使用可作旁证）。
3. **借用纪律**：slot6 回调参数是共享对借用；处理器必须同步复制、禁止跨回调持有（R2 §1；P5 的有界通道复用）。
4. **异常不越界**：处理器 panic 隔离（P4 callback_boundary 复用）。
5. 字段级验证：msg 对象的字段偏移在首个真实样本（G2 采样）落地时以 captured fixture 复核（P5 `FIELD_NUMBERS_VERIFIED=false` 同一纪律）。

## 2b. 优选路径修订（2026-10-08 晚：单观察者指针交换）

进一步审读 OnRecv 反编译（R2 phase1 `1b41ae6.c:113-123`）发现比链表插入**简单得多**的等价注册点：

```
plVar5 = *(this + 0x140);          // 单观察者指针（非链表）
if (plVar5 != 0) {
    msg_pair.ctrl->strong += 1;    // 共享对强引用 +1
    plVar5->vtbl[1] (+0x08)(plVar5, &msg_pair);   // slot1 通知
}
```

**方案（指针交换 + 转发）**：
1. 读 `*(this+0x140)` = 原观察者 O0（保存）；
2. 写入我们的对象 Ours（vtable slot1 = Rust 处理器）——x64 对齐指针写为原子；
3. Ours.slot1(obs, msg_pair)：**边界内复制**必要字段 → 有界通道 → 转发调用 `O0->vtbl[1](O0, msg_pair)`（QQ 行为完全保持）；
4. 停止：写回 O0。

相对链表插入的优势：零节点分配、无链锁问题、单原子指针写、转发链保真；风险与前置：
- `[未定证]` O0 的寿命（登出可能析构——停止必须在登出前/检测 O0 变化时恢复原指针）；
- `[未定证]` this+0x140 何时被赋值（登录会话建立时；G1 探针读到的 MSFService 上可直接观察该指针当前值）；
- slot1 是否为 O0 被调用的唯一槽（OnRecv 只见到 slot1；其余使用点扫描待做）；
- 处理器重入纪律同 §3（复制/panic 隔离/有界通道）。

链表插入方案（§2）降级为备选（若 +0x140 被证为非 push 路径专用）。

## 5. 运行时对象定性修正（2026-10-08 深夜,G2 机械验证的负结果）

G2 探针在 31208 上实测：**Service 单例的 +0x140/+0x170/+0x178 全为 0** —— 通知结构不在 Service 上。

根因 `[verified]`：OnRecv（`1B41AE6`）是 `.rdata 0x413F7A8` vtable 的 **slot4**,RTTI COL 解码类名为 **`msf::internal::Manager`**——接收通知结构（+0x110 transport 注册、+0x140 单观察者、+0x170 监听链）属于 **Manager 实例**,不是 MSFService 单例。证据自洽：G1 的 transport 安装者 `b7c6c0` 正是把 transport 写入 owner+0x110（Manager+0x110）。

**修订**：§2b 的指针交换与 §2 的链表注册,目标对象从"Service 单例"改为 **Manager 实例**（字段偏移不变:OnRecv 的 this 即 Manager）。下一会话第一步:定位 Manager 的运行时实例(构造器 rip-scan 0x413F7A8 → 存储位/getter;或经 transport 指针反查 owner),observe-msf 增加 Manager 字段读数,然后交换/注册落地。

机械验证留存(31208,g2 listen 首跑):adopt/anchor 全过——getter/租约/锚点链路复用成立;仅目标对象错了,修正后即接续。

## 6. Manager 运行时定位成功（2026-10-08 深夜续,实例 20016）

断电重启后新实例 **PID 20016**（创建 14:15:28Z,测试账号,锚点命中）。observe-msf 新增**堆扫 Manager vptr**（needle=base+0x413F7A8,MEM_PRIVATE 提交区逐区 RPM）:

- **Manager = 0x260232e95b8**（vptr RVA 验证 ✓）
- **+0x170 链非空**:head=0x260230eeee0（QQ 自己的监听器在链上——链表注册是正确路线,+0x140 单观察者路线废弃,该字段为 0）
- 节点方向:`45e5` 完整反汇编 = **`*(node+0x08)`**（MSVC list 节点双指针之一;精确 next/prev 方向与插入锁 = 下一步:dump 哨兵节点 0x260230eeee0 头 0x30 字节 + 首节点对照 OnRecv 迭代即可定论）

工具:`observe-msf` 已含 Manager 定位（JSON `manager` 字段:manager/o0/list_head）。G2 监听器实现(g2.rs)需从 +0x140 交换改为**链表注册**(§2 原方案)。

## 4. 实施顺序

1. recon 负结果记录（2026-10-08 晚）：+0x170 存储扫描 405 函数、MSF 区域 LEA/LOAD 扫描 24+6 函数——均为其他子系统的同名偏移字段（0x717F34/731B9A 等已逐一排除），链插入方未定位。
2. 【主路径】`caligo_g2_listen_start/stop`（g1.rs 模式）：+0x140 指针交换 + 转发 + 恢复；先加只读核验（当前 O0 非空、vptr 可读）。
3. LAB：假宿主模拟 OnRecv 通知（slot1 调用），验证交换/转发/恢复对称与复制纪律（P4 框架）。
4. 现场：注入 → 交换 → 用户发采样消息（10 私聊+10 群，唯一编号）→ 恢复 → captured fixture 回填 P5 → G2 判定。

## 哨兵 dump（2026-10-08 深夜续,决定性数据）

S=0x260230eeee0 = {+0x00 = 0x260232e9720(= Manager+0x168,内嵌节点 E), +0x08 = 0x260231a0ca0(堆节点1), +0x10 = 0x10}。OnRecv 从 S 起沿 +0x08 走(prev 方向)、终止于 Manager+0x178、listener=*(node+0x20)。下一步:dump 节点1(0x260231a0ca0)头 + 完整链走查定插入方向与锁;g2.rs 链表注册按本节数据重写(+0x140 交换实现保留弃用)。
