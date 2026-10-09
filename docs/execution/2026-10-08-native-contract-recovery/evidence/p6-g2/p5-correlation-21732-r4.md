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
