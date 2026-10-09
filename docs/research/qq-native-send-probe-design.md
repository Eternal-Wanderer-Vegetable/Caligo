# 原生发送链路探测设计(qq-native-send-probe-design)

> 日期:2026-10-09。状态:P1 设计定稿,P2 待 P1 现场证据。
> 上游:thread-contract §10(发送拓扑闭合)、k4-native-route-decision(G3 解封)、
> receive-design(G2 已 PASS,提交 5dd6f38)。
> 标签:`[verified]` 本机已核对/现场实证;`[inferred]` 证据形成的工程判断;
> `[assumed]` 必须实测验证。

## 1. 静态链路解码(2026-10-09,r4-g3 证据树)

证据树:`E:/stella/_reference/qq-native-r4-g3-20261009/`(phaseA–D,Ghidra 定向
反编译 137 函数,目标 SHA256 与 R2 同源 `63112ab9…`)。全部为静态只读解码。

### 1.1 发送入口与完整调用链 [verified]

```
业务层  BaseMsgMgr::SendMsg(0x25757ea, base_msg_worker.cc, 日志"[send msg]")
        ├─ cmd = "MessageSvc.PbSendMsg"(0x429a870;chat_type==4 → "MsgProxy.SendMsg" 0x429a890)
        ├─ 超时/重试策略 = {180000, 1, 180000, 1, 2, 1}(0x18 字节×2 组)
        ├─ blob(shared_ptr<T>,T 待 P1 实证)= 对象 vtbl+0x100(&out, 0) 产出
        └─ 0x2578e18(路由串, cmd, blob, policy, messenger_sp)
              └─ 全局注册表按键查找(2b8674/2b86d4)→ route_obj
                    └─ route vtbl+0xC8 转发器(0x4c7aa6:mov rax,[rcx];mov rax,[rax+0xC8];jmp)
                          ↓(内部最终到达)
MSF 层  MSFService::SendSSORequestInternal(0x732A80, msf_service.cc)
        两个成员适配器:0x7329f6(传 bool=1 + 回调)/ 0x7333ac(传 bool=0,无回调)
        ├─ 断言 *(this+0x60) != 0("messenger_ is nullptr")
        ├─ 任务对象 0x88 字节(0x72ca0d alloc + 0x72daf4 ctor):
        │   +0x18=bool(SendSSORequest 参数4);+0x38=trace 串(this+0x270 非空时);
        │   内嵌:cmd 属性包(shared_ptr,0x78ab00 构造 0x60B:{标志, 3×string},
        │     tag4="QQForNT-PC", tag6=cmd, tag9="formal"/"test" 按 this+0x270)
        │   内嵌:request shared_ptr pair(参数3 {ptr,ctrl} 移转)
        └─ transport vtable slot7(+0x38)= 0x1B4E4EC(732EB9 调用点):
            slot7(transport, string& uin /*this+0x1f0 副本*/,
                  unique_ptr<task>* /*移转*/, policy48&, i32* token_out, callback&)
            · 未连接:同步 *token_out=0 且返回 0(不排队)——失败信号 [verified §10]
            · 已连接:token=b78d06(int);cmd 非空时 transport slot3 补属性;
              callback 经 b7f6f6 包装(→ 空函数桩 0x72d596/0x72d5c8:
              **MSF 层回调有意置空,真实结果路由不在此层**)
            └─ 0xB7CE8A:conn 状态检查(conn vtbl+0x30,值 4/5 = 可提交态);
                组装闭包 0x88 字节 {conn_pair@0x00, cmd_string@0x10,
                task_ptr@0x20, ?@0x28, policy@0x30, token@0x60, callback@0x68}
                → D32138(dispatcher=*(global+8) @静态 0x67510A8, 闭包, 桩)
                    └─ dispatcher slot0 0x31F8CEE → 0x31F88EA 入队;
                       跨线程提交 → 二次派发到 owner 线程 dispatcher(63EC4)
                       + 显式唤醒 0x217DE40 —— **对"我们线程提交"结构性支持**
                       [inferred:现场准入待 G3-P2 验证]
消费侧  dispatcher 消费线程(QQ worker 池)执行闭包 → conn->send(task…)
        响应:73bd6c(token, status, 响应对象)→ 按 cmd/token 分发到
        BaseMsgMgr 注册的消费者(95d4e6 "register before send")
```

### 1.2 对 G3 探测有约束力的事实 [verified]

| # | 事实 | 证据 |
|---|---|---|
| F1 | 发送命令串:`MessageSvc.PbSendMsg`(私聊/群常规)/ `MsgProxy.SendMsg`(chat_type 4) | r4-g3/phaseA/25757ea.c + 字符串 RVA |
| F2 | transport vtable RVA `0x41403B8`,slot7(+0x38)=`0x1B4E4EC`,现场三次采样同值 | thread-contract §10.1;phaseH/1b4e4ec.c |
| F3 | slot7 六参形状(uin string、task 移转指针、policy48、i32 token_out、callback) | phaseH/1b4e4ec.c + phaseA/732a80.c 交叉 |
| F4 | conn 状态检查 = conn vtbl+0x30 值 `(x&0xFFFFFFFE)==4` 为可提交 | phaseH/b7ce8a.c |
| F5 | policy48 = {180000,1,180000,1,2,1}(消息发送实测值;b7d248 为逐字段拷贝) | phaseA/25757ea.c + phaseB/b7d248.c |
| F6 | 任务对象 0x88B、闭包 0x88B、MSF 回调被置空(0x72d596 桩) | phaseB/b7f7b2.c + 反汇编 72d596 |
| F7 | request blob 类型 = shared_ptr<T>,T 未知(在 task 内、由 BaseMsgMgr vtbl+0x100 产出) | phaseA/25757ea.c;**P1 实证目标** |
| F8 | 真实响应回调 ABI = BaseMsgMgr 注册消费者,非 MSF 层参数 | phaseC/73bd6c.c;**P2 实证目标** |

