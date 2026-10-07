# K3-SEND-001 验收记录(计划 §8 格式)

| 字段 | 值 |
|---|---|
| 环境摘要 | qq-9.9.33-52230(aff854e8),Windows 11 26200,manifest 5/5 PASS |
| 账号别名 | TEST-ACCOUNT-A → GROUP-C |
| 预期 | 群内出现唯一正文 `CALIGO-K3-GROUP-001`,发送者 TEST-ACCOUNT-A |
| 实际 | 执行者目视确认消息出现;内核返回 `{result:0, errMsg:''}` |
| 判定 | **通过**(2026-10-07,执行者验收) |
| 通道 | 客户端内直调:`_linkedBinding('QQNT')` → 活会话 nt_1 → MsgService.sendMsg(NapCat 同款四参) |
| 副作用 | 消息一条;钩子全部移除;实例存活 |
| 隐私 | 无消息内容/凭据入库(local-evidence 全程 gitignored) |
