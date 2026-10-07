# K3-E 事故记录:13056 环境悬垂崩溃(2026-10-07 10:07 UTC)

## 事件

exec 首次实弹(ARMED 探针)触发 QQ 崩溃,实例 13056 死亡。crashpad
63a82da9(已归档本目录):frame3 = QQNT+0x429A0D1(GetEnteredOrMicrotaskContext
内部,与 K2-03 事故同函数),frame4/5 = caligo_bridge_e_100637.dll
(0x1AD19 = call_sret1(entered_ctx) 的返回地址,反汇编确认)。

## 根因:env 指针陈旧(stale env)

- 探针使用的 env=3cec002ad800 来自数小时前的 envscan;
- 期间 QQ 内部状态变化(账号登出/环境回收),该 Environment 已被释放;
- env+0xA0 读出悬垂垃圾 isolate → 页校验只验证"已映射"不验证"有效" →
  上下文访问器内部 AV → crashpad 接管 → 进程死亡;
- QQ 随后自动重启(新进程树)。

## 修复(已入库)

- exec.rs/asyncrun.rs 增加 **env 新鲜度门**:`[env+0]` 必须 ===
  `qqnt_base + 0xA804990`(主 vtable RVA,与 K2-02 图谱同源);
  失配即拒绝并提示重扫(envscan)。qqnt_base 经加载器传入(ExecCtx 增列);
- 该门对所有 env 类探针生效(async 同步修补)。

## 教训

- K2-02 的 env 是"扫描时快照",不是稳定句柄——**任何跨时间复用前必须
  重新验证**;本次事故与 K2-03 两次事故同类(调用前未验证目标有效性),
  防御性校验必须在使用点而非获取点。

## 现状

- 13056 死亡;现存主进程树 22532(2026-10-06T23:39 启动,账号归属未确认,
  未触碰);crashpad 已归档;修复已构建。

## 修正归因(第二次崩溃,49688 树,2026-10-07 10:31,crashpad 9a0d383c)

- 49688(env 全新鲜)同样崩在 GetEnteredOrMicrotaskContext → **推翻"陈旧 env"
  单因归因**。真正机制:**上下文阶梯在远程线程上调用**——
  GetEnteredOrMicrotaskContext 读取当前线程的 v8 TLS,外来线程上是未初始化
  垃圾,无论 isolate 是否有效。13056 的崩溃同为此机制(陈旧 isolate 只是
  让路径更短)。
- 修复:阶梯移入 exec_cb(主循环线程)——asyncrun 六十余次零事故的架构。
  新鲜度门保留(卫生措施,防止悬垂 env)。
- 事故序列:13056(10:07 崩)、49688(10:31 崩)——均为阶梯位置缺陷,
  一次修复覆盖。两次 crashpad 均已归档本目录。
