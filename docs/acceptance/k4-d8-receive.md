# K4-D8 验收报告:常驻接收与 G2 样本(k4-d8-receive)

> 执行:2026-10-08 07:30–08:20(Asia/Shanghai),实例 13388(08:02:13 启动,
> wrapper.node 唯一命中,登记于 local-evidence/k4/d8-r4/)。
> 构建:release + research,含 wire-seq ACK 协议修复(f56618c)与事件通道
> 修复(8e2b014)。门 1/门 2 全过;旧探针全程未用。

## 1. 现场结果

| 判据 | 结果 |
|---|---|
| bootstrap + daemon + worker 接入 | ✅ 0x0;connects=1,Hello 受理,心跳全程零超时 |
| **监听器 ARM(V8 阶梯真实执行)** | ✅ 无 owner_init_retry;消息实时流入 |
| **样本捕获** | ✅ 38 事件全数到达控制面 drain(23 条 K8 测试样本 + 15 条真实群聊流量) |
| **原生 ID 全异性** | ✅ 38/38 全异,零碰撞(L04:同正文不同 ID 均保留) |
| **DUP 对(同正文连发两条)** | ✅ 两条均保留,ID 各异(7694081896337046 / 7694081899911811) |
| **>600 字符长文本** | ✅ 1188 字符完整到达,零裁剪(JS 上限 8000) |
| 私聊/群聊区分 | ✅ 同数字 peer 场景未复现,但 kind 判据(chatType)对全部 38 条正确分流 |
| **对称移除** | ✅ `qq-entry-stop` → owner 泵 closed;关闭后实例存活 |
| 方向(SelfSent) | ⚠️ 见 §2 配置项 |
| platform_time | ⚠️ 见 §2 观察项 |

## 2. 如实记录(不降级隐藏)

1. **方向判据配置错误(执行者侧,我的调用)**:`--daemon-account` 传了计划
   占位值 `10001`,而真实 uin 是 `1694717255` → 手动自发样本被标为 incoming
   (sender=1694717255 可辨)。映射逻辑 `sender==account` 本身平凡正确;
   修正 = daemon 启动传真实账号(此值在 test-scope-local.md,不入 Git)。
   该项验收状态:**机制在,配置错,待下轮一条样本复核**。
2. **platform_time 全 None**:JS 侧读 `m.msgTime` 未命中(NT 消息对象的时间
   字段名不同)。计划允许"取不到 → None 不猜";字段名确认属 D8 后续
   离线工作(K3 recv2 旧日志中有时间字段样本可对照)。
3. 15 条非 K8 事件 = 真实群聊消息,全部正确透传(含 2 条空文本的非文字
   消息)——证明监听器对真实流量稳定;内容未入 Git(test-scope 隐私纪律)。

## 3. G2 判定

**G2 核心判据 = PASS(受限)**:
- 23 条 K8 样本(私聊 11 + 群 12)全数到达,身份/正文/来源正确;
- 同正文 DUP 对保留、>600 字符完整、真实流量并发无丢失;
- 受限项:方向配置错误(§2.1,待一条复核样本)、platform_time 字段名
  (§2.2,离线可修)。
- 计划 §7-D8 的"10+10 唯一正文"计数:实际私聊 001–010(+长文本)、
  群 001–010(+DUP×2)已覆盖;`GROUP-0010` 为执行者手误编号,不影响判定。

## 4. 关闭证据

- `qq-entry-stop` exit 0x0;日志 `closed via owner pump`(tid 50856);
- daemon Stopped;实例 13388 关闭后存活;
- 计数器全程:bootstraps_attempted=0,protocol_errors=0,heartbeat_timeouts=0,
  events_gap_dropped=0。

## 5. 下一步

1. 方向配置修正 + 一条 SelfSent 复核样本(下轮实例,5 分钟);
2. platform_time 字段名离线确认(js 侧加字段探测);
3. 之后进入 **D9(参数化发送与 G3)**——worker 的 SendText 原语在适配器
   已有挂点(当前显式拒绝),按 K3 send 脚本接线后走同链路验收。
