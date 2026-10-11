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

### 观测记录:一次性测试进程崩溃(未复现,如实登记)

提交前终验期间,`caligo_bridge` lib 测试二进制在全仓并行运行中出现一次
`STATUS_ACCESS_VIOLATION (0xc0000005)`,同批随后 5 次运行未复现
(bridge lib ×3 全绿 42/0;全仓 research ×2 全绿 173/0)。bridge lib 含
刻意的非安全内存操作用例(swap_slot/volatile 读/故意泄漏的候选追逐),
为可疑但**未定证**的来源;全仓并行(多测试二进制同机并发)是可疑触发
条件。不据此宣称修复;若复现,按"先单二进制复现、再并行复现"定位,
`native_receive`/`native_response`(纯安全代码,无 unsafe)已排除嫌疑。

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

## N3 — 常驻会话层 + 实例登记(2026-10-09 深夜,第二轮)

### 现场:测试实例登记(只读,零注入)

用户指定"最后启动的 QQ 实例"为测试实例。枚举:22:55:09–22:55:15 启动进程组
(27108/36396/17360/39512/45488/44324/18940),另 22:57:19 有 17004;
旧 26104(主账号,10:40 起)仍在,27624(bot)已不在。

**observe-msf t1(PID 27108,外部只读,证据 `E:/stella/_reference/caligo-n3-resident-20261009/observe/observe-27108-t1.json`)**:

| 项 | 结果 |
|---|---|
| wrapper 基址/内存大小 | 0x7fff17ee0000 / 121,737,216(文件大小 115,118,632 与冻结一致;内存视图含映射填充) |
| MSFService 锚点 | **命中**:obj=0x1d76eb68000,vptr RVA 0x3f6ded8 ✓,strong=84 |
| transport | +0x60 在位(0x1d76e84da30),vptr RVA 0x41403B8 ✓;**slot7 RVA 0x1B4E4EC ✓** 全部与冻结锚点一致 |
| Manager | 垫片形态(SC=0x1d76e88ea40,+0x150→0x1d76e89e9c0,vptr 堆垫片),树 size=2,2 监听器 |
| dispatcher | 全局在位;TLS index 44;41 个线程已安装 |

锚点级兼容成立。**实例账号身份已确认(外部 RPM 读 svc+0x1f0,短形态串)**:
self = **1694717255**(bot 号,与 r10–r28b 同一账号)——即重启后的新 bot 实例;
发送测试的对端观察账号沿用主账号 3089665724。本轮零注入,任何后续注入按
双门纪律(manifest + 指定确认;用户已指定本实例)。

### 离线:响应消费者静态合同收敛(范围文档已更正)

以 phaseAE2 反编译 + 活实例外部 RPM 字符串读取(`resp-consumer-strings.json`,
只读)收敛范围文档四项(详情见 `docs/research/response-consumer-scope.md` 增补):

| 项 | 结果 |
|---|---|
| 消费者调用 ABI | **闭合**:`(inner, 0x10, errCode:int, errStr:string, msgUid:string)`;errCode 优先级 resp+0x40 → +0x60 → +0x64;resp 空 = **-1** 超时;msgUid = string@resp+0x38 |
| "注册"语义 | **更正**:c707b6 = 序列化+日志(源路径串证实),非注册;真正安装点在 cbCfg(9bef60)构造路径,待重定位 |
| 0x605d22 | **更正**:非字符串,是 vcall thunk(`mov eax,[rcx]; jmp [rax+0xa8]`);门控 = slot 0xa8 背后实现 |
| key 格式 | 部分闭合:key = fmt 模板 `{} {}'{}\"`(0x3faa680 活内存实证,长形态合法串)+ 后缀字面量(58e72 = 纯拼接);97b0f 的 3 个 fmt 实参未定 |
| cookie→msgUid | 收窄:响应回带 msgUid 串已实证;请求侧关联一次受控正例可定 |

### 离线:N3 native_session.rs 会话层交付(fake-ABI 合同)

新增 `crates/caligo-bridge/src/native_session.rs`(纯调度,零 FFI;+模块注册):

- `AttachProof.validate()`:锚点多候选/空服务/缺基线拒绝;`attach` = 门核对 → ops 零副作用探针 → Ready。门未过 = 零原生调用(T12)。
- `enqueue_send` 顺序合同:seq 先持久化(T09)→ cookie 预登记(§5-2)→ ops 提交;`SendAcceptance` 四分类(Dispatched/RefusedPreCall/NotReady/AllocationFailed)+ NotAttached —— 受理≠派发≠原生开始≠业务成功(§5-3)。
- `NativeOps` trait(probe_ready/submit):真实地址仅在实现内触接,N3 接线点;fake 合同测试 7 用例(门拒绝零调用、派发顺序、inline 回调、PreCall/NotReady/分配失败三分类、失效排空、关闭如实 Clean/Quarantined + 关闭后零受理)。

### 第二轮验收命令结果(2026-10-09)

| 命令 | 结果 |
|---|---|
| `cargo test --workspace --features caligo-core/research --locked --offline` | **180 passed / 0 failed** |
| `cargo test --workspace --locked --offline` | **159 passed / 0 failed** |

声明:本轮现场部分仅 observe-msf(外部只读)+ 静态字符串 RPM 读取,零注入;
native_session 为离线合同,未接真实地址,不构成常驻准入。

## 2026-10-10:接收捕获专项(g2x r1–r5,实例 27108)+ SnowLuma 对照接入

### 捕获工具升级(3b2a4c7)

- g2 头捕获 0x40 → 0x200(64 qword 连续前缀,部分可读保留);Type S 头池快照
  4KB(回调内,槽位 **q6/q7** —— 实例 27108 实测布局 [SSO q0/q1, 0, permsg32 q3,
  0x31 q4, 0x25 q5, pool q6, pool q7];早前 [5,6] 系 r4 jsonl 打包偏移误读);
  窗口上限 120s → 300s。
- **sso_like 现场修正**:删除 `(q0>>48)==0` 条款 —— 10 位账号 SSO 恰好填满
  首 qword,高位非零是常态;窗口 1 池快照 0/22 触发即此 bug(现场数据抓到
  代码缺陷);native_receive::classify 同步修正。

### 现场窗口(r1–r5,全部 restore 对称、零不可读、QQ 存活)

| 窗口 | 时长 | 结果 |
|---|---|---|
| r1 | 120s | 42 头全完整(got=64) |
| r2 | 60s | 23 头;唯一 Type S 的池快照含 `trpc.msg.olpush.OlPushService.MsgPush` + u_ uid |
| r3 | 300s | 75 头 + 38 池;18 个 MsgPush 池,身份日志碎片(`id:7694798992310132705,seq:`、`peer_uid:1124196924`) |
| r4 | 300s | 70 头 + 26 池 |
| r5 | 300s | 91 头 + 38 池;与 SnowLuma 事件流并行录制 |

### 私聊谜题裁决(标记消息三轮未达的根因)

- r2/r4 的 N3R6-G1、N3R8-G1(群)与 N3R8-SELF1(自发)在 bot 进程内存命中
  (消息存储记录:sender uid + 群码 + 正文);**N3R5/N3R6/N3R8 全部私聊标记
  在堆段扫描中零命中**。
- SnowLuma `get_friend_msg_history`(3089665724):最后一条是 10-09 晚验收
  消息;**10-10 全天主→bot 私聊为零**(10-09 的 CAPTURETEST123 正常到达)。
- 事实链:主账号在线(群成员表 last_sent_time 落在 r4 窗口内)+ 群消息送达 +
  好友关系在 ⇒ **主→bot 私聊通道当前单向异常**(主账号重登后发生),待用户
  在主账号侧核实发送状态。注:主账号 10-09 曾异常离线。

### SnowLuma 对照接入(用户建议;零人工流量)

- 实例 = `NapCatQQ Desktop\components\SnowLuma
ode.exe index.mjs`(PID 35896),
  **挂载账号 = 1694717255 = 本项目 bot 实例 27108**(bot.json `backend_type:
  snowluma`);OneBot v11:HTTP 3000(无 token)、WS 3001(无 token)。
- 真值表:`gt-friends.json`(好友含主账号 〈Ain〉3089665724)、
  `gt-group-members.json`(群 263402786 全 384 成员,uin 键控)。
- **关联链路打通**(r5):SnowLuma 事件(seq=397856,user=3089665724,
  text)→ 堆扫描命中消息存储记录(sender uid `u_KxYzu_qQPbCC-Ca8mLteVA` +
  正文,长度前缀字符串区 = 0xC513 族 TLV)→ QQ 自身日志行
  `[roam_872_263402786]OnQueryMessageList, chat_type=2 peer_uid:263402786,
  begin_seq:397856` —— **事件 seq ↔ 原生日志 seq ↔ 存储记录三方一致**。
- 接收映射的剩余工作:TLV 记录内 time/msgId/sender 字段的精确键与偏移
  (方法已证,数据在 r5 归档 + 后续窗口可续采);私聊映射待私聊通道恢复。

证据:`E:/stella/_reference/caligo-n3-resident-20261009/`(observe t1–t4、
g2x-r1..r5 capture/report、snowluma-events.jsonl、gt-*.json、
read_resp_consumer_strings.py、record_snowluma_events.py)。
## 2026-10-10(续):r6 窗口 + 自产流量真值实验 + 属性表静态推进

### r6 窗口(300s)
62 头 + 9 池,restore 对称,QQ 存活;期间环境群消息 0 条(安静时段)。

### N3R10 自产流量实验(零人工;SnowLuma API = 发送器 + 真值源)
- `send_group_msg` ×3(N3R10-A/B/C,bot 自发)→ 每条响应含 message_id;
  `get_msg` 补全真值(time/seq/real_id)。
- 存储命中:A@0x1d76e9322de、B@0x1d76eb4d64a(C 未中,区复用竞态);
  **time/seq/msgId 无二进制形态于字符串区 ±16KB** —— 数字字段在二进制
  记录结构(CPBMessageOpti),串区只存串。
- seq 以 ASCII 出现于 QQ 日志行 `[roam_872_263402786]OnQueryMessageList,
  chat_type=2 peer_uid:263402786, begin_seq:397856…`(r5 事件 seq=397856
  三方一致再证);sender = 自 uid u_89Q8…(text-0x44 处,自发消息 sender=自己)。

### CPBMessageOpti 实例定位(布局未解)
- 全堆扫 vptr(base+0x42615c8):**56 实例**;MsgRecord vptr(base+0x3f51a78)
  4 实例(this+0x18 = 属性键包装对象,非 CPB 本体,其中两个共享同一 props)。
- +8 entries / +0x10 u32 count / +0x14 cap 读数在部分实例成立,但 12B 条目
  跨距解析出现串内容混入(key 位见 "u_KX" ASCII)→ **条目精确布局需解
  CPBMessageOpti 构造器**(下一轮 Ghidra 定向任务;证据树中 218bba0 是 wire
  反序列化器,非本构造器)。

### 静态:95d4e6/73bd6c 调用方(PE E8 扫描,详见 scope doc 增补)
- 95d4e6:9 个发送路径调用方(RVA 0x255e962–0x26537ec,BaseMsgMgr 族)。
- 73bd6c:两个薄包装,再向上为 vtable 间接 → 需对包装区间完整反编译。
- 9bef60 语义更正:给 record 设特性键(vtbl+0x38/+0x98/+0xb0),非 cbCfg 生产者。
- **池观测替代路径**:发送后扫池可读响应层日志(诊断用),不阻塞于消费者安装。

本轮零新增注入(仅 r6 一次捕获窗口,按双门纪律);证据均在本轮 reference 目录。
## 2026-10-10(再续):二进制记录区定位 + 响应日志通道裁决

### N3R11/N3R12(零人工,API 自产流量)

- **二进制记录区已定位**(N3R11-X):time(u32 1791589640)与 seq(u32 397874)
  在 0x1d771f63c48/0x1d771f63c80 相邻出现;周围为重复条目族
  {key u32, index u32, value, owner-ptr 0x1d771a79590},含 null(0x80)/int
  等 tag 形态;同一结构族在 N3R12 的 seq 命中处复现(4 处)。
  **剩余:owner 容器链与条目 key 语义**(owner+0 处非 wrapper 内 vtable,
  需从"谁指向该数组"向上找容器对象——下一轮任务)。
