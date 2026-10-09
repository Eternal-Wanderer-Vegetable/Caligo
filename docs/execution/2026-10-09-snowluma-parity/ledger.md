# 执行台账:SnowLuma 能力对齐 N0–N2(2026-10-09)

> 计划:`docs/plans/2026-10-09-gitnexus-plan-caligo-snowluma-capability-parity.md`
> 本台账只记录执行事实与证据归属;不把静态/LAB 结果写成现场通过。
> 本轮范围(计划 §13):N0 基线修复 + N1/N2 可验证合同;不注入 QQ、不操作真实会话。

## N0 — 基线

### 基线登记

| 项 | 值 |
|---|---|
| HEAD | `cb263ac953c1306696bb6ebcdedefec6e4ef22be` |
| 起始 dirty | 仅 `?? docs/plans/2026-10-09-gitnexus-plan-caligo-snowluma-capability-parity.md`(与本计划一致) |
| 工具链 | rustc 1.97.1 (8bab26f4f 2026-07-14) / cargo 1.97.1 (c980f4866 2026-06-30) |
| 目标目录 | `E:/stella/_reference/caligo-snowluma-parity-20261009/cargo-target`(与注入现场 DLL 分离) |
| wrapper.node | 未读取本轮运行实例;冻结摘要 `63112ab9…87a5f0` 仅作 profile 记录,不作本轮运行准入(计划 §1) |
| 新依赖 | 无(全部离线,`--locked --offline`) |
| QQ 操作 | 零(未注入、未探测、未发送) |

### 样本索引(不移动历史证据,只建索引)

现场成功样本(证据 `docs/execution/2026-10-08-native-contract-recovery/evidence/p6-g3/`):

| 样本 | 实例 | 内容 | 状态 |
|---|---|---|---|
| r9 | 24316(私聊) | 干净文本送达+渲染;encode len=123,head 前缀含真实 peerUid | 归档 |
| r18 | 27624(群 263402786) | 群文本送达+渲染;encode len=173,head=`0a07120508a2eacc7d1a93010a90011276aa027350006000`(群号 varint + elem37 118B 起点) | 归档 |
| r19/r19b/r19c | 27624 | auto-seq 单调(seq 1→13)+ rdtsc random,三条全送达 | 归档 |
| r24 | 27624 | 普通文本对照组(同桥同时段 PASS,用于 @ 拒因隔离) | 归档 |

失败/否决样本索引:

| 样本 | 内容 | 结论 |
|---|---|---|
| r10–r15 | 群盲试六轮 | 静默拒;后定位 elem37 常量损坏(88B 截断+0x06 错字节) |
| r20–r28b | @ 元素八轮 | 静默拒;形状知识全部入代码,阻塞于响应 retCode;r24 对照组排除桥/频率因素 |
| slot7 插桩 r1/r2(v3) | 镜像 vtable / 对象 vptr 交换 | **2/2 实例损失,路线整体否决**(台账 2026-10-09 再续) |

### N0-2 测试基线修复

| 变更 | 内容 | 动机 |
|---|---|---|
| `g3.rs` `send_msg_encoding_friend` | 8 参签名补齐(`at_uid=None, at_uin=0`);真实 UIN 常量合成化;断言钉到已证真实结构(uid 路由叶、ContentHead 三键 `{1:1,2:0,3:0}`、元素37 标记、正文叶) | E0061 参数回归;按计划禁止"只加参数让过期断言继续绿" |
| `legacy_gate.rs` | research 构建下运行期跳过(`caligo_bridge::gate::research_enabled()` 常量判断);常量域断言保持无条件 | `caligo-core/research = ["caligo-bridge/research"]` 特性转发使 workspace research 构建下 D0 拒绝合同天然不成立——与 `gate.rs`/`onebotd_gate.rs` 的条件编译口径对齐(后两处分别用运行期/编译期手段,cli 无自有 research feature 只能运行期) |

### N0 验收命令结果(2026-10-09,本轮实测)

| 命令 | 结果 |
|---|---|
| `cargo test --workspace --features caligo-core/research --locked --offline` | **173 passed / 0 failed**(N1/N2 新增 22 用例后终验;修复前:E0061 编译失败 + 4 legacy_gate 失败) |
| `cargo test --workspace --locked --offline`(普通构建) | **152 passed / 0 failed**(终验) |
| `cargo test -p caligo-core --features research --locked --offline` | **72 passed / 0 failed**(终验) |
| `cargo test -p caligo-bridge --test native_adapter_contract --features research --locked --offline` | **7 passed / 0 failed**(含于 workspace 计数)
| `cargo test -p caligo-cli --test legacy_gate --locked --offline` | **4 passed / 0 failed** |
| `cargo test -p caligo-core --test onebotd_gate --locked --offline` | **2 passed / 0 failed** |

声明:以上均为离线合同测试;现场 r9/r18/r19 成果未被重置(未触碰 QQ)。

## N1 — 共享 codec 与接收映射

### N1-发送侧:共享 codec 迁移(C1)

