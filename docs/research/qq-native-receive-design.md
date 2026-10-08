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

## 4. 实施顺序

1. 定位链插入方（注册函数）+ 锁行为 → 解除 §3.1。
2. `caligo_g2_listen_start/stop` 导出（g1.rs 模式）：注册/摘除 + JSONL 证据。
3. LAB 先行：假宿主按本设计模拟 OnRecv 循环，验证注册/回调/摘除对称（复用 P4 测试框架）。
4. 现场：注入 → 注册 → 用户发采样消息（10 私聊+10 群，唯一编号）→ 摘除 → captured fixture 回填 P5 → G2 判定。