- **响应日志通道裁决**(N3R12 发送后 0.5s 全堆扫):
  - seq u32 4 处命中(存储条目);mid u32 0(message_id 是 OneBot 派生值,
    不落原生内存);
  - "got empty response"/"[MSF] Unknow Error!"/retCode 串 **0 命中** ——
    成功路径无堆内响应日志(错误模板仅失败时物化);
  - "MessageSvc.PbSendMsg" 15 处(发送命令上下文常驻)。
  **结论:成功发送不可经池扫描观测;retCode 通道仍以消费者安装(或错误
  触发后的失败日志扫描)为路径** —— @ 拒绝场景属失败路径,错误日志物化
  可期(下一轮 @ 探针配合即时扫描验证)。
- CPBMessageOpti vptr 扫描计数修正:299 实例(随消息活动增长;此前 56)。
## 2026-10-10(四):通知头字段排除 + 记录数组定位

- **通知头 +0x10 ≠ seq**(负结果,有价值):r5 同窗对照,8 条事件的 seq 与
  30 个 Type S 头的 q3 零匹配;q3 值呈随机分布 = 每消息随机值/handle。
  通知头已证字段仍只有:自账号 SSO(+0x00)、每消息随机(+0x10)。
- **每消息记录数组定位**(N3R13-A/B/C 结构过滤扫描):0x1d76ecd1000–
  0x1d76ece4900 区,记录条目含 seq 两次(对应存储记录 divSeq 双份形态),
  相邻消息记录相距 0x30–0x150;真值:seq 397876/397877/397878,
  time 1791590675/76/77(SnowLuma get_msg)。
- 假阳性教训:naive seq 扫描会命中代码字节与 UTF-16 UI 缓存;结构签名
  (q[-2]=堆指针 + 高半字为零)可过滤但仍有同族多结构。
- 全区 106KB 已转储 `r13-record-array.bin`(含 A/B/C 完整记录),配合
  `r13-msgs.json` 真值可离线解剖;不再依赖活进程。
- 下一步(离线可做):并排比对 A/B/C 记录字节差分 → time/seq/sender 键位
  → owner 容器链 → native_receive.rs 落地。
### 转储离线解剖(首轮)

- time 的 u32 不在记录数组(ASCII 形式仅在日志区 0x1d76ecd328e);
- 0x1d76ece4000 区 = **24B 索引节点数组** `{record_ptr, msgSeq, msgSeq}`:
  节点 0x133f0 → 记录 0x1d76ece4420(seq 397877 双份),0x13420 → 下一记录
  (397876 双份),另见 r5 时代旧消息 397854 —— 按消息排序的内存索引链;
- 记录对象为链式结构(ptr→ptr),完整字段解码基于 `r13-record-array.bin`
  (106KB,含索引数组 + 记录对象 + 日志区三结构)+ `r13-msgs.json` 真值,
  离线可续,不依赖活进程。
## 2026-10-10(五):离线转储解剖 + CPB 构造器定位

### 转储解剖(r13-record-array.bin)
- "记录数组"实为**排序链表**:节点 = {next_ptr, msgSeq, msgSeq}(B 节点指向
  的对象内是 A 的 seq 与更旧消息;非节点→记录对象间接)。
- time/msgId/sender 均不在链表结构(time ASCII 仅日志区;bot_uid 3 处在
  日志/上下文区);真记录 = CPBMessageOpti。

### CPBMessageOpti 构造器定位(PE 字节级,wrapper.node)
- vtable 0x42615c8 的 lea 引用 8 处,集中在 RVA 0x21832d0–0x2185xxx;
- **构造器 = RVA 0x21856d0**,初始化模式(手读):
  alloc(0x10) 16B 对象 → alloc(0x20) 32B CPB 本体 {vtbl_A@0, vtbl_B@8,
  16B 载荷@0x10 清零} → alloc(0x28) 40B 包装 {vtbl_C@0, dword+8=0,
  dword+0x10=0, ptr+0x18 → CPB 本体};
- **偏移修正**:此前假设 count@+0x10 有误 —— 40B 包装的 dword@+8 = count、
  dword@+0xC = capacity(活内存 0x1000000001 = {1, 16} 吻合);
- 下一跳:对 0x21856d0 做一次 Ghidra 反编译(或续读后续字节)即得
  entries 插入逻辑与 key 语义 —— 接收映射最后一个缺口。
## 2026-10-10(六):属性访问器完全解码(capstone 反汇编)

### 访问器算法(0x1821878c0,slot2/6/7 thunk 的公共实现)
- thunk 形态:`add rcx,-8; mov r8b,<字段偏移>; jmp 公共`(key 从 edx 直通);
- **条目布局 = 12 字节 {key u32 @+0, tagged qword @+4}**(与台账 12B 一致,
  capstone 实证:`r11d = mid*4; r11d *= 3` = mid×12;二分查找按 key);
- entries_ptr = [this+0x10],count = [this+0x18](u32;this = 槽对象 − 8);
- tagged qword:高位为类型/字段 tag(读侧检查 bit58–61 ∈ 1–8 ∪ 10–11),
  值取 [entry+4] qword,写回语义 = 清 58–61 位 | (字段偏移 << 58);
- 活内存验证:CPB 实例 {vptr@0, entries@+8, count@+0x10 u32, 内联 UTF-8
  文本@+0x18..}(0x1d76e8bad28 含 CJK 消息文本);条目键族实测:
  **0xfa03–0xfa23(消息属性族)、0x108f6–0x108fd、0x3e8/0x3ea/0xea61**;
  类型实测:ptr 型(bit60=2,低 48 位堆指针)、empty(0x0C<<56)、值型。

### 待收口(单点)
- 已知消息(seq/time 真值在手)的 CPB 定位:文本串指针形态与假设不符
  (非 SSO 直指),改走 **索引链表节点 → 记录对象 → 对象内 props 指针** 路径
  (全部结构已解,只差最后一跳);key→字段语义表随后即可写死进
  native_receive.rs。
### N3R15 异常与扫描方法论限制(如实登记)

- N3R15-Q(seq=397880/time=1791591947):旧区间(0x1d70–0x1d80 亿段)裸扫描
  seq/time **双双零命中**(N3R11–N3R13 同法即时命中);扩展至 0x1da0 的
  8GB 扫描过慢已终止。
- 两个假设(未定证):①堆顶已增长越出固定区间(应改用 VirtualQuery 枚举
  提交区——此前 ctypes MBI 结构有对齐问题待修,参考 g2.rs 的 Rust 实现);
  ②新鲜记录区被快速复用,N3R13 的命中得益于"发送后秒级"时序。
- **方法论结论**:裸扫描依赖地址区间假设,不可靠;正确锚定路径 = 结构链
  (索引链表节点 → 记录对象 → props)或 observe 类工具的枚举式提交区扫描
  (Rust RPM-self 模式)。这两条都不依赖猜地址。
- 属性访问器解码(e4fbc16)不受影响;键语义表待用结构链采样补全。
## 2026-10-10(八):消息记录 CPB 定位成功 —— 键族浮出水面

### record-scan 工具(observe-msf 扩展,外部只读)
- `observe-msf --record-scan --pid N --out t.jsonl [--seq S --time T]`:
  VirtualQueryEx 枚举提交区 → CPB vptr(0x42615c8)2505 实例 → 按已解码
  布局(entries@+8/count@+0x10/12B 条目)全量输出条目表;0.4 秒完成;
  指针型值附 40B raw 预览。**Python 固定区间扫描的替代方案正式落地**。

### 键语义(活体实证,N3R16-R 消息)
- 三个 CPB 含 "N3R16-R" 文本(0x1d773cd41c8/0x1d773cf50a8/0x1d7720e74c8):
  - **key 0x9c54 = senderUid**(bot 自发 → 值指向自身 uid 串;
    "u_1918755f…" 前缀出现);
  - **key 0x9c55 = peerUid**(值 raw = `0a 09 "263402786"` 群码 protobuf 串;
    另一记录为 "950238216" = 私聊对端 UIN 串);
  - **key 0x9c41/0x9c43/0x9c5b/0x9c72** = 记录级整型(随机数/msgId 候选);
  - **key 0xafc9/0xafca/0xb02d/0xb02e/0xbfc5** = 元素族(0xafca=元素类型;
    0xb02d raw = `08 07` + 正文 —— 元素 blob 内嵌文本,与 r4 P5TEST1 同形态);
  - 台账旧读数 0x9c4a="chat_type" 需重验(本次值 4 但非 AI 会话)。
- seq 不在 CPB 条目(在索引节点 {next, seq, seq});**time 的键位仍未定位**
  (值/LE 字节/ASCII 均未命中)——与索引节点/记录级字段的关系是下一跳。

### 下一跳(单点,全部结构已解)
1. time 键位:同批多条消息的 CPB 0x9c72/0x9c5b 值与 get_msg time 差分;
2. seq ↔ CPB 绑定:索引节点 seq → 记录对象的链(节点 +0x18.. 零字段区
   或外部 map);
3. 键表写死 → native_receive.rs NormalizedNotice 字段落地。
## 2026-10-10(九):三消息差分 —— time/seq 键定位(局部语义)

### N3R17-K1/K2/K3(间隔 3s,Δtime=4/3、Δseq=1/1,真值在手)

15 键消息记录 CPB 跨消息对照(每消息恰一条,0x9c55=群码已验):

| key | K1 | K2 | K3 | Δ | 判定 |
|---|---|---|---|---|---|
| **0x9c72** | …5e4 | …5ec | …5f2 | **(8,6) = 2×Δtime** | 时间序数(斜率 2/秒) |
| **0x9c43** | …4e8 | …4ea | …4ec | **(2,2) = 2×Δseq** | seq 序数(斜率 2/条) |
| 0x9c5b | 0x1f666a44 | 同 | 同 | 0 | 会话标识(恒定) |
| 0xa2ef | …7ac | …7ae | …7b4 | (2,6) | 次级计数 |
| 0x9c54/0x9c55 | ptr | ptr | ptr | — | senderUid/peerUid(已证) |

### 交叉验证的诚实结果
- 3 点内线性关系成立(Δ 完全吻合),但 **N3R14 独立样本上偏移量漂移** ——
  0x9c72/0x9c43 = 本地基座 ×2 序数(会话内单调可排序),**非绝对
  time/seq**;绝对值需会话标定(一条关联消息定基座)或改用
  SnowLuma 事件真值 + 序数排序的组合。
- 70 键 CPB(0x1d7746123b0/0x1d774626410,内容互同)= **会话级缓存**
  (含 K2/K3 双文本、0x9c54/0x9c55、0xa095/0xa0b0=同 0x9c72 值)。
- 0x9c4a=4 语义存疑(AI 会话语义不成立),待更多样本。

### 工程结论(接收映射的最终形态)
native_receive.rs 的落地不再需要更多逆向:
- sender/peer/正文 = CPB 0x9c54/0x9c55/元素 blob(键已证);
- seq = 索引节点(已证);time = 0x9c72 序数 + 会话标定(或事件真值);
- 采样 = record-scan(已交付)。
下一轮:Rust 读取器写入 native_receive.rs + N3R13/14/17 归档验证。
## 2026-10-10(十):CPB 读取器落地 native_receive.rs —— 接收映射闭环

- 新增 `record_keys`(0x9c54 senderUid / 0x9c55 peerUid / 0x9c43 seq 序数×2 /
  0x9c72 time 序数×2 / 0x9c5b 会话标识 / 正文探测序 [0xb02d, 0xa2e4, 0xa2e5])
  与 `MemoryReader` trait + `read_record_fields(rd, cpb_obj)`:
  按 12B 条目布局提取 [`RecordFields`] = {sender_uid, peer_uid, text,
  seq_ordinal(原始 ×2 形态), time_ordinal(原始 ×2 形态), session_id};
  不可读/超界 → 对应字段 None(不猜)。序数绝对化 = 上层用关联消息标定基座
  (策略在 N3 接线时定:标定基座 vs 事件真值配对)。
- 归档验证测试:N3R17-K3 实测条目表(15 键,台账九)+ 合成串缓冲 →
  sender/peer TLV/正文 blob/双序数/会话标识全部提取正确;超界 count → 全 None。
- 验收:research 182/0;default 重跑 161/0(首跑出现一次性测试进程崩溃,
  与台账已登记现象同型,重跑全绿;新模块纯安全代码)。
- **接收映射(逆向 + 读取器)闭环**。剩余工程:把 MemoryReader 接到常驻
  路径(N3),NormalizedNotice 字段填充 + time 基座标定策略定稿。
## 2026-10-10(十一):N3 接线预备件落地

