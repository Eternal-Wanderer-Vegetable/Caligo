# 响应侧拒绝码消费者 — 范围与锚点(下一工作块起点)

日期:2026-10-09。状态:**范围已定;静态合同大幅闭合(同日 N 阶段增补,见文末);实现未开始**。
动机:群聊 @ 消息 r20–r28b 八轮静默拒(普通文本同桥 PASS,见
`g3-send-27624-r2x.jsonl`),无 retCode 无法继续定位;发送失败可观测性
是 @ 元素与一切后续功能(回复/转发)的调试前提。

## 已解契约(2026-10-09,反编译 r4-g3 phaseAE2)

### 注册侧 0x95d4e6(发送前由 25757ea 调用)
```
95d4e6(cookie*: [4]u32, spA: *shared_ptr, spB: *shared_ptr(record), cbCfg)
  若 A、B 均非空:
    cookieArr = {tag=2, cookie[0..16]}
    940588(&cookieArr, thunk@0x605d22) → 门控(bool)      # [更正] 第2参非字符串,
                                                          # 是 vcall thunk jmp [vptr+0xa8]
    若门控:
      key_fmt = 97b0f(fmt模板对象@0x3faa680)              # 模板 = "{} {}'{}\""(活内存实证)
                                                            # 格式化实参(反编译未保留,待运行时)
      key1 = 58e72(key_fmt, "svr"@0x4721cdf)              # 字符串拼接
      c707b6(A, cbCfg, key1)                              # [更正] 仅序列化 A + 日志(非注册!)
      key2 = 58e72(key_fmt, "sdk"位置@0x4791e1d)
      94e64a(*B, elements)                                # record → 元素表
      c707b6(elements, cbCfg, key2)                       # 仅日志
```
- cookie = **客户端自配的 16B msgUid**(25757ea 从发送上下文 +0x20 取)。
- A = 消息对象(vtbl+0x100 序列化;c707b6 日志会序列化它);
  B = MsgRecord(B vtbl slot0 有一次无参调用,产物被 key 覆盖,用途不明)。
- cbCfg = 9bef60 产物:对 record 形对象设特性键 0x17d5–0x17db、0x1a2d–0x1a30。
- **[更正] "注册"语义**:95d4e6 内没有消费者表写入;c707b6 全程只序列化+打日志
  (其 fmt 参考源路径 ".\modules\sys_env\publi…" 为日志文件名)。真正的
  消费者安装发生在 cbCfg(9bef60)的构造/设置路径 —— 注册点需重新定位。

### 响应侧 0x73bd6c(响应到达分发)
- param_1 = 消费者/处理器对象:{+8 日志器, +0x18 inner, +0x30 消费者
  调用指针, +0x38 verbose 标志}。
- 错误码提取(反编译实证,层级完整):
  1. 传输层参数(param_3)非 0 → errCode = param_3,errStr = 错误映射表
     查询(未命中 = "[MSF] Unknow Error!"@0x48df2da 区);
  2. resp 空 → errCode = **-1**(超时哨兵),errStr = "got empty response"
     (@0x47b1cd1 区);
  3. resp+0x40(int)非 0 → errCode = resp+0x40,errStr = string@resp+0x48;
  4. resp+0x80(字节)非 0 且 resp+0x60 非 0 → errCode = resp+0x60,errStr = resp+0x68;
  5. resp+0x80 非 0 且 resp+0x64 非 0 → errCode = resp+0x64,errStr = resp+0x68;
  6. 否则 errCode = 0(成功)。
- 终局调用(**合同闭合**):
  `(param_1+0x30)(param_1+0x18, 0x10, errCode:int, errStr:string, msgUid:string)`
  —— 门控 = 3fbab2(param_1+0x18)(消费者在场才调用);
  **msgUid = string@resp+0x38**(libc++ 串拷贝)。

## 实现路径(建议)
1.  bridge 内构造假 MsgRecord(vtable 0x3f51a78 + CPBMessageOpti 条目数组,
    最小 props:0x9c41/0x9c4a/0x9c55)与假消息对象 A(vtbl+0x100 序列化)。
2.  cbCfg/9bef60 路径重新定位消费者安装点(95d4e6 已证不是注册点)。
3.  以发送时用的 16B msgUid 为 cookie;请求体与 cookie 的关联字段待确认
    (响应按 resp+0x38 的 msgUid 串回带;cookie→msgUid 恒等假设待一次
    受控正例验证)。
4.  消费者入口(被 param_1+0x30 调用的函数)实现
    (inner, 0x10, errCode, errStr, msgUid) 签名,写入 g3s jsonl。

## 未决问题(实现期解;2026-10-09 静态收敛后剩三项)
- ~~940588 门控语义~~ → 收窄为:**slot 0xa8 背后的实现**(0x605d22 是
  vcall thunk,非字符串);需 941c96/该类 vtable 解码或运行时观测。
- **97b0f 的 fmt 实参**(3 个 {} 的实参来源;反编译丢失,需运行时或更深层解码)。
- ~~key 组合器格式~~ → key = fmt("{} {}'{}\"") + 后缀字面量(58e72 = 纯拼接,
  已闭合;后缀字节位置的精确字面量边界待定:"svr"/"sdk" 位置读数)。
- ~~请求 PB 中 msgUid 字段号~~ → 收窄为 cookie→msgUid 恒等验证(响应回带
  msgUid 串已实证;请求侧关联字段一次受控正例可定)。
- ~~消费者 A 实参 ABI~~ → **已闭合**:(inner, 0x10, errCode, errStr, msgUid)。
  剩余:消费者对象本身的 vtable/寿命(被 9bef60/cbCfg 持有方式)。

## 证据
- r4-g3 phaseAE2/functions/(95d4e6.c、73bd6c.c、9bef60.c、c707b6.c、940588.c、
  941b3a.c、97b0f.c、58e72.c、d3be4f.c = fmt 引擎、d3cb24.c)
- 25757ea.c(调用点上下文)
- @ 拒绝迭代:r20–r28b(g3-send-27624-r2x.jsonl;形状演进全记录)
- 活内存字符串读取(2026-10-09,实例 27108,外部 RPM 只读):
  `E:/stella/_reference/caligo-n3-resident-20261009/resp-consumer-strings.json`
  —— 0x3faa680 = "{} {}'{}\"";0x605d22 = thunk 字节(非串);
  "got empty response"/"[MSF] Unknow Error!" 文案实证。
