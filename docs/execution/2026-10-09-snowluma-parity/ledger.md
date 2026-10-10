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