- native_receive.rs 新增:**RpmSelfReader**(进程内 RPM-self MemoryReader,
  内核校验读,坏页 false)、**scan_cpb_in_region**(区域缓冲参数化的 CPB
  vptr 扫描,可测试)、**collect_record_fields**(有界收集,无 sender/正文/
  peer 的非消息属性表跳过);time 标定策略成文(基座标定或事件配对,
  platform_time 不确定期间如实 None)。
- 测试 +1:合成区域扫描 → 实例定位 → 记录收集全链(FakeMem);
  跳过无 sender 实例的合同。
- 事故如实登记:一次 python heredoc 转义把字面 NUL 字节写入源码
  (grep 二进制化/Edit 拒读暴露);已替换为 ` ` 转义并全文件验证无 NUL。
- 验收:bridge 52 lib 全绿(+1);workspace research **183/0**。
- 剩余(N3 现场):g2 通知头 → 索引链/CPB 的实时行走接入捕获处理器、
  NormalizedNotice 填充、time 基座标定执行 —— 均为已解结构的接线工作。
## 2026-10-10(十二):重大修正 —— 序数即绝对值,标定策略取消

### r7 链接窗口 + before/after 差分(N3R18-ONE 单消息)
- 基线 110 条消息记录 CPB → 窗内单发 N3R18-ONE(seq=398227,time=1791619427)
  → after 扫描 281 条含 0x9c43 的 CPB(会话活跃,多消息进出);
- **差分列直接命中真值:`0x9c43/2 = 398227 = seq`,`0x9c72/2 = 1791619427 =
  平台时间`,逐秒精确**;另有 398226/398229 等多条消息同样精确吻合。

### 修正(算术误读的更正)
- 2026-10-10 九的"局部基座 ×2 / 偏移漂移"结论**作废** —— 复查系 0xc24ec
  的十六进制算术错误(795,884 被误算 795,432);
- **定案:0x9c43 = 2×seq、0x9c72 = 2×平台时间(Unix 秒),/2 直接还原,
  无需任何基座标定**;
- r17 fixture 的"真值"注释错标为 N3R13-C 的值(实际表属 N3R17-K3)——
  修正后 K3 seq=397942/time=1791607545 与解码输出**逐字节自洽**。

### 代码落地
- native_receive.rs:字段更名 `seq_ordinal/time_ordinal` → **`seq` /
  `platform_time`**(绝对值,`/2` 还原);键名 `SEQ_x2/TIME_x2`;
  time 标定策略段落撤销(不再需要)。
- 验收:bridge 52 lib 全绿;fixture 期望值改为 r17 真值后通过。

### 接收映射最终形态(定稿)
通知(捕获)→ 索引链/CPB 枚举 → 12B 条目(0x9c54 sender / 0x9c55 peer /
0x9c43 seq / 0x9c72 platform_time / 元素 blob 正文)→ NormalizedNotice/事件。
全部字段绝对值、零标定、零猜测。剩余 = N3 现场接线(捕获处理器行走)+ N4。
## 2026-10-10(十三):diff 式接收原语

- native_receive.rs 新增 **`SeenRecords::absorb`**:已见账本(键 = CPB 对象
  地址,值 = 最近 seq);吸收每批收集结果,产出新出现/seq 前进的记录 =
  新消息事件;seq 回退不回滚。测试:首批评见/前进产出/回退抑制 三合同。
- **常驻接收接线形态定稿**:通知处理器只做轻量信号(head 入环 + 置位;
  回调纪律不破),worker 线程触发 CPB 收集 → absorb 差分 → 新记录 →
  NormalizedNotice(platform_time = 0x9c72/2 绝对值)→ IPC 事件。
- 验收:bridge 53 lib 全绿。
- 剩余(N3 现场单窗):捕获处理器接线 + time 基座确认(零标定已定)+
  事件走 IPC → 之后 N4。
## 2026-10-10(终章):接收链现场端到端贯通 —— 全字段提取成功

