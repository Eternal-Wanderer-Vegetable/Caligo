# K4-D7 首次接入报告:候选 B 现场执行(k4-d7-first-attach)

> D7 交付物(计划 §7-D7)。执行:2026-10-07 17:07–17:43(Asia/Shanghai)。
> 指定测试实例:执行者于 17:07 新建并登录 TEST-ACCOUNT-A;主进程判据
> (wrapper.node 唯一命中)与 PID/创建时间登记于本机
> `local-evidence/k4/d7-a/instance.md`(不入 Git;本报告只用别名与机制值)。
> 构建:release + `caligo-bridge/research`;门 1(manifest 五项 PASS)与
> 门 2(执行者确认)逐层通过。old probe/exec/onebotd 全程未用。

## 1. 执行序列与结果

| 步骤 | 动作 | 结果 |
|---|---|---|
| 0 | 只读侦察:进程枚举 + 模块判据 + envscan(0.28s/0.73GiB/read_failed=0) | 唯一命中,env 候选地址取得 |
| 1 | `qq-entry` 首入(绝对 bridge 路径) | **exit 0x0 = OK** |
| 2 | 单实例探针(第二次 bootstrap 调用) | **exit 0x16 = AlreadyBootstrapped** |
| 3 | 10 分钟观察窗(60s 间隔只读采样) | 全程存活;RSS 755–758 MB(±3 MB 无趋势);线程 141–142 稳定 |
| 4 | `qq-entry-stop` 关闭协议(wait 10s) | **exit 0x0 = OK**(closed 确认) |
| 5 | 关闭后 3 分钟稳定观察 | 存活;RSS 恒 756 MB |

停止条件检查:全程 **零 AV、零 QQ 非预期退出、零线程/上下文异常** —— 未触发任何 Quarantined 条件。

## 2. B0-FIELD 判定(对照 bootstrap-contract §3 十项表)

| 项 | 判定 | 证据(机制级) |
|---|---|---|
| 宿主进程角色 | **PASS** | 唯一 wrapper 命中;执行者显式指定 + PID/创建时间登记 |
| 装载点 | **PASS** | research bridge 经门控 loader 加载;首入经显式导出调用 |
| 真实回调来源 | **PASS** | bootstrap 完成 = RequestInterrupt 回调在 JS/loop 线程执行且通过零 current 核对(任一失败即对应错误码,得 0x0 即全链通过) |
| API/ABI 版本 | **PASS** | 6 个符号(QQNT 导出面)全部解析成功(ERR_RESOLVE 未触发);sret/ABI 按 K2-03 实证规则调用无事故 |
| owner 线程 | **PASS(机制级)** | uv_async_init 在首入回调线程完成;关闭经 pump 在**同一线程**执行(shutdown 0x0 要求 tid 一致,否则 pump 拒绝并 Quarantine) |
| 合法环境来源 | **PASS** | envscan 只读扫描 + 新鲜度门首验即过(vfptr = qqnt+0xA804990) |
| 引用持有 | UNKNOWN | D8(会话/监听器层) |
| 会话失效信号 | UNKNOWN | D8 |
| cleanup 来源 | **PASS** | CLOSE_REQ → owner pump → uv_close → 关闭回调确认;关闭后实例稳定 |
| 一次性初始化与版本拒绝 | **PASS** | 二次调用 0x16;门 1 基线不符即拒(全程生效) |

**B0-FIELD = PASS(受限)。** G1 核心判据闭合:合法 owner 入口、对象生命期
(bootstrap→ready→closed)、进程稳定、stop 闭合全部有现场证据。
受限项(如实,不降级为 PASS):
1. **阶段 JSONL 日志丢失**:CLI 把相对报告路径写进远程 ctx,DLL 按 **QQ 进程
   工作目录**解析导致写入失败(Program Files 不可写,静默失败)。owner 线程
   **数值 tid**、isolate/loop 地址未取回 —— 证据以退出码链 + 外部只读观察为准。
   缺陷已修复(qqentry.rs 路径绝对化),D8 起生效。
2. **账号身份(TEST-ACCOUNT-A)未在本阶段验证**:属会话层,归 D8。
3. **"core 断开/恢复一次"未执行**:bridge 尚无管道客户端,管道在 daemon 侧。
   归 D7-b(D8 前的接线阶段),不由本报告冒充。

## 3. 与历史路线的对照(为什么这次没有重蹈 K3)

| K3 缺陷(事故台账) | 本次 |
|---|---|
| 远程线程直调 v8(I-03/04) | 宿主 API 全部在 RequestInterrupt 回调 = JS/loop 线程执行 |
| 零 current 放行(§5.1) | 零 current → InterruptNoCurrentContext,LAB 与现场同一机器 |
| uv_async_init 在远程线程(违例恰未崩) | init 在 owner 线程完成;跨线程只有 uv_async_send |
| 句柄从不 close | CLOSE_REQ → uv_close → 关闭回调确认;超时保留句柄 |
| 泄漏式 Box::leak / 每轮新建 | 单次 bootstrap;二次调用显式拒绝(0x16) |

## 4. 过程缺陷记录(不影响判定,均如实)

1. 相对 bridge 路径:第一次 attach 的 LoadLibraryW 在 QQ 进程内解析失败
   (快照核查拦截,QQ 内仅一次失败的 LoadLibrary 调用)→ 改绝对路径成功。
2. 相对报告路径:阶段日志丢失(§2 受限项 1)→ 代码已修复。
3. 关闭后的**同进程再绑定**当前不支持(close 后 ready 不复位,再 bootstrap
   拒绝):§6.2 允许 cleanup 完成后逻辑重绑定 —— D8 设计输入,或每代次一新实例。

## 5. 结论与下一步

- **G1(B0-FIELD)核心闭合**;D6 表 2.5 项闭合(指定实例已登记)。
- 下一步 **D7-b/D8**:bridge 侧管道客户端 + 常驻 resident 接线(把
  daemon_chain 的 LAB 链延伸进 QQ),随后 D8 接收样本(需监听器接线,
  引用持有与会话失效信号两项 UNKNOWN 在该阶段闭合)。
- 本机证据(不入 Git):`local-evidence/k4/d7-a/`(instance 登记、envscan
  报告、10 分钟观察日志、post-stop 日志)。
- 实例处置:生命周期已 Closed;DLL 代码按 §6.7 留至 QQ 正常退出,
  不做热卸载;实例交还执行者正常使用/关闭。
