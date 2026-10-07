# K4 现场准入核对表(k4-field-entry)

> D6 交付物(计划 §7-D6)。逐项 PASS / FAIL / UNKNOWN;**UNKNOWN 等同未通过**。
> 这里的 PASS 只准入 D7 最小初始化验证,**不代表 B0 已经现场通过**。
> 填写纪律:每项标注证据来源(文档/测试/现场记录);失败分支指向对应 D 编号。
> 本表通过前,`local-evidence/k3-g/final-boundary.md` 的现场暂停继续有效。

## 1. 前置阶段闭合(离线项)

| # | 条件 | 判定 | 证据 |
|---|---|---|---|
| 1.1 | D0 默认封闭:CLI inject / bridge 导出 / 旧 onebotd 普通构建拒绝 | **PASS** | `docs/research/k4-execution-scope.md`;legacy_gate 4 项 + onebotd_gate 2 项 + 门控单测 |
| 1.2 | D1 事故事实核对完成,无未追溯损失 | **PASS** | `docs/research/k4-incident-register.md`(六例入账;"无 caligo 帧"已修正) |
| 1.3 | D2 候选合法来源确立,B0 表可解释合法首次调用 | **PASS(离线)** | `docs/research/k4-bootstrap-contract.md`(候选 B;[assumed] 项留给 D7) |
| 1.4 | D4-LAB 通过:owner 检查先于宿主 API、关闭回基线、10k+100 轮无泄漏 | **PASS** | resident_lifecycle 11 项(计数分配器) |
| 1.5 | D5-LAB 通过:全链(管道→actor→journal→客户端)、认证/代次/取消/恢复 | **PASS** | daemon_chain 3 + transport_contract 3 + caligod_smoke 1 + runtime/recovery 契约 16 项;阶段报告 01 |
| 1.6 | 日志、资源计数、stop 与 Quarantined 路径已接线 | **PASS** | ResidentCounters/RuntimeCounters;close/Quarantined 契约测试 |

## 2. 版本与目标实例(执行者项)

| # | 条件 | 判定 | 证据 |
|---|---|---|---|
| 2.1 | 冻结版本 9.9.33-52230 模块摘要与磁盘安装一致 | **PASS(2026-10-07 核对)** | 五项 SHA-256 与 environment.json 逐字节一致;D7 当日须**复验一次**并记录 |
| 2.2 | API/ABI 版本记录(Node 24.11.1 / V8 14.4 / Electron 40;vtable RVA 绑定) | **PASS(静态)** | K2-03 r3 指纹;native-entry-contract §1 |
| 2.3 | 指定测试账号与对端样本已登记(仅本机) | **PASS(执行者已填写)** | `local-evidence/test-scope-local.md`(值不入 Git;使用别名) |
| 2.4 | 进程角色与创建时间判定标准明确(wrapper.node 独载 + PID+创建时间双核) | **PASS** | native-entry-contract §1;test-scope §4 |
| 2.5 | D7 目标实例:专为实验新建、登录 TEST-ACCOUNT-A | **UNKNOWN(待 D7 当日)** | 执行者启动并记录 PID+创建时间到 local-evidence/k4/ |

## 3. 风险与恢复面

| # | 条件 | 判定 | 证据 |
|---|---|---|---|
| 3.1 | 每个 unsafe 入口有入口/线程/生命周期契约;现场待验证点最小化 | **PASS** | host_adapter 契约注释;qq_entry 阶段化机器(每阶段可独立取证/中止) |
| 3.2 | 现场待验证入口的清单与最小调用明确 | **PASS** | bootstrap-contract §3 十项表(逐项标注 [assumed]) |
| 3.3 | 一次崩溃/身份漂移立即停止的条件已落实 | **PASS** | resident Quarantined 契约;daemon degrade;D7 停止条件(计划 §7-D7) |
| 3.4 | 旧 probe/通用 exec/onebotd 不可复用(即使 research 构建) | **PASS** | D0 门控;k4-execution-scope §3 边界清单 |
| 3.5 | 人手可恢复指定测试实例(正常退出路径) | **PASS** | recovery-notes §2;不改安装、不碰无关实例 |
| 3.6 | 候选 B 首入机器已构建并通过 LAB 符号注入测试(真实 QQNT ABI 除外) | **PASS(离线)** | qq_entry_lab(research 构建) |

## 4. 结论

- 离线项全部 PASS;**剩余 UNKNOWN 仅 2.5(D7 目标实例)**,由执行者在 D7 当日闭合。
- 按计划 §7-D6:**条件闭合即准入 D7 最小初始化验证**。D7 结果回填
  `docs/research/k4-bootstrap-contract.md` 的 B0 表,并产出首次接入报告。
- 执行者签字:____________ 日期:____________
