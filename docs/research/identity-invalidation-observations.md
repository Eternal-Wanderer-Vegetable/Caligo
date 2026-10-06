# 身份与失效观察记录(identity-invalidation-observations)

> K1 交付物(计划 K1 第 6、7 条)。状态:**实测未开始**;本文件先固化实验设计与"已完成/未完成"台账,实测后逐条回填。

## 1. 已完成的观察(只读,2026-10-06)

| 观察 | 结果 | 证据 |
|---|---|---|
| QQ 进程拓扑 | 18 进程;2 个主进程候选(19352:2026-10-05T11:32:20+08:00 起,27992:2026-10-05T13:33:25+08:00 起) | local-evidence/k0/env-check-output.txt、k1-probe-process.txt |
| 进程创建时间双源一致 | PowerShell Get-Process StartTime 与自有 probe(GetProcessTimes)一致(修复纪元常数后) | 同上 |
| 版本判定 | 所有进程加载 versions/9.9.33-52230/QQNT.dll;QQ.exe 版本号与实际加载目录一致(本实例) | 同上 |
| PID ↔ 创建时间绑定 | 已实现"两者一起核对"的取数路径(process 子命令输出含 started_utc) | probe-build-record §4 |

以上全部是**进程级**身份证据;账号级身份(account uin、登录态)未经任何实验获取。

## 2. 未完成实验(阻塞于:执行者指定测试实例,见 test-scope §2)

按计划 K1 第 6、7 条设计,逐条记录预期/实际(实验时回填):

### EXP-K1-01 加载与握手(probe 无副作用验证)— **已执行,2026-10-06 通过(加载/握手/探测范围)**

- 前置:manifest PASS(5/5 模块 SHA-256 与冻结基线一致);指定实例 PID 27992(创建时间 2026-10-05T13:33:25+08:00,详见 local-evidence/test-scope-local.md 指定表);隔离复查通过(模块快照无第三方协议端)。
- 干跑先行:注入器先在自建牺牲进程(cmd 哨兵)上完成全链路验证(远程加载 → 远程导出解析 → 探测调用 → 报告回读 → 资源释放),QQ 不参与。
- 操作:`caligo-cli inject --pid 27992 --bridge <release caligo_bridge.dll> --manifest ... --report ... --confirm-designated-test-instance`。
- 实际结果:
  - probe_exit_code=0(OK);remote_base=0x7ff96e9e0000;握手 PASS(protocol_version=1,bridge_build "caligo-bridge 0.1.0")。
  - probe 报告从 QQ 进程内部取得:host_pid=27992,module_snapshot 30 项 Tencent/.node 模块,快照完整。
  - 注入后 liveness:进程存活且 Responding=True,主窗口在;QQ 总进程数 18 无变化;`caligo_bridge.dll`(139264 字节)出现在 27992 的模块列表。
  - **人工收发确认(计划 K1 第 6 条的"正常人工收发")待执行者复核** —— 进程/窗口层面无异常,消息层面由用户日常使用确认。
- 证据:local-evidence/k1-live/exp-k1-01-pid27992-{log,report,liveness}.txt、dryrun-cmd-report.json。
- **重复性(同会话内)**:EXP-K1-01b 二次注入同实例 —— LoadLibraryW 幂等(同基址 0x7ff96e9e0000),
  probe 再次执行成功,报告与首次一致(host_pid 相同),QQ 仍 Responding=True
  (local-evidence/k1-live/exp-k1-01b-repeat-report.json)。
- 遗留:probe 报告不含账号信息(入口契约未闭合,见 §EXP-K1-02);QQNT.dll 在快照中(它必然在),bridge 对 QQNT 导出的调用尚未发生。

### EXP-K1-02 账号可见性(仅观测)— **已执行,2026-10-06,注册面观测完成**

- 前置:执行者手动启动 QQ 并登录(本轮候选 49148/38472,指定 49148,记入指定表);manifest PASS;双门控注入。
- 操作:`inject --obs-report`(新增 `caligo_obs_run`:定位 node_module 链表头 → 只读遍历,不调用任何 QQ/Node 函数)。
- 实际结果(完整报告:local-evidence/k1-02-pid49148-obs.json):
  - 链表头运行时推导 RVA `0x0C7092F0` 与离线静态解码**完全一致**(get_linked_module_rva 0x1C911B0、qq_magic_napi_register_rva 0x1C8B450 同样一致);image_size 218705920 与 K0 观察一致;
  - 遍历 46 个节点,无环、无 cap、无 note;
  - **`major` 以 linked binding 形态注册**(name="major",flags=2 NM_F_LINKED,version=-1)——即主进程 JS 可经 `process._linkedBinding('major')` 取得其导出面;节点 filename 暴露腾讯构建路径 `E:\data\landun\workspace\rx64\v8-bytecode-unified\node\electron_loader.cpp`(蓝盾 CI;v8-bytecode-unified 印证 JS 字节码化);
  - `QQNT` 自身亦为 linked binding(version=-1);其余 44 个节点全部是 Electron 内置绑定(electron_browser_*/electron_common_*,Node ABI version 143 = Electron 40);
  - wrapper.node / qq-proton.node / initIpc_x64.node **不在** linked 列表(已加载但走 per-Environment DLOpen 注册路径)——主进程进程级内置面 = QQNT + major + Electron。
  - liveness:注入后 PID 49148 Responding=True。
- 结论与边界:
  - **A1 入口门找到**:`process._linkedBinding('major')` 是五类入口(账号/会话/接收/发送/结果)候选所在的确定位置;入口**名**仍未知,下一步对 major.node 的注册回调做静态字符串挖掘(离线、零风险)取得候选绑定名;
  - 账号 uin 本身仍未观测(需要 JS 上下文执行或数据面观察,属下一阶段);
  - 本实验全程只读,未调用任何 QQ/Node 代码。

