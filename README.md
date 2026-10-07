# Caligo

自有 Rust QQ 接入项目:自有桥接(bridge)官方 QQ 会话 → 自有 Rust 核心(core)→ 自有 OneBot V11。
运行链路不依赖 SnowLuma、NapCat、LLBot、PMHQ 等第三方协议端或专有内核;官方 QQ 自身承担登录、签名、会话与网络通信。

> 当前状态(2026-10-07,K4 阶段):**项目拥有真实收发的研究原型能力**(K3 群/私聊收发有执行者验收记录),但**注入式现场路线已冻结**——六次测试实例损失后(`local-evidence/k3-g/final-boundary.md`),对存活 QQ 的注入类探针暂停;K3 文档中"生产层定型"的表述由 `docs/research/k4-execution-scope.md` 修正为"能力证明,非生产定型"。K4 按 `docs/plans/2026-10-07-gitnexus-plan-caligo-k4-runtime-recovery.md` 重建:宿主合法初始化 → 常驻 bridge → 命名管道 → 常驻 core;现场部分保持暂停直至 D6 准入。普通构建下 `inject`/旧 onebotd/相关 DLL 导出默认拒绝(`--features research` 仅供自建宿主离线研究)。

## 仓库结构

| 路径 | 说明 |
|---|---|
| `2026-10-06-gitnexus-plan-caligo-native-qq-kernel.md` | 第一阶段实施计划(K0-K6 逐阶段门槛) |
| `docs/research/environment.json` | K0 环境/工具链/模块摘要记录 |
| `docs/research/source-register.md` | 来源登记(采用/参考/拒绝/待审,含固定版本) |
| `docs/research/test-scope.md` | 测试范围与唯一正文约定(真实账号只在本机 local-evidence) |
| `docs/research/recovery-notes.md` | 恢复办法与实验收尾流程 |
| `docs/research/native-entry-contract.md` | K1 入口契约(候选入口、证据、未知项) |
| `docs/research/loading-route-decision.md` | 加载路线决定(自有 Rust loader,拒绝文件修补) |
| `docs/research/probe-build-record.md` | probe 构建与测试记录 |
| `docs/research/identity-invalidation-observations.md` | 身份/失效实验设计与台账 |
| `docs/contracts/version-adapter-manifest.json` | 单版本模块摘要适配清单 |
| `crates/caligo-model` | 数据模型:账号、会话、方向、代次 |
| `crates/caligo-bridge` | 进程内 bridge(cdylib;K4-D0 起宿主入口默认门控,新增 `gate`/`resident`) |
| `crates/caligo-core` | 进程外核心(library;IPC 帧/握手,K4 增加 runtime/journal/transport) |
| `crates/caligo-cli` | CLI:`process` / `exports` / `verify-manifest` / 已冻结的 `inject` |
| `docs/research/k4-execution-scope.md` | K4-D0 遗留路线冻结与门控记录 |
| `docs/research/k4-incident-register.md` | K4-D1 事故台账与资源审计 |
| `docs/research/k4-bootstrap-contract.md` | K4-D2 bootstrap 候选契约 |

## 构建

```bash
cargo build --workspace          # 调试构建
cargo test --workspace           # K4-D0 起含门控/生命周期测试
cargo build --release --workspace
```

需要 Windows x64 + Rust MSVC 工具链(版本见 `rust-toolchain.toml`)。

## probe 用法(只读)

```bash
caligo-cli process                                   # 枚举 QQ 进程与 Tencent/.node 模块
caligo-cli exports <module-path> [--all]             # PE 导出表
caligo-cli verify-manifest docs/contracts/version-adapter-manifest.json
```

`inject`(旧注入路线)在普通构建中**默认拒绝**(退出码 3);只读子命令不受影响。
恢复条件与 K4 分阶段准入见 `docs/research/k4-execution-scope.md` §5;
使用与恢复的历史流程见 `docs/research/recovery-notes.md`。

## 纪律(摘自计划)

- 模块摘要不匹配 → 拒绝 attach/发送,不猜偏移。
- 运行中的 QQ 实例不会被自动选为实验目标;每次实验由执行者指定并记录。
- 真实账号、聊天内容、QQ 二进制不入库;现场证据只存本机 `local-evidence/`(已 gitignore)。
- 在入口契约闭合(ABI/线程/生命周期)前,不调用任何发送入口。
