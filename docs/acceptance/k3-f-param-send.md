# K3-F 参数化发送验收记录(计划 §8 格式)

| 字段 | 值 |
|---|---|
| 环境摘要 | qq-9.9.33-52230(aff854e8),Windows 11 26200,manifest 5/5 PASS |
| 账号别名 | TEST-ACCOUNT-A → GROUP-C |
| 预期 | 群内出现 `CALIGO-K3-GROUP-002`(参数化通道) |
| 实际 | 执行者目视确认 ✓(两条,对应两轮独立发送) |
| 判定 | **通过**(2026-10-07) |
| 通道 | AsyncCtx 参数区(JSON)→ PARAM_SEND_TEMPLATE → 活会话 sendMsg(NapCat 四参) |
| 工程意义 | CLI `--send-peer/--send-chat/--send-text` 直达发送;生产层定型 |
| 隐私 | 群消息原文仅 local-evidence |