## 2. G3-P1 现场探测:transport 对象 vptr 捕获交换(v3,本轮实现)

> **r1(镜像 vtable 补丁)已弃用**:2026-10-09 09:55 在实例 21732 上交换
> wrapper .rdata 的 slot7 指针,11 秒后进程以伪装 C++ `std::length_error`
> 终止(crashpad dump 定性,见 ledger 2026-10-09 条目与 `evidence/p6-g3/`)。
> 镜像页改写 = 可检测篡改面,与 G2 的堆指针交换属不同风险级。

### 2.1 目标(通过门)

1. **机制**:slot7 指针交换 → 真实 SSO 发送(心跳/用户消息均可)被捕获 ≥1 次
   → 恢复对称 → QQ 存活。零发送、零 QQ 函数调用(除 getter 租约,沿 G1/G2 先例)。
2. **契约证据**(离线分析捕获):
   - task 0x88 字节转储 → request blob 的 {ptr,ctrl} 候选定位;
   - blob 头 0x20 字节 + vptr → **钉死 F7 的 T 类型**(P2 的构造依据);
   - uin string(F3 形状核验)、policy(F5 核验)、callback 块 0x20B;
   - 若窗口内用户发送真实消息:blob 内含 PbSendMsg protobuf 明文 →
     **P5 编码器字段回填**(qq_protocol send.rs 的 verified:false → 实证)。

### 2.2 机制(v3:对象 vptr 交换,纯堆,同 G2 风险级)

- `transport = *(svc+0x60)`;**身份双门**:transport 对象头 vptr 必须 ==
  wrapper_base + 0x41403B8(raw),不符即中止;
- **伪 vtable**:自分配 RW 内存 16 qword = 原表副本,slot7 → 蹦床,其余槽
  = 原函数。QQ 自身有 Manager 垫片 vtable 先例(G2 发现),对象级 vptr
  多样性是 QQ 原生模式;**不触碰模块映像页**;
- 交换 = transport 对象头一个堆 qword(原子 8B,仅该对象受影响);
- 蹦床:同 r1(LAB 真实执行 6 参验证,含第 5/6 参经栈的 MSVC ABI 形状);
  捕获后尾跳原函数 `0x1B4E4EC`,栈面与直调完全一致。**页永不释放**
  (恢复后迟到调用仍完整工作);
- 捕获处理器纪律(v3 强化):**零分配/零锁/零 format** —— 固定
  96-qword 记录 × 64 槽,seqlock 发布([0] 完成位,Release);全部读取经
  `safe_read_qword`(RPM-self);有界(uin ≤128B、候选 ≤4);JSON 编码
  在窗口结束后由我们线程统一做;
- 恢复:窗口结束 → 确认对象 vptr 仍为伪表 → 写回原值 → 校验。
  被第三方覆写则不回写(座位丢失语义,同 G2);
- 窗口:默认 120s;捕获不依赖用户(心跳/推送 ACK 均走 slot7);
  用户从本实例发送一条消息则额外取得 PbSendMsg fixture。

### 2.3 风险登记(v3)

| 风险 | 缓解 |
|---|---|
| 蹦床首次执行即热路径 | 存根 20 条指令;LAB 真实执行验证;捕获零分配零锁 |
| 对象 vptr 并发读 | 原子 8B 对齐交换(读侧要么旧要么新);伪表页永不释放 |
| 恢复窗口竞态 | 关闸(ACTIVE=0)后迟到调用安全直入原函数 |
| QQ 对 transport vptr 的类型断言 | QQ 原生垫片 vtable 先例(Manager shim);site 阶段核验 raw 后才交换 |
| r1 未知根因再现(时序扰动类) | 镜像改写已移除;若 v3 仍致损 → 本会话路线全停,回静态 |

### 2.4 交付物

- `crates/caligo-bridge/src/g3.rs`(research 路线,导出 `caligo_g3_probe_run`);
- caligo-cli `inject --g3-run-report <abs> --g3-run-ms <ms>`(loader 接线同 g2);
- LAB:`g3` 模块测试(存根编码、捕获记录形状、恢复对称——假宿主);
- 现场:`evidence/p6-g3/`(注入 jsonl + 捕获 fixture + 事后 observe 复查)。

## 3. G3-P2 受控发送(P1 之后,本文仅登记方向)

1. 以 P1 实证的 blob 类型构造请求体(P5 编码器产出 PbSendMsg 字节;
   分配须经 QQ 侧分配器——具体途径依 P1 类型定,`[assumed]`)。
2. 调 `0x7329f6`(成员适配器,bool=1)或直接 slot7:cmd=`MessageSvc.PbSendMsg`,
   目标=机器人账号;回调块自建({payload 0x10, callable@+0x10, dtor@+0x18})。
3. 响应侧:注册消费者途径(F8)或先以 G2 接收侧自同步推送(olpush)为相关证据。
4. 线程准入:G1 先例(getter 从我们线程可调)+ F6 跨线程二次派发结构支持;
   现场首次提交即为终验。
5. 通过门:机器人账号收到测试消息 + 全程零崩溃 + 恢复对称。

## 4. 边界

- 本文不解封任何现场发送;P1 窗口内零提交。
- 注入式仅限 research 构建 + 指定测试实例(21732,主账号 3089665724,
  2026-10-08T22:19:27 创建);生产路线仍按 K4(宿主合法初始化+常驻 bridge)。
- 所有进程内读取 RPM-self;报告路径绝对化;注入前最后一步 research 构建
  (独立 target-dir,DLL 锁则换名)。
