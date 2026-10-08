# K4-D9 阶段报告:发送接线与剩余问题(k4-d9-send-wiring)

> 执行:2026-10-08 08:25–10:40(Asia/Shanghai),实例 48676/29876/1068/35156/30780
> 五个生命周期。构建:release + research(92/28/45 全绿零警告)。
> 本报告为**阶段报告**:发送接线完成并有架构级验证;G3 的 10+10 样本验收
> 因 worker↔daemon 会话重连稳定性问题**未完成**,列为下轮唯一焦点。

## 1. 已完成(代码 + 架构验证)

| 项 | 状态 |
|---|---|
| OwnedRequest::SendText / HostOp::SendText 全字段(id/chat/peer/text) | ✅ |
| qq_v8 D9 发送脚本(K3 sendMsg 链;fired → R.sendResults[mid];Promise 结果回收) | ✅ |
| QqOwnerAdapter::SendText → SendFired{mid}(非终态,§6.6)| ✅ |
| resident SendPending + pending_mid + complete_sends(mid 匹配 → Success{native_id}/Failure)| ✅ |
| worker: wire seq 单调 + EventAck 回显前移(ACK 风暴修复)| ✅ LAB 6/6 |
| IO 取消部分读交付(帧流撕毁修复)| ✅ LAB 6/6 |
| TryCatch 诊断(未捕获 JS 异常文本入日志)| ✅ 现场:抓到 `ReferenceError: JS is not defined` |

## 2. 现场发现并修复的缺陷(每项有 LAB/现场证据)

1. **JS 围栏标记泄漏**(尝试 3,TryCatch 抓到):内联脚本的 `JS`/`JS;` 围栏
   成为脚本内容首行 → 未捕获 ReferenceError → RunEmpty。修复:剥离围栏;
   四个 JS 产物过 Node 语法校验。
2. **Context 未进入**(尝试 3 探针):安静轮转点上 `1+1` 可执行但业务脚本
   Run 空 → V8 规范要求 Context::Enter → 阶梯补 Enter/Exit 严格配对。
3. **ACK 重放风暴**(尝试 3,LAB 6/6 复现后修复):抑制 ACK 回显 seq=0 →
   窗口永不前移 → worker 无限重放 → daemon 互斥饿死。修复:wire seq 回显。
4. **IO 取消部分读丢失**(LAB 猎捕):CancelIoEx 后部分读字节必须交付。
5. **钩子吞事件**(D8 收尾发现):take_events 在钩子内被丢弃 → 事件通道转发。

## 3. 未完成:worker↔daemon 会话重连稳定性

现场序列(实例 30780):bootstrap 0x0 → worker 接入(会话绑定)→ **桥接
管道中途断开** → daemon Degraded → worker 重连未成功恢复会话 → 控制面
send 返回 Degraded。G3 样本发送未开始(发送会真实到达对端,不在不稳定
会话上执行——§7-D9 纪律)。

定位方向(下轮首要):
- daemon accept_loop 在 bridge 断开后的再接受路径(disconnect→accept 次序);
- worker 重连退避与 daemon accept 窗口的时序配合;
- 会话重绑定(attach 幂等路径)在 Degraded→reconnect 链路的正确性;
- 诊断已齐备(listener/poll/连接失败全有变体级日志),下轮一轮现场即可
  收敛。

## 4. 实例处置

全部实例由执行者正常退出;35156 出现注入拒绝(error 5,首次分配即拒,
零副作用)——与 K3 记录的累积注入摩擦模式一致,佐证"每实例一轮生命周期"
纪律。现场 daemon 已 Stopped,journal 保留。

## 5. 下一步(优先级序)

1. worker↔daemon 会话重连稳定性(LAB 复现断连-重连时序 → 修复 → 现场);
2. 方向判据复核(真实 uin 已入 daemon 启动,待一条 SelfSent 样本);
3. G3 发送样本 10+10(串行节奏,§7-D9);
4. platform_time 字段名(K3 recv2 日志对照)。