### EXP-K1-02 补充轮(b/c:回调捕获与服务面枚举)— **已执行,2026-10-06**

- 02b(并排 DLL,target-obs 构建,避免锁定冲突):捕获 QQNT/major 两个 linked binding 的注册回调。
  发现:两者 context_register_func 指向**同一通用 thunk**(QQNT RVA 0x01C8B430,与静态解码一致);
  真正的注册函数在 nm_priv(node+0x30)指向的 napi_module 结构内(+0x10)。
- 02c(target-obs2 构建):捕获 napi_module 本体:
  - major:napi_module 在 **major.node RVA 0x58000**,真注册回调在 **major.node RVA 0x22D50**,modname="major";
  - QQNT:napi_module 与注册回调在 **wrapper.node** —— "QQNT" linked binding 由 wrapper.node 提供;
  - node_module 结构体均为堆分配(qq_magic_napi_register 内 new,与静态解码一致)。
- 服务面枚举(离线,identifiers 子命令):major.node 229,199 条标识符字符串
  (local-evidence/k1-02-major-identifiers.txt),得到 nodeIKernel* 服务 30+/监听面清单;
  wrapper.node 为纯 C++ 层(kNTOnAddSendMsg 等 NT 内部事件名)。
- liveness:三轮注入后 PID 49148 仍 Responding=True(三份只读 bridge 共驻,随进程退出回收)。
- 全程只读;未调用任何 QQ/Node 函数。

### EXP-K1-03 退出/重登/失效 — 未执行

- 需要执行者在指定实例上做账号退出/重登操作;进程级核对工具(PID+创建时间)已就绪。

### EXP-K2-00 自有 linked binding 注册机制验证 — **已执行,2026-10-06,负结果但架构性收获重大**

- 前置:方案评估完成(js-context-acquisition.md);指定实例 PID 49148(仍为 EXP-K1-02 实例);双门控注入。
- 操作:新 bridge 导出 `caligo_register_entry` → 远程调用 → 经 QQNT 导出 `qq_magic_napi_register`
  注册自有 napi_module(name="caligo_bridge",flags=2,镜像 major 实测模式;回调仅原子记录 env 指针)。
- 实际结果:
  - 注册调用返回 0(OK);干跑(无 QQNT 进程)优雅返回 2(ERR_NO_QQNT);
  - **obs 链表仍 46 节点、无 caligo_bridge**;entry_fired=false;
  - 离线解码定位根因:**QQNT 有两条链表** —— node_module_register(0x01C8F5EB)维护表 A
    (0x0C7092EA,qq_magic 插入处);get_linked_module 读表 B(0x0C7092F0,linked 查找表)。
    经 qq_magic 注册的绑定不进入 lookup 域,回调永不触发。
- 证据:local-evidence/k2-00-*.json/txt、k2-00-static-node-module-register.txt。
- 结论:方案 b(单靠 qq_magic)不能取得 env;**自建 Environment 路线(方案 d)确认为主路线**,
  所需导出 API 全部核实存在(CreatePlatform/NewIsolate/CreateIsolateData/NewContext/
  CreateEnvironment/AddLinkedBinding/LoadEnvironment/uv_loop)。详见 js-context-acquisition.md §3-4。
- liveness:注入后 PID 49148 Responding=True。

### EXP-K1-04 安全收尾 — **已执行,2026-10-06 闭合**

- 人工收发确认(计划 K1 第 6 条):执行者确认实验期间 TEST-ACCOUNT-A(私聊对象 FRIEND-B)消息收发无任何异常。
- 实例退出:执行者以正常方式退出全部 QQ 实例;复核 `Get-Process -Name QQ` = **0 个**,指定实例 PID 27992 已消失。
- 崩溃残留排查(近 6 小时):
  - `%LOCALAPPDATA%\CrashDumps`:无近期 .dmp;
  - WER ReportArchive/ReportQueue:无 QQ 条目;
  - Windows Application 事件日志 Level 1/2:无 QQ 相关错误,且无任何应用错误条目。
  - 证据:local-evidence/k1-live/exp-k1-04-crash-check.txt。
- 安装完整性:实验后 `verify-manifest` 复核 5/5 模块 SHA-256 与冻结基线一致(实验未触碰 QQ 安装)。
- bridge DLL:实例退出后文件锁释放,按源码(含 `unsafe extern` 签名修正)重建为新哈希,并在牺牲进程上完成冒烟验证(加载/探测/握手全链路正常)。
- 结论:加载 → 验证 → 正常使用 → 正常退出 → 无残留,整个 EXP-K1-01/04 生命周期闭环完成,未观察到任何异常。

## 3. 台账小结(对计划"你现在的第一轮执行清单"第 6 条的回应)

- **G1 通过?不宣称。** 加载链(EXP-K1-01)与运行时注册面(EXP-K1-02)均已实测;G1 的
  "重复获得真实账号与有效会话"仍缺账号级证据。
- **已补的证据**:EXP-K1-01(加载/握手/重复性/liveness)、EXP-K1-04(收尾/无残留)、
  EXP-K1-02(注册面:major = linked binding 实证,链表头三方交叉验证一致)。
- **缺的证据**:major 的绑定函数名清单(下一步:major.node 注册回调静态字符串挖掘,离线零风险)、
  每个 §6.1 契约字段、账号 uin 观测(需 JS 上下文)。
- **A1→A2 判定?不触发。** major 入口门已实证存在,五类入口候选位置确定;A1 仍是主路线。
- **不执行的动作**:不调用任何 major 绑定函数;不在 JS 上下文做任何写实验;不试偏移、不升版本。
