# ADR: 常驻发送的 wire sequence authority

- 日期：2026-10-11
- 状态：Accepted（仅指本机 core→resident 实现选择）
- 关联计划：[N4 响应恢复](../plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md) §6.4

## 背景

QQ 文本 wire 需要 `uint32` 序号。旧 bridge 在编码时使用常数 `0`，
而 `NativeSession` 里的 allocator 可以先于编码分配；两处各自拥有序号会导致
编码值、账本值与重启状态不一致。序号也不是 QQ message identity 或 OneBot
`message_id`。

## 决定

常驻生产候选链路只由 core 的账号 actor 分配序号：

1. `AccountActor::dispatch_begin` 从恢复后的高水位取下一个 `u32`。
2. core 将带 `wire_seq` 的 `DispatchIntent` 写入 journal，成功后才返回序号。
3. `CoreToBridgeMsg::Dispatch`、resident `OwnedRequest::SendText` 与 `HostOp::SendText`
   逐层携带同一个值；bridge 用它编码并登记，不能再分配一个替代值。
4. 恢复时从 journal 已用值的最大值继续；编码失败、拒绝或崩溃留下的空洞不回收。
   写 journal 失败、超出 `u32` 范围或序号传递不一致时拒绝 native 调用。
5. 旧 `g3_session` research 入口没有 core journal，现明确返回错误并拒绝发送。

独立的 `NativeSession` LAB 可以注入 `SeqSource` 验证顺序，但不得把该 allocator
接成 core 常驻路径的第二 authority。

## 后果与边界

- durable allocation 必须先于 encode / preregister / invoke；同一序号只覆盖本
  actor 的本地恢复合同，不推断 QQ 服务端要求跨账号或跨会话全局单调。
- 分配失败允许留下未使用的 DispatchIntent/序号空洞；恢复按 unknown 处理，不重发。
- `wire_seq` 仅是发送 wire 字段，不参与 MessageStore 身份主键，不可用于生成
  QQ native ID 或 OneBot ID。
- 当前离线测试证明落盘顺序、传值一致及重启不复用；没有证明新 QQ build 的
  server-side sequence scope 或 real-message identity 关联。

## 实现证据

- `crates/caligo-core/src/runtime.rs`: `dispatch_begin` 与 `transition_with_wire_seq`。
- `crates/caligo-core/src/daemon.rs`: durable dispatch 后将原序号写入 Dispatch DTO。
- `crates/caligo-model/src/ipc_v2.rs`: Dispatch wire contract。
- `crates/caligo-bridge/src/resident.rs`: 原序号贯通到 HostOp。
- `crates/caligo-bridge/src/native_session.rs`: allocate→encode→register→submit 顺序合同。
- `crates/caligo-core/tests/recovery_contract.rs`: `r3_wire_sequence_is_persisted_and_not_reused_after_restart`。