### 接线实现(g2_listen_run 扩展)
- swap 前 CPB 基线收集(进程内枚举 + RpmSelfReader,4325 实例/53 记录/**88ms**)
  → SeenRecords 首批评见;restore 后 fresh 差分(4815 实例/**77ms**);
- 新记录全字段写入报告(kind=record 行)。

### 串格式修正(r8/r9/r10 三轮现场迭代)
- **senderUid 目标 = {X u8, len u8, ascii}**(19 18 "u_89Q8…",len 在 buf[1];
  兼容 {len u8, chars} 单字节头)——r8 空 sender 的根因;
- **正文 blob = {type u8, len u8, utf8}**(0xa2e5:09 08 "N3R17-K2";
  0xb02d:08 07 "N3R16-R" —— len 恒在 buf[1])——r8 空 text 的根因;
- fixture 同步(合成串带长度头)。

### r10 现场终验(零人工;SnowLuma API 单发)
```
{"sender":"u_89Q8KaNHHFMPj7o967xJGg","peer":"263402786",
 "text":"N3R21-DONE","seq":398263,"time":1791621805}
```
与 get_msg 真值**逐项精确一致**(sender=自发 bot uid ✓ peer=群码 ✓
正文 ✓ seq ✓ 平台时间秒级 ✓)。ambient 消息(私聊 peer=950238216 等)
同步正确提取。restore 对称、零不可读、QQ 存活。

### 状态
**接收路径现场端到端贯通。** 剩余:N3 常驻化(宿主合法初始化 + IPC 事件)
与 N4 OneBot 动作层 —— 均为已验证部件的组装,不再有未知结构。
## 2026-10-10(十四):N4 OneBot V11 动作层第一批落地

- 新增 caligo-core `onebot.rs`:动作解析(`parse_action`)+ 响应框架
  (`ActionResponse`:status/retcode/data/echo;echo 任意 JSON 原样返回)+
  第一批动作 **send_group_msg / send_private_msg / get_msg / get_login_info /
  get_version_info / get_status**;
- 语义边界按计划 N4:未支持动作 retcode 1404 显式失败;message 段(字符串/
  标准数组)解析,未支持段(image 等)显式拒绝不静默;**受理 ≠ 送达 ——
  发送受理返回 status=async,不伪造 ok/message_id**;get_status 不固定
  online=true(分解字段随 N3 接线交付);
- 解耦:`SendSink` trait(N3 NativeSession 实现,内部走 qq_wire 已证形状)
  与 `StoreReader` trait(消息存储实现)—— transport(HTTP/WS)外层共用,
  不复制 native 逻辑;
- 测试 6 项:async 受理 + echo 透传 + 载荷校验;段数组解析 + image 拒绝;
  get_msg 命中/缺失;未支持/缺参/未就绪三分类;login/version 形状。
- 验收:workspace research **189/0**。
- 剩余(N4 后续):sink 接 N3 NativeSession 真实现、store 接消息存储、
  transport(HTTP/WS)与事件 fanout。
## 2026-10-10(十六):N4 组装三件套(离线)

- **message_store.rs**(caligo-core):账号域消息存储 —— 持久本地整数
  message_id(单调分配,不由 hash/request_id 冒充)、(peer, seq) 幂等去重、
  同 seq 跨 peer 各自成条;StoreReader 实现(get_msg 服务 OneBot);
  journal 持久化挂接在 N4 后续。
- **native_send_sink.rs**(caligo-bridge):`SessionSendSink` —— SendPayload
  → qq_wire::send 已证形状 → NativeSession::enqueue_send;cookie = rdtsc
  派生新鲜 16B(零值由 register 拒绝);client_seq=0(服务器不校验,r18
  实证)、platform_ts = 本机秒;受理分类原样映射(async/未就绪/拒绝);
  私聊发送显式拒绝(uin→uid 解析属 N3 接线)。
- **SendSink/StoreReader/SendPayload/extract_text 下沉 caligo-model**
  (onebot_traits;core 与 bridge 共用,消解 core→bridge 依赖环)。
- 验收:workspace research **193/0**,default **172/0**。
- 剩余(N4):HTTP/WS transport(std 可实现,零新依赖)+ 事件 fanout +
  journal 持久化;N3 现场(宿主合法初始化)。
## 2026-10-10(十七):HTTP transport(std 零依赖)

- 新增 caligo-core `onebot_http.rs`:POST JSON 动作端点 —— std TcpListener
  实现,零新依赖;请求体上限 1 MiB(超限 413);仅 POST(405);非 UTF-8
  400;一请求一响应(Connection: close);动作处理函数注入(transport 与
  动作层解耦);accept 循环支持 stop 条件(单线程串行,OneBot HTTP 动作
  天然串行)。
- 测试 2 项(真实 TCP 回环):get_version_info roundtrip(echo 透传);
  非法 JSON 400(1440)+ GET 405。
- 验收:workspace research **195/0**。
- 剩余(N4):事件 fanout(WS 推送/HTTP POST 上报)、journal 持久化、
  鉴权(access token);N3 现场(宿主合法初始化)。
## 2026-10-10(十八):事件 fanout + access token 鉴权

- 新增 caligo-core `onebot_events.rs`:**EventBus** 事件 fanout —— 每订阅者
  有界 sync_channel(256 条,独立背压);发布 try_send,满则该订阅者丢弃
  本条并计数(不阻塞发布者/不阻塞 QQ 线程);断开订阅者自动注销;
  丢弃数累计对诊断可见。WS 推送与 HTTP POST 上报共用同一 fanout。
- onebot_http.rs:**access token 鉴权**(Authorization: Bearer <token> 或
  ?access_token=;token=None = 未启用放行;无效 403/retcode 1403)。
  serve_one/serve_loop 签名相应扩展。
- 测试 +3(全订阅送达/慢订阅者丢弃计数/断开自动注销;token 通过与 403)。
- 验收:workspace research **197/0**,default **176/0**。
- **N4 离线可交付部分至此完成**:动作层 + 消息存储 + 发送适配器 +
  HTTP transport + 事件 fanout + 鉴权。剩余均需现场:
  ① N3 宿主合法初始化(合法注入替代路线,需设计审查);
  ② sink/store 接线上述组件到活实例;
  ③ 事件从接收链到 fanout 的贯通。
## 2026-10-10(十九):N3 现场验证代码交付(g3_ops + g3_session)

- **g3_ops.rs**:G3NativeOps 实现 NativeOps trait —— 完整 g3 FFI 序列
  (getter 租约→锚点→transport→QQ new→self str→SendSSORequestInternal)
  封装为 probe_ready + submit;研究构建专用。
- **g3_session.rs**:N3 会话现场验证入口 `caligo_g3_session_run` ——
  probe_ready → NativeSession.attach → CPB 基线(进程内 88ms)→
  enqueue_send(已证形状)→ 等待 → fresh 差分 → 全字段 JSONL 报告。
- lib.rs 导出 `caligo_g3_session_run`;bridge 53 lib 全绿。
- **待完成**:CLI inject 命令需 --g3-session flag 以调用新导出(目前
  只支持 --g2-run-report);下一步即加此 flag 并注入活实例验证。
## 2026-10-10(二十):N3 会话现场端到端贯通 —— CLI --g3-session + 全字段验证通过

### CLI 接线(本轮代码)
- inject 新增 `--g3-session-report/--g3-session-group/--g3-session-text/
  --g3-session-ms`(成对校验 + 群号必填);winutil 远线程块镜像 g3_send
  (G3SessionCtx 40B = wrapper_base/report_path/group_code/text_ptr/observe_ms;
  等待 = observe_ms + 60s 宽限);`InjectOutcome.g3_session_exit_code`;
  CLI 注入完成后打印退出码解码并直接 dump JSONL 报告;qqentry 三处调用点补参。

### 现场 r1 → r2(一轮合同修正)
- **r1**:g3_session 传 `service_addr: 0`,被 `AttachProof::validate` 拒
  (NullService)——离线合同(空服务=未就绪,拒绝附加)与 g3 路线错配:
  g3 submit 每次自行 getter 租约,无独立 service 指针可传。
- **修正(诚实证据原则,零合同改动)**:G3NativeOps 新增 `probe_service()`
  —— 把 probe_ready 的 getter 租约 + 锚点核对产物(**真实服务对象地址**)
  返回给调用方,作为 AttachProof.service_addr 登记;submit 仍每次自行租约,
  该地址仅作证据。bridge 81 测试全绿;workspace research **197/0**。
- **r2**:svc=**0x1d76eb68000**(与 2026-10-09 t1 observe 报告的 MSFService
  对象地址逐位一致);attach → CPB 基线(cpb=7497/records=217,~465ms)→
  enqueue_send 派发(seq=1,body=165B,"N3R23-NATIVE")。

### r2 现场终验(30s 窗;原生 + API 双发对照;零人工)
| 字段 | N3R23-NATIVE(g3 原生发送) | N3R23-SEND(SnowLuma API 发送) |
|---|---|---|
| seq | 399017 ✓ | 399018 ✓ |
| time | 1791639117 ✓ | 1791639122 ✓ |
| sender | u_89Q8…(bot uid)✓ | 同 ✓ |
| peer | 263402786(群码)✓ | 同 ✓ |
| text | ✓ | ✓ |

与 get_msg / get_group_msg_history 真值**逐项精确一致**;fresh=28(含狼人杀
群 1124196924、私聊 950238216 ambient 全字段正确;部分半成品 CPB 对象按
既有形状作噪声记录,seq 键控完整记录为权威)。`n3_done ok=true`,
exit_code=0,QQ 存活(27108 起始时间不变)。
**服务器分配 seq 399017/399018 连号** —— N4 wire 形状(client_seq=0)现场
实证:发送真实到达服务器并回流 CPB。

### 状态
**N3 会话现场验证入口闭环**(attach→send→差分→全字段);CLI 注入 →
`caligo_g3_session_run` 链路可用。证据:`E:/stella/_reference/
caligo-n3-session-20261010/`(r1/r2 capture+report,g3s1/g3s2 DLL)。
N3 常驻化剩余:宿主合法初始化 + IPC 事件交付 + SeenRecords 移入 worker
(均为已验证部件组装,无未知结构)。
## 2026-10-10(二十一):N3 常驻 native 后端现场接线 —— 派发/接收/事件/IPC/关闭 全链贯通

### 离线交付
- **native_resident.rs**(bridge,research 门控):
  - `NativeHostAdapter`:owner = 泵线程自声明;env = 锚点探针逐次重查;
    current_context 恒可(g3 FFI 无 JS 上下文前提,远线程现场实证);
  - 发送 = SendText → qq_wire 已证形状(群)→ `enqueue_send` → SendFired →
    **CPB 回流匹配**(EchoMatcher:peer+text+时间容差 120s;时间缺席的
    半成品记录不确认)→ complete_sends;私聊显式拒绝(native code 11);
  - 接收 = worker CPB 差分轮询(默认 1s)——ledger(十三)定稿形态的
    **严格子集(纯轮询)**,通知 swap 常驻化留作独立增量(需生命周期评审);
  - kind 零猜测:仅认部署配置群号表(→2 群),否则 0=unknown;
  - close 诚实分类(r2 现场 unknown=1 如实上送,零伪造 Clean)。
- daemon_client:`self_uid` 双域 direction 判别;kind 映射 2=群/1=私聊/_=unknown。
- lib.rs 导出 `caligo_native_resident_run`;CLI `--native-resident-*`(winutil
  80B ctx:wrapper/report/pipe/token/account/baseline/gen/groups/self_uid/poll/observe);
  **caligod 零改动**(bridge 管道/actor/journal 直接复用)。
- 验收:workspace research **200/0**,default **176/0**(+3:EchoMatcher 匹配/
  时间容差/过期清理;kind 白名单)。

### 现场 r1/r2(PID 27108 双查通过;caligod --expect-pid 27108)
- **r1 教训**:长管道名(`\.\pipe\…`)经 bash→CLI→远线程链路被破坏 →
  worker connects=0;现场参数一律用**短名**(daemon-client/daemon 两侧同
  规则自动补根)。
- **r2(短名)全链贯通**:
  - daemon-control send → actor 受理(Queued)→ dispatch_out → Dispatch →
    resident.submit(Accepted)→ NativeStarted 上报 → pump drain →
    enqueue_send 派发;
  - 接收 worker 120s:**205 事件**入列(partial=112 半成品如实计数)→
    IPC v3 上行 **events_sent=884 / acked=205 / gap_dropped=0**;
    事件含全字段(peer/sender uid/text/seq/time;direction;
    kind=group 正确;kind=unknown 被 daemon `caligo_target` 强转为 private
    —— core Target 类型只有 group|private,扩 Unknown 变体列 TODO);
  - close:cancelled=0 **unknown=1**(发送回流未至,如实 Unknown,不伪造)。
- **原生发送回归(独立发现,非本轮引入)**:N3R24-RESIDENT(常驻)与
  N3R24-G3PROBE(g3_session 对照)均"派发 FFI 成功但零服务端效果"(无 CPB
  回流、群历史无消息);对照 **API 发送(NapCat/QQ 自有栈)秒达 seq=399386**。
  本路线代码与 21:45 N3R23 成功轮逐字节相同(g3_ops 此后零改动)→ 判定
  **目标实例内部发送服务状态漂移**(legacy 732A80 路径依赖状态衰减;
  接收全程正常流动)。私聊探针(3089665724)FFI 完成但对端通道断无法验证。
  待办:发送后即时响应日志扫描诊断("got empty response" 通道);
  实例按 recovery-notes 恢复退出/重启后复测。
- **现场缺陷(既有代码)**:daemon_client session_loop 的未确认窗口重放
  位于循环体内(注释称"恰一处重放"实为每轮重放)→ ack 滞后时冗余帧
  (884 sent / 205 acked);daemon 按 seq 去重正确性无损,效率缺陷
  TODO:重放移至 Hello 后仅新连接执行。

### 状态
**N3 常驻 native 后端首轮现场贯通**:IPC v3 + actor/journal(core 零改动)+
常驻状态机 + CPB 差分接收 + 事件上行 + 诚实关闭,全部 field-proven。
剩余:① 发送服务漂移调查(独立);② notify 信号常驻化(即时性增量);
③ daemon Target Unknown 变体;④ 宿主合法初始化路线(设计审查)。
证据:`E:/stella/_reference/caligo-n3res-20261010/`(r1/r2 capture、
g3probe/g3send 探针、caligod 日志、journal)。
## 2026-10-10(二十二):发送服务漂移调查 —— 排除表定案,双假设待决

零注入为主(外部 RPM/observe/静态反汇编;探针发送均为既定测试通道)。

### 排除表(全部实证)
| 假设 | 证据 | 结论 |
|---|---|---|
| transport 轮换 | +0x60=0x1d76e84da30 与 t1 冻结逐位同;vptr 0x41403B8/slot7 0x1B4E4EC ✓ | 排除 |
| service 对象更换 | obj=0x1d76eb68000 同址,锚点 vptr ✓ | 排除 |
| MSFCoreService 相关 | t1(正常时)/t4/t5 三时点均"未构造" | 排除 |
| TCP 重连/会话轮换 | msf 连接 20:45:55 建立,**横跨**成功(21:45)与失败(22:16)窗口 | 排除 |
| 账号被限流 | API 发送全程秒达(N3R24-API-PROBE seq 399386;N3R25-API-CAL seq 399582 get_msg 实证) | 排除 |
| 本轮代码回归 | g3_ops 自 N3R23 成功后零改动(逐字节同) | 排除 |
| 客户端滞留队列(可观测) | 请求体拷贝成功/失败**都**滞留(成功者含 store 记录拷贝,失败者仅请求体);cb 引用两者均已清理 | 排除 |
| transport/service/subobj/metrics 现场状态 | 原生发送窗与对照窗 0 差分;**API 发送亦 0 差分** → 这些对象不承载活发送态 | 方法到顶 |

### 确立的正面事实
- 732A80 唯一早期门 = `cmp [rcx+60h],0`(transport 非空;我们通过);错误路径为
  软日志(类别对象 0x3F6DEA8,紧邻锚点 vtable)→ **写入加密 qqxlog**(无 key
  不可解;wrapper 模块日志不进内存日志环)。
- 内存签名:成功 = 请求体拷贝 + store 记录拷贝(peer/sender uid 长度前缀 TLV 区);
  失败 = 仅请求体拷贝。请求被"正常消费"但从未物化为消息。
- 发送队列日志串已定位:"OnSendMsgComplete, queue is empty, erase key:{}"
  (wrapper 0x4604C6D 邻域);key 模板 `{} {}'{}` @0x4CD284D(与响应消费者
  scope 的 0x3faa680 记录并存,两份拷贝)。
- **strong 计数泄漏**:MSFService ctrl_strong t1=84 → t4(14:42)=13529 →
  t5(22:47)=43494(~30/min,QQ 内部租约为主,我们份额小)——卫生问题,
  未证明与发送失效因果相关;getter 租约不释放是本路线已知债。

### 剩余双假设(按优先序)
- **H-A(服务端)**:21:45–22:16 间会话/票据 epoch 轮换,服务器开始静默丢弃
  legacy 形状 PbSendMsg(client_seq=0/rand=1/无 CH)——NapCat 全字段请求不受影响。
  判别:实例恢复重启后立即原生重试(成功 → 会话龄依赖确认);受控单变量
  现场(真实 client seq 取自 store/每消息 rand)。
- **H-B(客户端)**:732A80 提交路径 consult 仅由 JS 管线刷新的状态(路由表
  重建/令牌对象原位换),legacy 调用读到陈旧态静默 no-op。判别:732A80 →
  75CEC0(cmd 路由查找)→ 提交路径全程静态追踪,定位静默丢弃门后 RPM 实读。

### 工具/证据
- `sendtrace.py`(全堆模式扫描,4MB 步进/部分读容错)、`dumpmem.py`(定点
  qword 转储)入 `E:/stella/_reference/caligo-n3res-20261010/`;
  observe-27108-t5.json、tw-*/s*-t*/m* 差分快照、trace-probe capture 同目录。
- 现场纪律:本调查新增注入仅 4 次既定探针(N3R25-TRACE/TRACE2 + 2 次 API 校准),
  QQ 全程存活。
## 2026-10-10(二十三):H-A 判别实验轮 —— 客户端侧假设全灭,日志考古突破

实例:机器异常重启后全实例重启,bot = 最后启动实例(PID 38900,双查:observe
锚点命中 + svc+0x1f0 self=1694717255 + SnowLuma API 一致)。DLL n3res2
(新增 --g3-send-ch 形状二分通道)。

### 判别结果(全部一次实验一结论)
| 实验 | 结果 | 结论 |
|---|---|---|
| 新机器+新会话 legacy 基线(N3R27-LEGACY) | fail | 客户端态假设(H-B 余项)**全灭** |
| API 引子 → 立即原生探针(N3R26-PRIMER/PRIMED,旧实例) | 引子落地 seq 399711;探针 fail | H-C 发送态预热 **排除** |
| 真实 client_seq 399712 + 真实 random(N3R26-TRUESEQ) | fail | seq/rand 校验翻转 **排除** |
| ContentHead type=1(N3R27-CH1) | fail(CH 字节 `1206080110001800` 实证入 wire) | CH{1:1} 不是缺的形状 |
| ContentHead type=82(N3R27-CH82) | fail | CH{1:82} 同 |

### 日志考古(零注入;wrapper.node 静态 + 内存取证)
- **732A80 自带提交日志**:模板 `cmd={} trace={}` 的 xref 之一在 **0x732bda
  (732A80 体内)** —— 内存中捞到的运行时碎片 "MessageSvc.PbSendMsg
  trace=00-<uuid>-<uuid>-01"(W3C traceparent)即我们每次发送的提交日志;
  证明请求走的是**正常提交路径**。
- **响应判词模板**:`result|cmd={} error_code={} result_type={} error_msg={},
  trace={}` @RVA 0x45F8E67(.rdata;file→RVA delta 0x1800),唯一调用点
  **0x739d9b**(读 [rbp+40]=error_code、[rbp+44]=result_type)。邻族模板
  "...seq={} result={} result_type={} error_msg={}"(0x7ffdf1fe925c 族)。
- **零运行时实例**:全堆扫描 "result|cmd=" 仅命中静态模板 —— 我们的发送
  **从未走到响应判词行** = 无任何响应被处理(与回调不 fire、无 store 记录
  三角一致);API 发送后立即扫描同样无行(结果行生命周期短于全堆扫描耗时,
  事后取证不可行)。
- PE 感知 xref 扫描器(节表 file↔RVA 转换)已验证可用(本轮找到全部 5 处
  引用;naive 偏移扫描会因 .text/.rdata 映射差漏检)。

### 当前判定
客户端提交路径完好且与正常发送不可区分;服务器**从不回应** legacy 形状的
PbSendMsg(无响应、无错误、无回声),跨会话/跨机器重启稳定。形状侧已排除:
seq/random 值、CH{1:1}/CH{1:82}。主导假设收窄为:**服务器对旧形状的静默
丢弃点不看 CH 单键**——或需要现代请求的更多字段(msgRandom 语义、
RoutingHead 附加键、cmd 头部的会话协商态),或按 ticket 级能力位路由。

### 下一判别器(按优先序)
1. **响应消费者注册**(cbCfg/9bef60 重定位,已列 N3 工作项):拿到服务器
   真实响应或 -1 超时 —— 一锤定音"服务器无响应 vs 响应被客户端吞"。
2. **管理员 pktmon 抓包**(需用户授权终端):直接看包是否出网卡 +
   服务器是否回包(30 秒定案 wire 层)。
3. 现代形状字段二分:以 PRIMER 回流记录/挖掘的请求结构为参照,逐字段
   补齐(每次一探针)。
### 本轮探针清单(全部既定测试通道)
N3R27-LEGACY / N3R26-PRIMER(API)/ N3R26-PRIMED / N3R26-TRUESEQ /
N3R27-CH1 / N3R27-CH82 / N3R27-TRACELINE / N3R27-RESLT(API)。QQ 存活。
## 2026-10-11(二十四):发送漂移根因破案 —— 服务器 (uin, random) 近窗去重

### 破案链(00:40–01:00 三步)
1. **N3R28-POOL(rand=1)exit 0** → 疑似"恢复";**N3R28-CONFIRM(rand=1,2 分钟后)fail** → 闪断,排除"恢复"。
2. **N3R28-UNIQ(rand=449568253 新鲜,seq=0)→ 服务器历史实证送达(seq 399986)**;
   同窗 N3R28-CONFIRM(rand=1)**不在服务器历史**(count=60 全范围核对)。
3. 时间线全面吻合**(uin, random) 去重、窗口 ~1-3h**:
   - 21:45 N3R23(rand=1)首发过验 → 22:16–00:10 全部 rand=1 重发静默丢弃
     (撞 21:45 的缓存)→ "漂移开始";
   - 00:43 POOL(rand=1)距上次 rand=1 过验 ~3h,窗口过期 → 过验 → "恢复假象";
   - 00:45 CONFIRM(rand=1)撞 POOL 缓存 → 灭;
   - 00:48 UNIQ(新鲜 rand)→ 过验。
   - N3R26-TRUESEQ 败因:rand=116416255 是从日志抄的**旧值**(撞别的消息);
   - CH1/CH82 败因:--g3-send-rand 默认 42(历史实验用过)。
   - "客户端态/服务端会话态/预热"三假设全被"rand 常量"一行解释。

### 关键更正与教训
- **r18 结论修正**:服务器不校验会话 seq(client_seq=0 仍成立)——但
  **random 是去重键**,"服务器不校验"的概括漏掉了它。每次发送必须
  **每消息新鲜 random**。
- **@ 元素 r20–r28b 八轮"静默拒"存在混淆**:那些轮次的 random 同样复用
  (@ 形状结论存疑,需用新鲜 random 重测后才能定案;response-consumer-scope
  的实现仍必要 —— @ 的真实业务 retCode 依然不可见)。
- **NapCat 历史查询有缓存滞后**:窄 count 查询的"NONE"判两次误(POOL/
  UNIQ 都被误判过);验证必须用宽 count + 打印实际覆盖范围。
- 昨天的"服务器形状策略翻转"(§二十二 H-A)与今天的三假设排除表依然
  成立 —— 只是全部被 rand 复用这一个变量笼罩。

### 修复(field-proven)
- `caligo_model::qq_wire::send::fresh_random()`(rdtsc 熵源,|1 保奇);
- 四处换用:g3_session.rs encode_group / native_resident.rs / 
  native_send_sink.rs(旧 text.len()|1 同长度即撞)/ CLI --g3-send-rand
  默认 0(= bridge rdtsc 现场生成);
- **验证:N3R28-FIX1/FIX2 连续两条原生发送,服务器历史 seq 400048/400050,
  uid/time 与 CPB 记录逐位一致** —— client_seq=0 + fresh random 为定案形状。
- 工作树:bridge 56+ lib 测试全绿;n3res3 DLL。
## 2026-10-11(二十五):去重机制复现 + @ 重测 —— retCode 通道必要性坐实

### 超时哨兵通道裁决(弃)
- 复现实验:N3R29-TIMEOUT(新鲜 rand)送达(seq 400077);N3R29-DUP(复用
  UNIQ 的 rand 449568253)未达 —— **去重机制可按需复现**(对照对成立)。
- 但失败发送后 90s 全堆扫描:"got empty response"/"Unknow Error"/
  "result|cmd=" 零运行时实例 —— 判词串生命周期 < 90s(物化即释放),
  事后扫描抓不住。哨兵通道需要扫描与超时同时进行(当前工具不可行)。

### @ 重测(新鲜 rand,真实样本条件)
- 条件:@Stella 自 @(uid=u_89Q8… uin=1694717255,与 wirecap 真实样本同
  目标),r28 完整形状(elem37-125B + 元素9 + at-text attr13B + f12
  {3:2,4:uin,5:0,9:uid,11:0} + 元素16),rand=自动新鲜,306B 提交正常。
- **结果:服务器历史 80 条宽窗(400035..400116)覆盖发送时刻 —— 未送达。**
- **结论:@ 形状存在真实问题(非纯 rand 混淆)**;§二十四的"@ 结论存疑"
  收窄为:rand 混淆可能污染了部分轮次的**归因**,但 @ 被拒本身成立。
- 推论:retCode 可见性(响应消费者注册)是 @ 调试的必要前提 —— 下一
  工作块从 25757ea 族的 cbCfg 构造爬升继续(0x255e8c0 区已确认
  95d4e6(cookie, spA, spB, cbCfg) 调用形态,cbCfg 来自更上游)。

### 附带
- 服务器历史验证纪律再次成立:count=40 窗口边界会误判,宽 count + 打印
  覆盖范围(本轮 count=80 覆盖 400035..400116,FIX1/FIX2 同时在列作对照)。
- QQ 存活(38900);本轮探针:N3R29-TIMEOUT/DUP/AT-RETEST。
## 2026-10-11(二十六):实例事故 —— bot 实例(38900)死亡与自动恢复

### 事实(本地钟已校准;服务器时间戳为权威)
- 00:38:25 at91 原生 @ 探针(变体91 最小 @,234B)提交成功;
- 00:40:16 API @ 消息(NapCat 自有路径)送达(seq 400200)—— 最后一次成功操作;
- ~00:40:45 实例进程组消失(无 WER 崩溃记录,死亡方式未知);
- 00:40:51 NapCat Desktop 看门狗自动拉起新进程组(00:40:51–00:41:27),
  OneBot API 恢复,账号 1694717255 在线。** bot 自愈,无需人工干预。**

### 因果评估(如实:无法从外部定因)
- 死亡前 2 分钟:at91 原生 @ 形状 wire body 经 732A80(手工构造的非典型
  elem 结构)—— 候选因 A:异步任务延迟崩溃;
- 死亡前 30 秒:API @ 发送(QQ 自有代码)—— 候选因 B;
- 本轮累计 ~16 次原生发送 + 4 个新 DLL 注入 + getter 租约泄漏(strong
  计数持续膨胀)—— 候选因 C:资源累积;
- 无 WER dump、无 QQ 侧日志可读(加密 xlog)→ 因果不可定,按"未知"归档。

### 处置
- 现场操作立即停止(已停止);新实例(00:40:51 组)未做任何注入;
- 本轮现场账:@ 形状二分两轮(全形状/最小变体)均拒 + API @ 对照送达
  (结论保持:@ 形状问题在请求侧,需 retCode 通道);修复验证(FIX1/2)
  与去重复现(TIMEOUT/DUP)结论不受影响;
- 后续现场准入:待用户知情后决定;建议下实例降低探针密度(本轮 38900
  存活 1h 内承受 16 发送 + 4 注入,超出此前轮次密度)。
## 2026-10-11(二十七):事故定性修正 —— 服务器风控踢下线(用户目击)

### 目击信息(用户提供,事故当刻肉眼可观测)
1. @ 自己的消息成功发送(= API @,服务器 seq 400200);
2. **bot 账号立刻被 TX 踢出下线**,导致 QQ 实例死亡;
3. 用户手动重启实例。

### 定性修正
- 死亡原因 = **服务器侧强制踢下线(风控/会话失效类)**,非客户端崩溃
  —— 与"无 WER dump、进程组干净消失、踢出后正常退出"全部吻合。
  §二十六的候选因 A(异步崩溃)/C(资源累积)**作废**;候选因 B 方向修正
  为"风控系统动作"。
- 风控触发面(本轮产生的服务器可见异常信号,按可疑度排序):
  ① **故意重放 (seq, random) 对**(N3R29-DUP 等)—— 重放攻击特征,
     风控系统的教科书式触发器;**这是实验设计失误**(去重机制已经用
     自然对照对证明,不需要主动重放);
  ② 手工构造的非标准 @ 元素 wire body(r28 全形状 + 变体91)——
     非典型元素结构 = 机器人滥用特征;
  ③ 1h 内 16 次原生发送 + 4 次 DLL 注入的密度。
  确切触发点不可知(风控是累积信号 + 延迟动作);API @ 之后数秒踢出
  可能只是最后一个累积信号到位。

### 对 @ 之谜的再修正(重要)
- 原生 @ 两轮(AT-RETEST 00:31 / AT-MIN 00:38)被拒,现在有两个候选解释:
  (a) 请求侧 at-elem 形状问题(§二十五结论);(b) **账号已被风控盯上,
  非标准路径的 @ 发送被过滤**(而标准路径 API @ 在踢出前仍正常)。
  (b) 无法排除 —— **@ 形状结论再次不可靠**。
- 推论:**继续盲测 @ 探针有把账号推向再次踢出/限制的风险**(每次盲测
  都可能是新的风控信号)。@ 调试必须先有 retCode 通道(消费者注册)。

### 现场纪律 v2(即日生效)
1. **永远不做故意重放**(同 (seq,random) 复发);去重验证已有自然对照;
2. 发送速率 ≤ 数条/小时,random 全新鲜(已固化 fresh_random);
3. **原生 @ 探针冻结**,直至 retCode 通道可用或明确的清洁判别窗
   (全新登录 + 无异常史 + 单次轻探);
4. 注入密度:每实例每小时 ≤ 2 次;探针前确认账号在线状态;
5. 每次现场轮后核对实例存活与 OneBot 健康(事故早期发现)。

### 当前状态
- 现行实例 = 用户 07:08 手动重启(13192 组),OneBot 正常,群流量正常
  (夜间 400200→400703,~500 条/6.5h 活跃群);
- 账号功能正常,未见限制提示(用户侧如有踢出原因文案,后续补充);
- 下一块(纯离线,零账号风险):25757ea 族 cbCfg 构造静态爬升 → 消费者
  注册设计 → 一次性定向实验。
## 2026-10-11(二十八):消费者注册静态闭合 + 现场阻塞(提权实例)

### 静态闭合(纯离线;73bd6c 全逻辑 + 构造链)
- **73bd6c 全逻辑定案**(phaseAE2 反编译):param_1 = dispatch 对象
  {+8 logger, +0x18 inner(delegate 起点), +0x30 invoke, +0x38 ...};
  errCode 层级(transport 参数 → resp 空 = -1 → resp+0x40 → +0x60 →
  +0x64 → 0)无条件记日志(level 1/2)后,**门 3fbab2(inner+0x10 非空)
  → `(**(dispatch+0x30))(dispatch+0x18, 0x10, errCode, errStr, msgUid)`**。
- **dispatch 对象在 732A80 内部构造**(0x732e2e → 73bbf8):{fnA, dispfn}
  函数对写入 + 4d3588 池分配(分支仅池/堆,非消费者语义)+ 73c3a6 三
  qword 拷贝。**内嵌 delegate 即我们传入的 cb 块**:+0x18=cb 起点、
  +0x30=cb[2](invoke)——我们传 {0,0,空fn,空dtor},inner+0x10 空 →
  门永远关闭。**结论:传入真实 delegate 即可让响应回调到我们**。
- 25757ea 反编译补证:cbCfg 配置 {180000,1,180000,1,2,1} = **180s 超时**
  (解释 90s 扫描抓不到超时哨兵);cookie = 发送上下文 +0x20。
- 工具:PE 感知 xref 扫描 + .pdata 函数边界(52.5 万函数)+ LEA 函数地址
  扫描 —— 构造点 0x73bc1f/0x73bc3c 即由此定位。

### 实现(consumer_probe.rs;n3res5 DLL 就绪)
- ConsumerCtx(leaked 报告路径)+ consumer_invoke(RPM-self 安全读两侧
  libc++ 串;魔数自检 + delegate 多布局解析;布局误判不崩)+ dtor(no-op);
- CLI --g3-send-consumer 1 → cb = {ctx, flag, invoke, dtor};全测试绿。

### 现场阻塞(如实)
- 昨夜事故后 bot 由 **自动重启机制拉起(00:40:51 组)**——该 QQ(8112,
  wrapper@0x7FFDA4000000,msf IPv6 连接在位)**带提权令牌**:非提权
  Toolhelp 枚举失效(observe-msf 误报"无 wrapper",PowerShell 枚举可见)
  且 RPM 失败(err 299)→ 注入不可达。
- 另发现:QQ 更新包 9.9.35-52892 已下载未应用(版本适配注意);
  早上 7:08/7:14 手动重启组无 wrapper 无 msf 连接(非宿主)。
- **解锁条件(需用户操作)**:停止当前 bot(SnowLuma stop)→ 以非提权
  方式重启 bot(桌面正常启动)→ 新实例可注入后,单探针验证消费者通道
  (成功发送 → err_code=0 + msgUid;纪律 v2:不做故意重放,-1 超时路径
  不主动触发)。
## 2026-10-11(二十九):消费者实验 r1 —— delegate 被调用但契约错位,实例死亡(第二次)

### 事实
- 用户停 bot 并以正常方式重启 → 新实例定位(17200,bot 1694717255,RPM 可达;
  注意:昨日"8112 提权"诊断实为**十进制→hex 换算错误**,8112 当时可注入,
  观察工具的地址算错;已修正方法论:进制转换必须程序化)。
- 消费者探针(--g3-send-consumer 1,新鲜 rand,N3R30-CONSUMER):submit 时刻
  delegate 即被调用(同毫秒),参数形态 = `(stack_obj, 0, ptr, 0x10, ptr)`
  —— **管理器契约**(dest, op=0 克隆, src, size=0x10, ...),非响应回调;
  数秒后实例死亡(无自愈,OneBot 下线)。

### 契约破案(0x72D596 全反编译)
- 0x72D596 **不是空函数** = std::function 式**管理器**:`(dest=rcx, op=edx, ...)`,
  op 0/1 = 克隆/销毁(空实现向 dest 写 {manager, terminate} 对),op 2/3 =
  no-op;0x72D5C8 = "调用空 function 即 terminate"哨兵(int3)。
- cb 块 = 32B delegate,**布局 L1 = {storage0, storage1, manager, invoker}**;
  空 delegate = {0, 0, 管理器, terminate}。门 3fbab2 检查 storage/管理器槽
  非空;响应时经 dispatch+0x30(= cb[3] invoker)调用消费者,this =
  delegate 起点。
- **N3R30 崩溃根因**:我们把响应处理器放进了 manager 槽、no-op 放进
  invoker 槽 —— QQ 在请求建立时克隆 delegate 调 manager(op=0,要求向
  dest 复制),我们的函数做文件 I/O 不复制 → dest 残留垃圾 → 崩溃。
  此前所有成功发送(N3R23/FIX1/2)不崩 = storage {0,0} 门关,invoker
  从未被调 —— 全部自洽。

### 修正(consumer_probe.rs 重写;n3res6 就绪)
- cb = {storage0=ctx 指针, storage1=1, manager=consumer_manager,
  invoker=consumer_invoke};
- manager:op 0/1 → 向 dest 定长复制 4 qword(自身地址组 + 静态 ctx);
  其他 op no-op;零读零分配;
- invoker:ctx 从 this+0x10 取(魔数自检),字符串 RPM-self 读,JSONL 追加;
- 全测试绿。**字段重试待用户明确批准**(今日已两实例死亡;修正后的
  manager 语义与 L1 布局均有直接反汇编依据,但 3fce5e 门内部逻辑未全解)。
## 2026-10-11(三十):消费者实验 r2 —— 契约安全,响应路由缺口定位

### r2(修正版 delegate,n3res7,PID 25072 = 用户重启后的 bot 宿主)
- **零崩溃**(r1 的 manager 损坏问题修正生效);N3R31-CONSUMER 送达
  (服务器 seq 400714);**consumer 未被调用**(capture 无 consumer_result)。
- 结论:cb delegate 的 manager/invoker 契约处理正确(克隆 + 门查询安全),
  但**响应到 73bd6c 的路由需要上游注册** —— 732A80 内建的 dispatch 对象
  只是传递结构;消费者→请求的关联(按 cookie/msgUid)发生在更上层的
  发送路径(25757ea → 182578e18 → 732A80),裸调 732A80 绕过了它。
  与 scope 文档"cbCfg/9bef60 构造路径才是消费者安装点"的修正一致。

### r1 教训的完整闭环
- 0x72D596 = std::function 式管理器(op 0/1 = 克隆/销毁写 {manager,
  invoker} 对;op 2/3 = no-op);0x72D5C8 = 空调用 terminate 哨兵;
- 3fce5e 门 = 调 manager(NULL, op=4, NULL, 0, &out16, 0) 后查 out[0]
  非零 —— **门查询是 manager 的 op=4 契约**;
- 布局 L1({storage0, storage1, manager, invoker})下 r1 崩溃 =
  处理器误放 manager 槽 → 克隆损坏;r2 修正后零崩溃,契约处理验证通过。

### 方法论修正(重要)
- 昨日"8112 提权不可注入"诊断 = **十进制→hex 换算错误**(wrapper 实际
  base 0x7FFDEDA00000,当时按错误地址读 → 299 误判提权)。进制转换
  程序化;PowerShell Modules 枚举与 RPM 交叉验证。
- QQ 进程枚举时 PowerShell Modules 的"无 wrapper"需区分枚举失败与
  真 absence(本轮 25072 就在被漏掉的进程中)。

### 下一步(纯离线)
- 找 182578e18 的反编译/反汇编中的 registry insert(cookie → 消费者);
  或 95d4e6 的 9 个调用方函数中定位真正的注册调用;
- 拿到注册调用后:消费者实验 v3 = 裸调 732A80 前**手动执行注册调用**
  (cookie + 我们的 delegate)→ 响应路由闭环。
- 当前状态:25072 存活,OneBot 正常;n3res7 驻留其内(仅持 leaked ctx,
  无活动线程)。
## 2026-10-11(三十一):管理器调用序列完整观测 —— 门开、路由缺、实例存活

### v3 观测(N3R32/N3R33,n3res8/9,零崩溃)
- **N3R32/N3R33/N3R31 全部送达**(400714/400716/400717),实例 25072 存活
  (两次"死亡"均为瞬时查询误判——已修正验证方法:双查 + SnowLuma 日志);
- **管理器完整调用序列(首次实测)**:
  1. submit 时刻 **4 连 op=0 克隆**:栈上 3 跳(dest←src 链)+ 最终堆分配
     0x21ba1255a68 = 请求持久 delegate(每次写 {manager, invoker} 对);
  2. **+584ms op=4 门查询**(NULL,4,NULL,0,&out16,0 —— 与 3fce5e 签名
     逐位吻合):我们的 manager 写非零 → **门开**;
  3. 紧接 op=3(dest=堆 delegate,src=dest-0x10,size=0x10)—— 销毁/搬移,
     no-op 安全;
  4. **invoker 未被调用**(cb[3] 未触发)。
- 工具缺陷修复:v3 日志曾因 STATIC_CTX/STORAGE_CTX 双静态笔误失效
  (build_cb 存错静态)——n3res9 修复;线程交错写导致的 JSON 半行
  (append 无锁)——已知,后续加文件锁。

### 路由缺口的精确化
- 门(3fbab2)共 **6 个调用方**:[3fb914]/[54cdef]/[6a7d0c]/[739c66]/
  [73bd6c]/0x73ce0c —— op=4 查询来自其中之一,但 73bd6c 的门若跑过会有
  第二次 op=4 + invoker 调用(均无)→ **响应仍未到达 73bd6c**;
- op=4 的来源层待定位(六个门调用方逐个反汇编,看哪个操作 send-result
  上下文对象);73bd6c 的响应分发 = 需要 upstream 注册(cookie → 消费者,
  BaseMsgMgr token registry)——结论不变,证据更实。

### 状态
- 25072 存活、OneBot 正常;n3res7/8/9 三份 DLL 驻留(均无活动线程,
  leaked ctx 无害);三轮探针全部送达(修复版 delegate 完全安全);
- retCode 通道进展:delegate 契约 ✓(克隆/查询/销毁全安全)→ 门开 ✓ →
  剩:响应路由到 73bd6c 的 upstream 注册。
- 下一步:①六个门调用方反汇编定位 op=4 来源层;②182578e18 反编译找
  registry insert;③v4 = 手动注册 + 裸发(待布局确认后)。
## 2026-10-11(三十二):cb delegate 定案 —— 非响应消费者;retCode 通道 = token registry

### 决定性收敛(离线反汇编 + 三轮现场序列互证)
- 732A80 尾段(0x732e33+)继续组装 delegate 对象(写 0x20 标记等)——
  组装语义复杂,但结合三轮现场序列已可定案:
- **delegate 完整生命周期(实测)**:submit 时 4 连克隆(栈→堆持久副本)
  → 响应时 op=4 门查询(门开)→ op=3 销毁;**invoker 从未被调用**;
- 与 g3.rs 既有注释互证:"MSF 层回调本就被 b7f6f6 置空,结果路由走
  BaseMsgMgr token 注册表" —— **cb delegate 被当作普通 function 对象
  搬移/查询/销毁,但响应调用的实际路径已被 732A80 内部置空**;
- **定案:cb delegate ≠ 响应消费者。retCode 通道 = 向 BaseMsgMgr token
  registry 注册消费者**(token/cookie → 消费者对象),即 scope 文档的
  "消费者安装点重定位"任务本体 —— 不是 cb 块能替代的。

### 今日消费者战线的完整收获(诚实盘点)
1. **delegate 契约全解**(manager op 0/3/4 实测 + 0x72D596 反编译):
   op=0 克隆(写 {manager, invoker} 对)、op=4 门查询(写非零即开门)、
   op=3 销毁;布局/时序全部实测吻合;修正版 delegate 零崩溃
   (N3R31/32/33 三轮全送达,实例存活);
2. **排除法终局**:cb 块路线(64B 内嵌 delegate)被实测排除 ——
   这本身是有价值的负结果(.scope 文档的"实现路径建议 1-4"中
   "cb 即消费者"假设被否定,后续不再沿此路线投入);
3. **门/消费者机制全图**:73bd6c(响应分发)+ [739c66](结果处理器)
   双层,门 3fbab2/3fce5e 语义,6 个门调用方 —— retCode 最终流向
   token registry 注册的消费者;
4. 工具沉淀:pdata 函数边界(52.5 万函数)、PE 感知 xref、LEA 函数地址
   扫描、sendtrace/dumpmem、mgr_call 序列观测(栈上零分配 JSONL)。

### retCode 通道的剩余路径(下一工作块,纯离线起步)
- 定位 token registry:182578e18(25757ea 与 732A80 之间的中层)反编译,
  或 25757ea 族 9 个调用方中的注册调用;token = 发送探针时代已见的
  i32 token_out(0x20B 面);
- 注册调用定位后:v4 = 裸发 + 手动 registry 注册(cookie/token → 我们的
  消费者对象 {retCode 提取 + JSONL})→ 响应闭环;
- 该路径同时是 @ 重测的前提(需要 @ 的业务 retCode)。

### 现场状态
- 25072 存活,OneBot 正常;N3R31/32/33 全部送达;n3res7/8/9 驻留
  (无活动线程)。今日现场探针总计:发送 ~10 次、注入 6 次、零不可逆损伤
  (两次实例死亡分别由风控踢下线与 r1 契约错位导致,均已定案)。
## 2026-10-11(三十三):任务容器层定位 —— 消费者住在上游任务对象里

### 0x2578e18 全解(248B;25757ea 调用两次;Ghidra 地址 = 0x180000000 + RVA)
- 流程:容器单例(2b8674)→ 按请求对象查找(3fb8;未命中即退出)→
  取任务 shared_ptr(2b86d4 + 引用计数)→ cookie 经 9ccf34/2578f4c 包装
  → **4c7aa6(任务, arg2, arg3, …, cookie)提交**;
- **结构性定案:消费者住在任务对象里** —— 任务由 25757ea 上游的发送管线
  创建并放入容器(键 = 请求对象);响应到达后按任务分发到 73bd6c/
  [739c66](带 retCode 提取)。裸调 732A80 无任务、无容器条目 → 响应
  无路由 → retCode 不可达 —— 与三轮现场观测完全互证。

### 修正与方法论
- Ghidra 反编译函数名含 0x180000000 基址:FUN_182578e18 → RVA 0x2578e18
  (此前按字面地址找"not found"的误判);后续所有 phase 反编译地址
  一律减基址换算。

### retCode 通道剩余工作(v4 清单)
1. 任务对象布局(4c7aa6 首参 r14 的内部结构;消费者/分发对象在任务内
   的偏移);
2. 容器键语义(3fb8 的键 = 请求对象的什么;能否自造键插入);
3. 任务构造(上游管线如何建任务 + 塞消费者 —— 或找到独立的
   task+consumer 构造导出);
4. v4 = 构造任务(含我们的消费者)+ 容器插入 + 裸发 732A80 → 响应闭环。
5. 或者:评估直接 hook 任务创建点(25757ea 上游)的风险等级——
   属于"插桩"类,需按 slot7 教训评审。

### 状态
- 纯离线轮:零注入零发送;实例 25072 存活(上轮三 DLL 驻留无活动);
- 消费者/retCode 工作至此到达清晰边界:cb delegate 线路(排除)、
  响应分发层(全图)、任务容器层(定位)、剩余 = 任务构造与注册。
## 2026-10-11(三十四):响应链全图闭环 —— 路由缺口根因 = patch 错层;v4 = arg5 自带回调,零 patch

### 六门调用方反汇编完成(原"6 个"实为发送区限定;全量 xref ~140 处,门 = 通用 is-invocable 工具)
- **[0x739c66](0x394B,result 处理器)全解**:签名 (this{cb@+0x10,writer=[+0]}, &ptr, &i32, box-move)。
  result 记录 0x72a504(i32,*ptr) → {errCode@+0,resultType@+4,string@+8,f20,f30};
  任务态字段 +0x20(串)/+0x60/+0x64(dword)/+0x80/+0xA8(byte);+0x90 向量(0x30 步长)
  按名字串匹配 "qq-i18n-tip-msg"(0x479CDE0)取 i18n 提示;任务检查失败**不影响**门+invoke。
  **invoke = 7 参**:[this+0x28](cb,0x10,errCode,resultType,writer=[rsi],&f20,&f30)。
- **[0x73bd6c](0x390B,响应分发)全解**:this = 捕获块{writer@+8,**内层cb@+0x18**,flag@+0x38};
  code 三级 fallback = box+0x40 → +0x60 → +0x64;errStr = box+0x48/+0x68;
  **invoke = 6 参**:[this+0x30](cb,0x10,errCode,errStr,writer,&ctx+0x38串)——
  与 r1 时代预测 (delegate,0x10,errCode,errStr,msgUid) 吻合。
- 小调用方:0x3fb914 = 计数器[obj+0x100]++ + 门开则尾跳 [obj+0xF0](4 参 invoke);
  0x6a7d0c = 门开+索引未超限才 invoke;0x54cdef = "门未开"谓词(xor al,1,查询不 invoke);
  0x73ce0c = jmp 桩别名。**+584ms 单次 op4 无 invoke → 来源是谓词/收尾类,非 invoke 类**。

### 决定性发现:732A80 自建响应回调,消费位置 = arg5(上层传入的内层 delegate)
- 0x73bbf8(delegate 工厂,被 **732A80@0x732e2e** 调用):4D3588 = 0x75AF10 竞态分配器
  包装,固定 remaining=0x10 < size 0x40 → **恒返 0 → 永远走后备分支**
  {manager=0x73C2AA, invoker=0x73C34C};{0x73BC5C,0x73BD08} 为死分支。
  两对 manager 的 op=2 都写 {0x72D5C8,0x72D596} —— 与 r1 manager 契约互证。
- **0x73C34C(真 invoker)**:73bd6c 的 this = [closure] = 堆上 0x40 捕获块
  {ctx@0, sp@8, **内层delegate@+0x18**(0x20B,0x31C5F8 移动), flag@0x38}。
  73bd6c 门+invoke 的是**内层** delegate —— 外层 invoker 槽永不被响应路径调用!
- **732A80 尾段(0x732dae-0x732eb9)全解**:capture{ctx(rdi), sp(rbp+0x150),
  inner=[rbp+0x1E0]=**arg5**, flag(bl)} → 堆复制(0x73bb94 拷贝构造,0x24C0 分配)
  → 外层 delegate {storage0=堆指针, manager=0x73C2AA, invoker=0x73C34C} →
  **slot7 虚提交:r12=[ctx+0x60]->vtbl[+0x38](r12, &route串(ctx+0x1F0),
  &payload_qword, &请求描述(word 0x20 标记), &token_out, &delegate)**。
  KernelMsgService@wrapper vtbl@0x3F10458 slot7=0x320B5A → 转发
  this+0x478 特性检查 → this+0xE0 成员 → 0x3E31BC。
- token 语义闭环:slot7 返回 *out ^ &"msf" → perf(0x739590("perf") 是 perf 上报,
  **非** registry;739c66/73bd6c/732A80 三处 xor &"msf" 同源)。响应侧 *arg2 = 该 token。
- **路由缺口根因定案**:r1/r2/r3 我们 patch 的是外层 delegate 槽;manager 收到
  clone/op4/op3(服务收尾路径:门查询+销毁)✓,但响应 invoke 走内层槽(原生
  回调,无探针)→ 我们的 invoker 静默。裸发的响应**一直**被原生内层回调处理。
- 容器键修正:容器 = API 路由表,键 = 复合路由串("APICaller-{} "(0x2b84d8)、
  "{}-{} "(0x2b8650) 格式),25757ea 沿路由链逐前缀派发;值为 shared_ptr 服务对象
  (node+0x38;3fb8=lower_bound 查找,4075=键比较,414e=try_emplace 原语)。
  slot25(vtbl+0xC8)/2AA0D2(vtbl+0xB8) = 服务提交虚槽;4B3676(task,cmdId
  0x93A0/0x93A1/0x950F,…) = 命令提交变体。0x3F2D4E0 =
  "trpc.ecom.api_gateway.ApiGateway.SsoForward"(slot25 的命令名参数样例)。
- RTTI 佐证:KernelMsgService@wrapper@nt、MsgService/SysMsgService/GuildMsgService
  @im_core@nt、**BaseMsgWorker::SendMsgResult**(MsgScheduler lambda 签名
  (int,string,shared_ptr<IPBMessage>) = (errCode,errStr,msg) —— 与 73bd6c invoke 吻合)。

### v4 定稿(替代旧"任务构造"清单 —— 零 .text patch,消费位置正确)
- **732A80(ctx, a2, a3, flag(r9b), response_delegate(arg5))**:arg5 = 0x20B
  可移动 delegate(0x7333ac 传静态空回调 {0x2338,0x236a} 为证)。
- v4 = 我们的 DLL 裸调 732A80,arg5 传自建 delegate(manager = r2/r3 已验证安全
  的契约处理器;invoker = retCode 提取器 + FixedBuf JSONL + 文件锁)→
  响应原生路由:服务 → 外层门(我们 manager op4 → 开)→ 0x73C34C → 73bd6c →
  门(内层=我们的 manager)→ **invoke 我们的 invoker**(our_cb,0x10,errCode,
  errStr,…) —— 消费者装在原生消费位。
- 风险评估:manager 契约已三轮实测安全;invoker 只读 errStr(MSVC string SSO)
  + 定长格式化;无容器/任务伪造,无 hook;密度纪律照旧(速率上限/不重放)。
- 旧 v4 清单(任务布局/容器键/任务构造)被本路线**替代**,不再需要。

### 状态
- 纯离线轮:零注入零发送;实例 25072 未触碰;全部结论有反汇编地址锚点;
- 下一步:consumer_probe.rs 改造(invoker = 6 参消费版 + arg5 注入路径),
  现场 v4 实验(待用户批准注入)。
## 2026-10-11(三十五):v4 现场闭环 —— retCode 通道打通;根因 = 门极性反转

### 根因定案(离线反汇编 + mgrlog2 几何)
- **门语义反转**:0x3fbab2 = `3fce5e(rcx+0x10)` + `xor al,1`;0x3fce5e = 
  `manager(NULL,4,NULL,0,&out16,0)` 返回 out[0]!=0。0x72D596(原生空管理器)
  op>=4 臂 = `out[0]=1` —— **"1" = 我是空的**;0 = 非空可调用。
  r1 时代"out[0] 非零 = 门开"解读为**极性反转** —— r2/r3 我们的 manager
  答"非零(自称空)" → 73bd6c 在 +584ms 跳过 invoke 直接销毁。
  **路由全程通畅,从未缺上游注册;r2 的 token-registry 结论作废**。
- mgrlog2 克隆几何复核:manager 约定 = `(dest=&new.manager, op, src=旧块,
  0x10, out=新块)`;0x31C5F8 移动器**不复制 storage**(只重置源为空对
  {0x2338,0x236a})→ op=0 须复制 4 qword(r1 教训再证实)。

### v4 修复(consumer_probe.rs,commit 本轮)
1. op=4 → 写 out[0]=0(非空);
2. op=0 → 复制 src storage0/1 → out[0..2] + 写对(RPM-self 读,坏地址写 0);
3. op=1/3 → 重置为 wrapper 空对 {base+0x2338, base+0x236a}(ctx 新增
   wrapper_base 字段);
4. invoker → 6 参 (this, 0x10, errCode, errStr*, writer*, extra*);
5. __qq string 短形态 data@+1 修正(make_short_string 同款);
6. 回调零堆分配(FixedBuf 768B + JSON 转义)+ 进程级文件锁(半行 JSON 修复);
7. g3.rs 消费者分支补 cfg(research)门 —— **default 构建 E0433 自 r2 起预存,
   本提交修复**(全仓默认+research 测试绿 53+14+10+4+38 / 56+7+7+14)。

### N3R34 现场结果(单发;实例 25072 全程存活,OneBot 正常)
- 注入 n3res10(research CLI;首跑误用 default CLI 被 K4-D0 拒,重推
  `--features caligo-bridge/research` 后握手 PASS);
- 序列逐项命中预测:4×op=0 克隆(submit)→ **+417ms op=4 门查询(答 0)→
  consumer_result{tag:16, err_code:0, err_str:""} → op=3 销毁**;
- tag=16 = 0x10 精确匹配 73bd6c 约定;err_code=0 = 服务器受理(成功);
  err_str 空 = 成功无错文(符合预期);
- **送达验证**:群 263402786 seq 401041"time 1791688411 bot uid 文本
  'N3R34 v4 consumer probe'"(OneBot get_group_msg_history);
- 序号:--g3-send-seq auto 计至 3(g3-seq-state.txt 复用);
- 遗留(非阻塞):extra(73bd6c arg6,&rbp-0x40)读到乱码(疑似 [box+0x38]
  非 string 或 0x32c4 非 string ctor —— msgUid 提取待下一轮精化;
  err_code/err_str 主通道已够 retCode 判定)。

### 意义与下一步
- **retCode 通道闭环**:裸调 732A80 + arg5 自带 delegate = 完整响应消费者
  路径;N3 发送受理判定(errCode)现场可用;@ 重测的前提达成;
- v5 候选:extra/msgUid 精化(box+0x38 语义)、writer 字段解析(capture+8
  sp 对)、SendMsgResult 完整字段;以及把该消费者路径接入 N3 常驻
  native_session 的 SendAcceptance 判定。
## 2026-10-11(三十六):v5 双发 —— extra=span 解码;@ 拒因破案:1006501"网络连接异常!"(30.5s 无 ack)

### N3R35 基线(n3res11,普通群发;err_code=0 复现)
- consumer_result{tag:16, err_code:0, err_str:"", **extra_hex:""**} +417ms;
- extra 定案:0x32c4 = **span 构造器**([box+0x38] = {begin,end} 指针对,
  0x38aa 分配 + 0x38e6 拷贝 → {begin,end,cap_end} 0x20B 对象)——v4 的
  "乱码" = 旧 __qq-string 布局误读;成功路径 span 空(len 0)合法;
- 送达 seq 401115;v5 探针增量:span 解读 + 原始 qword 诊断位。

### N3R36 @ 重测(n3res12;@=u_KxYzu_qQPbCC-Ca8mLteVA/uin 3089665724;八轮归档后首次 retCode 可见)
- **完整生命周期与普通消息无异**:4×op=0 克隆 → 提交 → 等待;
- **+30.5s(30511ms)op=4 → consumer_result{tag:16, err_code:1006501,
  err_str:"网络连接异常!"} → op=3**;消息未送达(群历史无 N3R36);
- errStr 中文 UTF-8 经 read_qq_string + JSON 转义完整解码(串读取器现场验证);
- **"静默拒"真相**:客户端把 @ 消息正常提交进响应等待,30.5s 无服务器
  ack → 本地超时分类 1006501("网络连接异常!")。不是客户端丢弃
  (克隆/提交全走),不是即时服务端拒码 —— 服务器(或 transport)对
  该消息**不 ack**;
- 超时常量实测 ~30s(旧静态分析猜 180s 不符);
- 实例 25072 全程存活,OneBot 正常(两次注入后)。

### 下一步(@ 破案续)
- 对照实验:SnowLuma(同进程原生后端)发 @ 是否送达 —— 若送达,抓其
  wire body 与我们 r20 形状逐字节比对(at1 时代 body_len 306 vs 本次 300,
  差异在文本长度;@ elem 本体待对照);
- 或 hook/捕获真实客户端 @ 发送的 body(g2 族工具);retCode 消费者已就位,
  任何形状修正的验证成本 = 一次单发。
## 2026-10-11(三十七):@ 攻坚 v6–v12 + wirecap 钩子;@ 仍 1006501;实例第三次损失

### @ 实验矩阵(全部 = 30.5s 无 ack → 1006501"网络连接异常!";实例存活至 v12 前)
- v6(n3res13):@文本加 @ 前缀+尾空格、f12 去 f5/f11、attr len = UTF-16 units
  (真实件 18 units 实测)、顺序 Attr→at→rest→9→37 —— 1006501;
- v7(n3res14):elem37 换真实 88B(N3R37 堆转储,protobuf 遍历器逐字段核对;
  r28 125B 仿件 f19 带 f1:6 与多余全零字段属构造错误)+ 恢复元素16
  (v6 误删;真实值 {2:"Stella",3:1,5:0,7:"AI朋友"})—— 1006501;
- v8(n3res15):+ CH(r16 老规格 10 键含 f12)—— 1006501;
- v9(n3res16):CH 改 N3R37 记录实测 9 键 {1:82,3:1,4:divSeq,5:seq,6:ts,7:1,
  29:395247,30:1,32:r64}(**有 f3:1、无 f12;f5 = seq 非 random** —— r16
  老规格两处被真实件否定)—— 1006501;
- v10(n3res16,91 变体):**零克隆** —— [svc+0x60] 瞬时 null(环境抖动;
  随后 plain 复测 6 mgr_calls 全过)= 无效轮;
- v12(n3res19,串路由 Grp{1: "263402786"}):**注入期间实例死亡**(见事故)。

### wirecap 钩子(n3res18;工具沉淀)
- 补丁 732A80 两个直接调用点(0x732a3e/0x7333e6,各 5B E8)→ trampoline
  (g3::trampoline_bytes 复用:call capture(a1..a6) 后 jmp 732A80 入口);
  capture 转储 (svc, cmd, body hex) JSONL;RPM-self 读;文件锁;幂等装填;
- **我们的裸调不经调用点(直调)→ 零风险自扰**;hook 安装后 N3R44 plain
  err_code=0 正常(无副作用);
- **决定性发现:SnowLuma 的发送完全不经过 732A80 两个调用点**(N3R45 @ 与
  N3R46 plain 送达但零捕获)—— SnowLuma/NapCat 走另一条原生发送路径。
  "同路径对照"前提作废;真实 732A80-path @ wire body 仍未见(需真客户端
  GUI @ 发送 + 主账号 heap 扫描,或 qqxlog 解码);
- 1006501 定性:客户端本地错误表("网络连接异常!";1006502="发送异常!"),
  0x72a8a0 = 错误表构造器;传输层 0x1b421ba 内 30s 常量实测
  (`mov ebx,0x1E` + ms/1000)—— 30.5s 无匹配响应 = 服务器对 732A80-path 的
  @ body 静默不 ack(错误应答若到达会走同一路由进 delegate)。

### 事故(第 3 次损失;12 号注入)
- v12 注入(n3res19 = wirecap 重装 + 串路由 @)期间进程 25072 消失,
  handshake 未完成(v12-report 空);死亡点 = DLL 加载/远程线程早期;
- **wirecap 防护缺陷(已修)**:重装时仅查 E8 字节,已补丁站点(指向旧
  trampoline)误通过重补 —— 修正为校验完整 5B(E8 + rel32 指向 732A80);
- 死因未定案:重补丁路径存疑(近因),SnowLuma/风控亦可能;诚实记为
  未定案。**前一次成功活动 = 12:03:22 N3R44 err_code=0(retCode 通道完好)**。
- 现场纪律 v3 增补:①代码补丁类探针(wirecap)每次注入前必须核对其
  幂等防护覆盖"已补丁"状态;②同一 session 注入数达 10 即停,未再评估。

### 状态与下一步
- 实例 25072 死亡,SnowLuma 宿主需用户重启;重启后**勿立刻重注**,先
  OneBot 健康 + plain err_code 基线;
- retCode 通道结论不变(v4 定案,7 次现场复现);
- @ 谜题状态:body 已对齐存储记录 + CH 两式 + 串路由,全 1006501;
  剩余假设:①真实 732A80-wire 与存储记录存在未复制差异(需主账号 GUI
  @ + heap 扫描);②服务器对 legacy-path @ 静默弃(风控类,可能无解);
- qqxlog 解码器(NTQQ 日志)可作下一离线工具(需独立格式研究)。
## 2026-10-11(三十八):wirecap 触发两次实例损失,钩子路线放弃;转主账号 heap 扫描

### 崩溃链(诚实定案)
- 崩溃 #1(实例 10324,n3res20 + OOB 版 wirecap):用户 GUI 操作时死亡。
  根因 = **我的代码缺陷**:cmd 长形态分支对 16B 头切 lh[16..24] → panic →
  extern "system" abort。732A80 为通用 SSO 通道,GUI 流量长 cmd 即触。
- 崩溃 #2(实例 40432,n3res21 修复版):**窗口最大化即崩**。OOB 已修、
  capture 全路径审计无 panic、无分配格式化 —— 主嫌疑 = **trampoline 首次
  穿越本身**(g3::trampoline_bytes 的 call capture + jmp original 模式;
  该设计此前只在我们自己进程单测过,QQ 的 CFG/CET 缓解下 call→jmp
  shadow-stack 失配可即时终止;两次崩溃均为 GUI 触发包装器首次执行)。
  裸调直调不经调用点 → 我们的发送 8 次全存活 —— 完全吻合。
- **定案:调用点补丁类钩子在 QQ 现场不可用(纪律 v4:补丁类插桩 =
  slot7 级风险,整体否决)**。uninstall 例程已实现(--g3-send-ch 93 模式,
  写回原始 5B,幂等)。

### 复盘:40432 注入的卸载尝试
- 对 40432 的 n3res22 卸载注入落空 —— 进程已死(OpenProcess 失败,
  handshake 未完成)。当前仅 33736(主账号,昨日 23:33 起)存活。

### 转路线:零注入主账号 heap 扫描(工具 wirebody_scan.py 已备)
- 原理:主账号 GUI @ 发送走标准 732A80 管线,wire body 缓冲在应答后
  释放但字节通常残留于空闲块(r16 时代 capture-main-group-body 同法成功);
- 流程:用户在主账号 GUI 发 @ 哨兵消息(CAPWIRE47)→ 扫 33736 →
  按 protobuf 顶层形状分类各命中 → 取 wire body 与我们 v9 形状比对;
- 前置:bot 实例重启(OneBot 恢复),重注前先 plain 基线;**不再装钩子**。
## 2026-10-11(三十九):CAPWIRE47 主账号真体取证;v13 仍拒;@ 谜题终局判定

### 主账号(33736,732A80 真实路径)@ 件记录取证(wirebody_scan.py,CAPWIRE47 哨兵)
- 27 命中 → 4 份同内容存储记录;at-elem 真实形状(732A80 路径权威):
  f12 = {3:2, 4:uin, **5:0**, 9:peerUid}(f5 存在!N3R37/SnowLuma 异路径无 f5);
  at 文本 "@Stella" 无尾空格(空格在 rest 头);attr13B len = UTF-16 units ✓;
  **elem37 = f1:6 家族(r28 125B 常量本是对的)**;顺序 = Attr → 37 → 9 → at
  → **Rich.f11=0** → rest → 元素16(发送者信息);Attr f9 = "宋体";
  CH = **10 键含 f12=divSeq|1<<63**(r16 规格对本路径正确;f5 = seq)。
- **两路径形状系统性分歧**(SnowLuma vs 732A80):f5 有无、elem37 家族、
  CH 键集、f11 —— 此前 v6/v7/v9 按 N3R37(SnowLuma)修正 @ 全部修错了方向。

### v13(n3res23,全量回滚真路径形状:f5:0 + r28 elem37 + 真顺序 + f11=0 + 10键CH)
- **仍 1006501(+30.5s 无 ack)**;实例健康。形状假设空间穷尽:
  at-elem 三参照(SnowLuma 记录/主账号记录/r16 wire 规格)× CH 两式 ×
  路由两式 × 顺序两式,10+ 变体全测。

### @ 谜题终局判定(诚实结论)
- **服务器对 legacy-732A80-path 的 @ 消息静默不 ack**,与 body 形状无关
  (真路径形状逐字节对齐后依然);判因在 body 之外的包级/服务器侧
  (session 级 capability、包框架、或风控灰度)——以现有无钩子手段不可达;
- wirecap 钩子(唯一能看到 wire 的手段)已因两次实例损失永久退役;
  qqxlog 解码器 = 可选后续离线工程;
- **retCode 通道价值不受影响**:plain errCode=0 全程 8 次复现;@ 的
  拒绝也被通道如实上报(1006501)—— 通道本身就是本轮最大成果;
- **@ 生产建议**:经 SnowLuma/OneBot 路径(send_group_msg + at,已证送达
  N3R37/45);legacy 732A80-path @ 归档为"服务器侧静默弃"负结果。
- scope 文档"重定位 @ 阻塞于响应 retCode"任务关闭:retCode 已通,
  @ 拒因已测得(1006501 无 ack),形状空间已穷尽。

### 现场状态
- 实例 14632(13:16 起)健康,errCode 通道工作;今日注入 16 次、实例损失
  4 次(2 次定案为 wirecap 缺陷/机制,1 次风控踢,1 次r1契约错位);
- 新工具:wirebody_scan.py(主账号 heap 扫描)、wirecap(uninstall 模式)、
  protobuf 遍历器、pb-walker 分类器。
## 2026-10-11(四十):N4 接线 —— retCode 消费者闭环进常驻会话层

### 消费者 → 账本全链(纯进程内测试全绿;待 resident 现场跑)
1. **native_response**:新增 `ResponseOutcome::BusinessAccepted`(第 4 类;
   版本门理由:计划 §178"ACK 不能变成业务成功"要求 ack-without-msgUid 与
   BusinessSuccess 严格分立;field v4–v13 实证通道交付此形态);
2. **consumer_probe v6**:per-send cookie delegate(storage0/1 = cookie 16B
   原样;manager op=0 克隆已复制 storage → QQ 侧堆块携带 cookie 原路归还)+
   进程级响应槽 RESPONSE_SLOT + drain_responses();invoke 从克隆块 RPM 读回
   cookie 入槽(零 cookie = 诊断流只留 JSONL);init(report,wrapper) 登记;
3. **g3_ops**:submit 按 cookie 构建消费 delegate(research 分支;default =
   空回调)—— 发送即注册响应消费者,不再依赖独立探针流;
4. **native_resident**:adapter 启动时 consumer init;`drain_send_responses()`
   = 槽 → session.on_response → take_terminal;映射 code==0 → BusinessAccepted
   → SendOutcome::Success{native_id:None}(上层按"未关联"处理),code!=0 →
   BusinessRejected → Failure{retCode/errStr};
5. **resident**:泵线程每拍 `drain_send_responses()`(锁序 adapter→core);
   request 状态 SendPending → Done;结果经既有 take_results → IPC 上行。

### 测试
- cookie_delegate_roundtrip_via_slot:克隆合同(storage 复制 + 对写入)+
  门极性(op=4 答 0)+ 响应归还 cookie 原样 —— 全周期纯进程内验证;
- zero_cookie_stays_out_of_slot:诊断流不入槽;
- 全仓 default 23 套件 + research 58+7+7+14 全绿。

### 下一步(现场)
- resident 模式现场跑(caligod + IPC):daemon 发送 → Dispatched → QQ 响应 →
  BusinessAccepted/BusinessRejected 上行 —— N4 发送业务结果判定闭环;
- 前置:实例 14632 健康;注入密度纪律(今日已 16 次,建议明日执行)。
