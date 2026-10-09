# 响应侧拒绝码消费者 — 范围与锚点(下一工作块起点)

日期:2026-10-09。状态:**范围已定,实现未开始**。
动机:群聊 @ 消息 r20–r28b 八轮静默拒(普通文本同桥 PASS,见
`g3-send-27624-r2x.jsonl`),无 retCode 无法继续定位;发送失败可观测性
是 @ 元素与一切后续功能(回复/转发)的调试前提。

## 已解契约(2026-10-09,反编译 r4-g3 phaseAE2)

### 注册侧 0x95d4e6(发送前由 25757ea 调用)
```
95d4e6(cookie*: [4]u32, spA: *shared_ptr, spB: *shared_ptr(record), cbCfg)
  若 A、B 均非空:
    cookieArr = {tag=2, cookie[0..16]}
    940588(&cookieArr, str@0x605d22) → 门控(bool)
    key1 = 58e72(97b0f(cookieArr, str@0x3faa680), str@0x4721cdf)
    c707b6(A, cbCfg, key1)                    # 注册消费者 A
    key2 = 58e72(97b0f(cookieArr, str@0x3faa680), str@0x4791e1d)
    94e64a(*B, elements)                      # record → 元素表
    c707b6(elements, cbCfg, key2)             # 注册元素表
```
- cookie = **客户端自配的 16B msgUid**(25757ea 从发送上下文 +0x20 取)。
- A = 消息对象(需支持 vtbl+0x100 序列化,c707b6 日志会序列化它);
  B = MsgRecord(record)。
- cbCfg = 9bef60 产物:对 record 形对象设特性键 0x17d5–0x17db、0x1a2d–0x1a30。

### 响应侧 0x73bd6c(响应到达分发)
- param_1 = 消费者/处理器对象:{+8 日志器, +0x18 inner, +0x30 消费者
  调用指针, +0x38 verbose 标志}。
- 错误码提取优先级:resp+0x40 → resp+0x60 → resp+100 → 传输层参数 → -1(超时)。
- 终局调用:`(param_1+0x30)(param_1+0x18, 0x10, errCode, errStr, msgUid串)`
  (门控 = 3fbab2(param_1+0x18))。

## 实现路径(建议)
1.  bridge 内构造假 MsgRecord(vtable 0x3f51a78 + CPBMessageOpti 条目数组,
    最小 props:0x9c41/0x9c4a/0x9c55)与假消息对象 A(vtbl+0x100 序列化)。
2.  以发送时用的 16B msgUid 为 cookie 走 95d4e6 注册——需先确认 940588
    门控(0x605d22 串 = 何种 cmd/类型校验)对本路径放行。
3.  cbCfg 回调写入 g3s jsonl(retCode + errStr)。
4.  发送请求体需带 msgUid 相关字段与 cookie 对齐(现 r18 形状无此字段;
    响应按 cookie 键控,请求/响应关联字段待确认)。

## 未决问题(实现期解)
- 940588 门控语义(str@0x605d22 内容)。
- key 组合器 97b0f/58e72 的精确串接格式。
- 请求 PB 中 msgUid 字段号(响应 cookie 关联)。
- 消费者对象 A 的完整 ABI(被 73bd6c 经 param_1+0x30 调用的实参时序)。

## 证据
- r4-g3 phaseAE2/functions/(95d4e6.c、73bd6c.c、9bef60.c、c707b6.c、940588.c)
- 25757ea.c(调用点上下文)
- @ 拒绝迭代:r20–r28b(g3-send-27624-r2*.jsonl;形状演进全记录)
