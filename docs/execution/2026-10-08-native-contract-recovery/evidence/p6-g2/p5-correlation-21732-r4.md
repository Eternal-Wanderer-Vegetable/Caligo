# P5 相关性验证(21732,r4 窗口,2026-10-09)

## 运行
- 用户以机器人账号 1694717255 → 主账号 3089665724 发送 P5TEST1/2/3。
- 窗口 captured=152(38 头)unreadable=0,restore 对称,QQ 存活。
  (注:--g2-run-ms=180000 被 g2_listen_run 的 120s 上限钳制,实际 120s。)

## 捕获定位( RPM-self 后验读池 )
- P5TEST1:消息 i=9 对象图深度 2(0x1f6fe1249c0),形态 `\x08\x07`+7 字节
  长度前缀 + "P5TEST1"(TLV/protobuf 风格)。
- P5TEST2:i=11/29 共享池(0x1f6fe1254f0);P5TEST3:i=11/27/29/33
  (含 0x341256f8400 群聊上下文池,有〈Ain〉/伊索: 昵称片段)。
- 私聊+群聊通知均到达同一通知树,监听器全部命中。

## 字段图进展(P5)
- 头 0x40 字节内**无发送者 uin**(bot uin 的 u32/u64/ascii 编码全不命中):
  发送者身份在对象图深层。
- 头形态两种:
  - Type S(标准):+0x00 self-uin SSO("3089665724",用户陈述证实=本机账号),
    +0x18 逐消息 32 位,+0x20=0x31/+0x28=0x25 恒定,+0x30/+0x38 池指针;
  - Type P(指针头):+0x00 指向元素/emoji 通知对象(含 emoji-recv 下载路径)。
- 池对象 = 推送批次共享字符串池:SEQ 链、昵称、正文(TLV 长度前缀)、
  群名、`trpc.msg.olpush.OlPushService.SsoPushAck` 方法名。

## 待办(P5 收口)
- 对象图系统测绘(通知 → 元素列表 → {sender,chat_type,body,seq,ts} 字段
  字典),可全离线(进程存活,池可读);必要时补一轮 0x200 头捕获。
- g2_listen_run 的 observe_ms 上限 120s 是否放开,随 P5 收口一并决定。

## 对象图测绘补充(同日续,只读 RPM)

### 通知对象图层级(实证)
- Type P 头 +0x00 → 消息记录/池对象(0x1f6fe0f4480 等,堆 0x1f6f 区)→
  元素 blob(P5TEST 文本宿主,0x1f6fe124xxx 区)。
- 池 = 批次共享字符串池,内容:消息 SEQ 链("3985043_3985042_...")、
  **成员表**(uin ASCII ↔ u_xxx uid ↔ 昵称,如 263402786/2167043552/
  1076624255/1033369266/2419020125/586058592/1124463446...)、
  群名("openPangu-2.0 交流群"、"巾帼商会"...)、正文 TLV({0x08,len,bytes})、
  emoji 下载路径、`trpc.msg.olpush.OlPushService.SsoPushAck`。
- self uid = `u_KxYzu_qQPbCC-Ca8mLteVA`(昵称 Vegetable,主账号 3089665724)。

### 已定字段
| 字段 | 位置 | 形态 |
|---|---|---|
| 本机账号 | 头 +0x00 (Type S) | libc++ SSO 字符串 |
| 正文 | 池内元素 TLV | {0x08, len, utf8 bytes} |
| 消息 SEQ | 池内 | ASCII 下划线链 |
| 成员映射 | 池内表 | uin↔uid↔昵称 |
| 推送方法 | 池内 | "trpc.msg.olpush....SsoPushAck" |

### 未定字段(下会话收口)
- 发送者 uid/uin 的**记录内位置**:bot uin(1694717255)不在字面表——
  NTQQ 记录内以 senderUid(u_xxx)引用,需解元素记录结构
  (P5TEST1 @池+0xc62,向前走 TLV 链到记录头)。
- 聊天类型/时间戳/消息 id 的偏移。
- 静态路线备选:74d7bf→74daab→builder(0x74db6d)寄存器接力已解到
  "pair 写入局部,第二调用以 {packet+0x18, packet, local+0x10, strings} 建对象"。

### 工程注记
- g2_listen_run 窗口上限 120s(observe_ms.min(120_000))钳制了 180s 请求。
- 深挖可全离线:21732 存活,池地址(0x1f6fe124xxx/0x1f6fe0f4480)仍可读。
