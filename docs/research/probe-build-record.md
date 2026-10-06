# probe 构建记录(probe-build-record)

> K1 交付物。构建时间:2026-10-06(Asia/Shanghai)。

## 1. 构建环境

- rustc/cargo 1.97.1(8bab26f4f),target x86_64-pc-windows-msvc(见 docs/research/environment.json)。
- 工具链由 `rust-toolchain.toml` 固定;依赖由 `Cargo.lock` 锁定(windows-sys 0.61.2、sha2 0.10.9、serde/serde_json、workspace 内部 crate)。
- MSVC:VS 18 Community,MSVC 14.51.36231;Windows SDK 10.0.26100.0。

## 2. 命令与结果

| 命令 | 结果 |
|---|---|
| `cargo build --workspace` | 通过(0 warning) |
| `cargo test --workspace` | **17 通过 / 0 失败** |
| `cargo build --release --workspace` | 通过 |

产物(release):

| 产物 | SHA-256 |
|---|---|
| `target/release/caligo-cli.exe` | 52139379FEDFA7CB7DCD3304D15A785C929C6D6E8DB4E6B40DB3391479B52071 |
| `target/release/caligo_bridge.dll` | 4C6ADD00D5DD725A1B44915E839F1150C984D1D2B49FC689682669170232FC9B |

## 3. 测试覆盖(与本计划验收项的对应)

| 测试 | 场景 |
|---|---|
| core/tests/ipc_contract.rs ×9 | 帧编解码往返、半包、多帧、超长帧拒绝(T12 部分)、魔数拒绝;握手接受、版本不匹配拒绝(T02 逻辑层)、基线不匹配拒绝(T01 逻辑层)、账号不匹配拒绝 |
| model 单测 ×3 | 同数字 private/group 会话区分(T03 逻辑层)、账号入键、NativeMessageRef 占位不可滥用 |
| bridge 单测 ×2 | 探测报告 JSON 字段与确定性 |
| cli 单测 ×3 | PE 解析拒绝非法输入;FILETIME 换算已知值(硬编码正确基准,防循环验证);自身进程创建时间合理性(GetProcessTimes 实测路径) |

**构建期发现并修复的缺陷**(记录供复核):
1. FILETIME→Unix 纪元常数写成 11_644_473_600_000_000(少一个数量级),导致进程创建时间解码为 2358 年。
   初始单元测试与实现共用同一错误常数,形成循环验证;已改为硬编码正确基准 + 自身进程实测双重校验。
2. windows-sys 0.61 的 `CreateRemoteThread` 需 `Win32_Security` feature;`Read/WriteProcessMemory` 位于
   `Win32_System_Diagnostics_Debug` 模块(编译期发现,已修正 feature 与导入)。

## 4. 只读调查命令实测记录

| 命令 | 结果 |
|---|---|
| `caligo-cli process` | 18 个 QQ 进程;主进程候选(19352/27992)含 wrapper.node+major.node+qq-proton.node+ipc.node+initIpc_x64.node;创建时间与 PowerShell 交叉一致 |
| `caligo-cli exports QQNT.dll --all` | 3385 个导出(全量:local-evidence/k1-exports-qqnt-dll-full.txt) |
| `caligo-cli exports major.node` | 0 导出 |
| `caligo-cli exports wrapper.node --all` | 66 导出(会话壳 C++ 类;`__qq::std` 自建 STL 命名空间) |
| `caligo-cli exports qq-proton.node --all` | 2 导出(napi_register_module_v1 等) |
| `caligo-cli exports ipc.node --all` | 32 导出(djinni IPC 接口) |
| `caligo-cli exports initIpc_x64.node --all` | 0 导出 |
| `caligo-cli exports caligo_bridge.dll --all` | 4 导出(解析器自校验通过) |

## 5. 未验证事项(如实记录)

- `caligo-cli inject` 的远程加载/远程导出解析序列**未在真实 QQ 实例上运行过**;
  首次运行须满足两道门(manifest PASS + `--confirm-designated-test-instance`),并按 recovery-notes §2 收尾。
- bridge 的握手报告仅覆盖"已加载"层;真实账号与会话观察依赖入口契约闭合(native-entry-contract §3)。