| 变更 | 内容 |
|---|---|
| 新增 `crates/caligo-model/src/qq_wire/{mod,send}.rs` | 发送编码权威源:pb 原语 + `encode_friend_text`/`encode_group_text`(field-proven r9/r18 形状;时间显式入参;超界显式拒绝);元素37 常量冻结(私聊 26B/群 118B,出处注释) |
| `g3.rs` 薄包装化 | 普通文本路径(私聊全部/群无 @)委托共享 codec;@ 研究分支(r20–r28b 归档,静默拒)留宿 g3 并显式标注"非生产支持面";未用常量/死分支移除 |
| `caligo-core qq_protocol/send.rs` 重接线 | 委托共享 codec;`SendTarget::Friend` 增 uid 字段(r6:uid 路由才是正确渲染路径);`FIELD_NUMBERS_VERIFIED` → `true`(发送侧,r9/r18 实证);新增形状合同测试(群无 CH、uid 路由、uin 降级、元素37 群版标记) |
| golden fixtures | `crates/caligo-model/tests/qq_wire_golden.rs`:6 用例,期望字节由测试内独立微型构建器拼出(与被测代码零共享);r9/r18 归档 head 形状字节回放(真实 uid/群号字节排除,合成身份替代);群无 ContentHead(T02);Unicode 完整(T03);同文不同 seq/rand 不同字节;超界拒绝;元素37 常量冻结断言 |

声明:共享 codec 与 golden 全部离线合成身份;**未做现场发送**,r9/r18 现场结论未被重置。g3 编码经包装层产出与迁移前逐字节同构(其原单测断言组保持全绿,新增 ContentHead 三键/uid 叶断言)。

### N1-接收侧:owned 捕获与规范化(记录级映射 blocked)

新增 `crates/caligo-bridge/src/native_receive.rs`(纯数据+映射,零 FFI 调用):

- `CapturedRecordHead`:8 qword 拥有式捕获记录;`Standard`(Type S)/`Pointer`(Type P)/`Unreadable` 三态;分类规则与归档形态锚定。
- **已证字段**(p5-correlation-21732-r4.md + 归档 g2-run-21732-r4-p5.jsonl):Type S 头 +0x00 = 本机账号 SSO(24B 跨 q0–q2,短形态全数字才认定);+0x10 = 逐消息 32 位(候选,未定证为 native ID 组成)。
- **未证字段显式 None**:`NormalizedNotice` 的 sender/peer/platform_time/native_id 恒 `None`(Option 表达"证据未到",禁止空串/本地时间/hash 冒充);不携带正文候选(N1 禁止从共享池组装业务事件)。
- 测试 5 用例:同批自账号恒定+逐消息 32 位变化、不发明合同、非数字 SSO 拒绝、Type P 分型、不去重语义。

**receive mapping blocked(按计划 N1 失败分支处理)**:sender/chat_type/平台时间/native ID 的记录内位置未定证 —— 离线对象图测绘对象为归档池数据与 0x200 头捕获,需现场窗口;p5-correlation 待办清单为下一入口。raw heads 数量不得计为业务消息数(测试固化不发明合同)。

## N2 — 响应消费者合同(离线可闭合部分)

新增 `crates/caligo-bridge/src/native_response.rs`(纯账本,零 FFI 调用):

- **预登记合同**(计划 §5-2):`register(request_id, cookie)` 先于原生发起;全零 cookie/重复 cookie/重复 request 拒绝;登记绑定发起时刻 `(host_generation, session_generation)`。
- **统一结果账本**(T06/T07/T08/T13):`complete(cookie, outcome)` 任意时序(inline 早于返回/重复/乱序/迟到);首次终结、重复审计不翻案、旧代次审计、未预登记 cookie 审计(`LedgerAudit` 计数);三分类 `BusinessSuccess{native_id}(类型强制携带稳定身份)/BusinessRejected{retCode,err}/TransportFailed` —— 原生返回≠业务成功的边界在类型上闭合(与 receipt.rs Unconfirmed 口径一致)。
- **关闭排空**:`drain` 把未终态表达为显式 Unknown 并移除;已终态保留待交付;不产生可被误读为"可重发"的信号(T13)。
- **seq 分配器**(T09):`SeqAllocator::allocate` 先持久化再交付,持久化失败拒绝分配(不前进);恢复只前进,崩溃窗口空洞保留(已发起 seq 绝不复用);作用域 = 单计数域(r19 证据:服务器不校验会话 seq,唯一失败模式自发撞车)。

与既有部件分工:`native_msf::MsfAdapter` 的 request_id 键控在途账本(假宿主合同)保留;cookie 键控预登记层在其上组合(request_id↔cookie 双向),真实 ABI 接线属 N3。

**未闭合(现场依赖,不在本轮)**:response-consumer-scope.md 四项 —— 940588 门控语义、97b0f/58e72 key 组合格式、请求 PB msgUid 字段号、消费者 A 实参 ABI。完成前 @ 维持归档,发送失败模式仍为静默(响应无消费者)。
