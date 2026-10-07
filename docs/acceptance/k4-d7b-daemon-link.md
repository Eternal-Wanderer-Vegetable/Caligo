# K4-D7b 报告:常驻管道链接入与重连(k4-d7b-daemon-link)

> 执行:2026-10-07 18:30–22:00(Asia/Shanghai),五个指定测试实例生命周期。
> 全程遵守 test-scope 纪律;旧实例均由执行者正常退出;原始证据在本机
> `local-evidence/k4/d7-b/`(不入 Git)。LAB 侧:92 / 26 / 26 测试全绿、零警告。

## 1. 验收结果(D7 计划第 3 步:core 断开/恢复,bootstrap 不随连接增加)

| 判据 | 结果 | 证据 |
|---|---|---|
| bridge DLL 装载 + 候选 B 首入 | ✅ exit 0x0 × 3 个实例(r3/r4/r5) | 阶段 JSONL:env_fresh / loop_chain(isolate+loop 地址)/ owner_tid |
| **worker 于 QQ 进程内接入 daemon** | ✅ | 计数 `connects/hellos_accepted ≥1`;daemon 会话绑定(Health/SendText 受理) |
| Dispatch → worker → owner pump → 结果回传 | ✅ | R2:Queued → NativeStarted → **DeliveryUnknown**(诚实终态:D9 发送原语未接线,worker 如实回传 Unknown,不冒充成功) |
| **daemon 强杀重启 → worker 自动重连** | ✅ | `connects=2 / reconnects=5 / hellos_accepted=2`;R3 全链再次贯通 |
| **重连绝不重新 bootstrap** | ✅ | **`bootstraps_attempted=0`**(贯穿全部会话的计数探针) |
| 跨 daemon 重启不自动重发 | ✅ | R1(上实例的 NativeStarted 未决项)恢复后仅可查询,三次重启未重派发 |
| 心跳 | ✅ | 35 次心跳 0 超时(5s 间隔,3 次未响应断开) |
| 正常关闭 | ✅ | `qq-entry-stop` 0x0:worker 先停(DAEMON_STOP)→ owner 泵 `closed via owner pump` → daemon Stopped → **QQ 实例存活** |

## 2. 现场发现并修复的缺陷(全部有 LAB 回归)

1. **跨 shell 管道名反斜杠转换**(错误 123):daemon 管道名改短前缀自动补根。
2. **控制面 PID 校验误伤**:指定实例 PID 校验限定 bridge 角色;控制面凭 token。
3. **owner-init 位置**:resident 初始化在远程线程调用被 precheck 正确拒绝
   (防线有效)→ 改挂 owner pump 首次 tick。
4. **pump 相位上下文语义**:安静 QQ 轮转点 `Isolate::GetCurrent` 合法为 0
   (K2-03);§5.1 零 current 拒绝限定 **V8/Node 表面**;D7 pump 面仅 libuv,
   gate = `uv_loop_alive` + owner 线程 + env 新鲜;D8/D9 触碰 V8 的操作另加
   context 校验(适配器文档化)。
5. **worker 短名解析**:管道名自动补根(daemon/worker 同规则)。
6. **IO 取消部分读丢失**(LAB 猎捕):CancelIoEx 后必须交付已完成的部分读
   字节,否则帧流撕毁 —— 双侧修复。
7. owner-init 失败重试(每泵)、worker 待提交队列、实时计数发布。

## 3. 构建卫生教训

三次 0x4 的共同根因是**构建产物陈旧**(DLL 被在役实例锁定 → 源码修复未落盘)。
处置:被锁文件重命名挪开(Windows 允许)+ 构建顺序纪律(先构建后接入)。
被锁文件以 `_stale/_rN` 后缀保留在 target/(不属证据链)。

## 4. 当前状态与下一步

- 实例 2616:生命周期 Closed、QQ 存活,交还执行者正常使用/退出。
- caligod:已 Stopped(执行者侧可 `daemon-control --pipe caligo-k5-d7b-control --auth <token> stop`)。
- **D8 输入已备齐**:事件上行窗口(EventAck/重放/Gap)在 worker 内待命;
  监听器接线(LibraryAdd 真实实现 + RM 载荷 + owned 拷贝)是下一个现场阶段,
  用于闭合 B0 表剩余两项 UNKNOWN(引用持有、会话失效信号)与 G2 接收样本。
