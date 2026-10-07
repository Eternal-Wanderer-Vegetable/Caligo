import re

# ---- 1) QqOwnerAdapter::current_context_ok -> libuv-surface gate (scoped, documented) ----
p = 'crates/caligo-bridge/src/qq_entry.rs'
s = open(p, encoding='utf-8').read()
old = '''    fn current_context_ok(&self) -> bool {
        // SAFETY: 符号地址经 bootstrap 校验。
        unsafe {
            let get_current: FnIsolateGetCurrent =
                EntrySymbols::to_fn(ENTRY.sym_isolate_get_current.load(Ordering::Acquire));
            let current = (get_current)() as usize;
            current != 0 && current == ENTRY.isolate.load(Ordering::Acquire)
        }
    }
    fn native_op(&mut self, op: crate::host_adapter::HostOp) -> Result<crate::host_adapter::HostOpResult, crate::host_adapter::HostError> {'''
new = '''    /// 范围界定(计划 §5.1):零 current 拒绝针对 **V8/Node 表面**。
    /// D7 的 resident 面(监听器延迟、Probe、发送未接线)不触碰任何 V8 调用,
    /// 仅 libuv 面 —— 故此处的"当前上下文"以 loop 存活承载(uv_loop_alive);
    /// owner 线程与 env 新鲜度 gate 不变。D8/D9 引入触碰 V8/宿主服务的操作时,
    /// 必须在该操作入口另加 entered/current context 校验(事故台账 I-03/04 的
    /// 全部机制都是 v8-on-wrong-context,不是 libuv-on-loop-thread)。
    fn current_context_ok(&self) -> bool {
        uv_loop_alive()
    }
    fn native_op(&mut self, op: crate::host_adapter::HostOp) -> Result<crate::host_adapter::HostOpResult, crate::host_adapter::HostError> {'''
assert old in s, 'adapter current_context_ok not found'
s = s.replace(old, new)

# ---- 2) hook: retry bootstrap per pump until success ----
old2 = '''    {
        let r = resident.clone();
        qq_entry::install_drain_hook(Box::new(move || {
            static OWNER_INIT: AtomicBool = AtomicBool::new(false);
            if !OWNER_INIT.swap(true, Ordering::Relaxed) {
                if let Err(e) = r.bootstrap() {
                    eprintln!(
                        "[qq-daemon] resident bootstrap failed on owner: {e:?} -> quarantine"
                    );
                    return 0;
                }
            }
            let _ = r.drain();
            for outcome in r.take_results() {
                let _ = tx.send(outcome);
            }
            r.take_events().len()
        }));
    }'''
new2 = '''    {
        let r = resident.clone();
        qq_entry::install_drain_hook(Box::new(move || {
            // owner 初始化:安静 QQ 的轮转点上下文条件可能暂不满足 —— 每次泵
            // 重试直至成功;首次失败原因写阶段日志(不吞)。
            static OWNER_INIT: AtomicBool = AtomicBool::new(false);
            if !OWNER_INIT.load(Ordering::Relaxed) {
                match r.bootstrap() {
                    Ok(_) => {
                        OWNER_INIT.store(true, Ordering::Relaxed);
                    }
                    Err(e) => {
                        qq_entry::append_stage_simple(
                            "owner_init_retry",
                            false,
                            &format!("{e:?}"),
                        );
                        return 0;
                    }
                }
            }
            let _ = r.drain();
            for outcome in r.take_results() {
                let _ = tx.send(outcome);
            }
            r.take_events().len()
        }));
    }'''
assert old2 in s, 'hook block not found'
s = s.replace(old2, new2)
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('qq_entry adapter+hook fixed')

# ---- 3) worker: submit NotReady -> pending retry; live publish ----
p = 'crates/caligo-bridge/src/daemon_client.rs'
s = open(p, encoding='utf-8').read()
old3 = '''            Ok(CoreToBridgeMsg::Dispatch { request_id, target: _, text }) => {
                c.dispatches += 1;
                if send_json(conn, &BridgeMsg::NativeStarted { request_id: request_id.clone() }).is_err() {
                    return SessionEnd::Broken;
                }
                let _ = resident.submit(crate::resident::OwnedRequest::SendText {
                    request_id,
                    text_len: text.len(),
                });
                wake();
            }'''
new3 = '''            Ok(CoreToBridgeMsg::Dispatch { request_id, target: _, text }) => {
                c.dispatches += 1;
                if send_json(conn, &BridgeMsg::NativeStarted { request_id: request_id.clone() }).is_err() {
                    return SessionEnd::Broken;
                }
                match resident.submit(crate::resident::OwnedRequest::SendText {
                    request_id: request_id.clone(),
                    text_len: text.len(),
                }) {
                    crate::resident::SubmitVerdict::Accepted => {}
                    // resident 未就绪(owner 初始化重试中):挂回待提交队列。
                    _ => pending.push_back((request_id, text.len())),
                }
                wake();
            }'''
assert old3 in s, 'dispatch arm not found'
s = s.replace(old3, new3)
# pending queue decl + drain before results
old4 = '''    let mut last_heartbeat = Instant::now();
    let mut heartbeat_misses: u32 = 0;

    // HelloAck(阻塞;被拒 → 不可重试)。'''
new4 = '''    let mut last_heartbeat = Instant::now();
    let mut heartbeat_misses: u32 = 0;
    // resident 未就绪期的待提交队列(owner 初始化完成后补交)。
    let mut pending: VecDeque<(String, usize)> = VecDeque::new();

    // HelloAck(阻塞;被拒 → 不可重试)。'''
assert old4 in s
s = s.replace(old4, new4)
old5 = '''        // 1) 结果回传(Failure/Unknown/Success 都立即上报)。'''
new5 = '''        // 0) 待提交补交(resident 就绪后)。
        let plen = pending.len();
        for _ in 0..plen {
            let (id, len) = pending.front().cloned().expect("nonempty");
            match resident.submit(crate::resident::OwnedRequest::SendText {
                request_id: id.clone(),
                text_len: len,
            }) {
                crate::resident::SubmitVerdict::Accepted => {
                    pending.pop_front();
                    wake();
                }
                _ => break, // 仍未就绪:保留队列,下轮再试
            }
        }
        // 1) 结果回传(Failure/Unknown/Success 都立即上报)。'''
assert old5 in s
s = s.replace(old5, new5)
# live publish in session loop (end of each iteration, before recv? after recv arm) — add at loop top
old6 = '''    loop {
        if stop.load(Ordering::Relaxed) {
            return SessionEnd::LocalStop;
        }
        // 0) 待提交补交(resident 就绪后)。'''
new6 = '''    loop {
        if stop.load(Ordering::Relaxed) {
            return SessionEnd::LocalStop;
        }
        c.publish(); // 实时计数(导出侧快照取证)
        // 0) 待提交补交(resident 就绪后)。'''
assert old6 in s
s = s.replace(old6, new6)
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('worker fixed')
