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
