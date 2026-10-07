# K3-D 接收/订阅验收记录(计划 §8 格式)

| 字段 | 值 |
|---|---|
| 环境摘要 | qq-9.9.33-52230(aff854e8),Windows 11 26200,manifest 5/5 PASS |
| 账号别名 | FRIEND-B → TEST-ACCOUNT-A(私聊);群聊流量多群实时 |
| 预期 | addKernelMsgListener 实时捕获收到的消息(全字段) |
| 实际 | onRecvMsg 捕获 FRIEND-B 消息(msgId 7693735546892302421,txt 见 local-evidence);一轮 60+ 条群消息零丢包 |
| 判定 | **通过**(2026-10-07) |
| 通道 | 活会话(nt_1)MsgService.addKernelMsgListener;移除 removeKernelMsgListener(15/16 已移除) |
| 附带 | C2C 发送 CALIGO-K3-SEND-001 → FRIEND-B 已收到(执行者确认) |
| 隐私 | 消息原文仅 local-evidence;仓库只记结构 |
