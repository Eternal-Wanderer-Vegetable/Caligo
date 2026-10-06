# 测试范围(test-scope)

> K0 交付物。冻结于 2026-10-06(Asia/Shanghai)。
> 隐私纪律:真实 QQ 号码、群号、昵称只写入本机 `local-evidence/test-scope-local.md`(已被 .gitignore 排除,不入库、不推送)。本文件只用别名。

## 1. 冻结的版本与平台

| 项 | 值 |
|---|---|
| 平台 | Windows x64(本机 Windows 11 build 26200) |
| QQ 版本 | 9.9.33-52230(aff854e8) |
| 版本目录 | `D:\Program Files\Tencent\QQNT\versions\9.9.33-52230` |
| 模块摘要 | 见 `docs/research/environment.json`;适配判据见 `docs/contracts/version-adapter-manifest.json` |
| 摘要不匹配行为 | 拒绝 attach / 拒绝发送(计划 §5 约束 T01),不猜偏移 |

## 2. 测试身份(别名 → 实际值在 local-evidence)

| 别名 | 含义 | 实际值记录位置 |
|---|---|---|
| TEST-ACCOUNT-A | 本项目实验用测试账号(bridge/core 附着的账号) | `local-evidence/test-scope-local.md` §A(待执行者填写) |
| FRIEND-B | 接收私聊测试文本的好友(与 TEST-ACCOUNT-A 已建立好友关系) | 同上 §B(待填写) |
| GROUP-C | 测试群(FRIEND-B 为成员之一,可代发群消息) | 同上 §C(待填写) |
| MEMBER-D | 群内另一测试成员,用于产生"他人发送"的群聊接收样本 | 同上 §D(待填写) |

**状态:待执行者填写。** 在填写之前,K2 的真实收发实验不得开始;K1 的只读 probe 实验允许在执行者临时指定实例上进行,但每次必须在 `local-evidence/` 记录当次 PID 与选择理由。

## 3. 唯一正文约定

所有由本项目产生的测试发送都使用可区分的唯一正文前缀,便于在真实会话流中定位与事后审计:

| 阶段 | 正文格式 | 示例 |
|---|---|---|
| K1 probe 身份/失效观察 | `CALIGO-K1-PROBE-<NNN>` | CALIGO-K1-PROBE-001 |
| K2 私聊接收样本 | `CALIGO-K2-PRIVATE-<NNN>` | CALIGO-K2-PRIVATE-001 |
| K2 群聊接收样本 | `CALIGO-K2-GROUP-<NNN>` | CALIGO-K2-GROUP-001 |
| K3 发送实验 | `CALIGO-K3-SEND-<NNN>` | CALIGO-K3-SEND-001 |

首轮 K0/K1 不发送任何消息;K1 probe 设计为无副作用观察(不调用发送入口)。

## 4. 实例隔离与选择规则

1. 本机当前可同时存在多个 QQ 进程(2026-10-06 观察到 18 个进程、2 个主进程候选,见 environment.json)。**运行中的任何实例都不自动成为实验目标。**
2. 实验实例由执行者在实验当次明确指定(记 PID + 进程创建时间,两者一起核对),首选:专为实验新启动的 QQ 实例并登录 TEST-ACCOUNT-A。
3. 不自动停止、不修改其他 QQ 实例;不遍历修改其他 QQ 安装。
4. 已观察:当前进程模块列表未见第三方协议端模块(如 NapCat/LLOneBot 式 .node)。实验实例仍须在每次实验前复查此条件,并记录到当次 local-evidence。

## 5. 首轮明确不覆盖

临时会话、陌生人主动私聊、全部媒体类型、群管理操作、多账号并发、历史消息补回、QQ 以外平台。(与计划 §1.3 一致)

## 6. 验收关联

- 通过条件挂钩:K0 要求"目标账号/群/好友明确"——本文件 §2 填写完成后该条件才闭合。
- 每个 K2/K3 样本的验收记录字段(环境摘要、账号别名、预期/实际、判定)按计划 §8 执行,存放 `docs/acceptance/`(K3 起建立)。
