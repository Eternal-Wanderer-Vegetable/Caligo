//! K4-D5 常驻 core 会话层:管道 server、双角色会话与账号 actor 的接线
//! (计划 §7-D5:`host harness → resident → pipe → runtime actor → journal
//! → test client`)。
//!
//! 线程模型:daemon 持有 `Mutex<AccountActor>`(单 owner 语义由互斥串行化);
//! bridge 与 control 各一条连接一个线程,请求/应答式 + 出站泵。
//! 停止:任一入口 Stop → 停止标志 → accept 取消 → actor.stop/close(Closed
//! 前在途项显式 unknown)。core 非正常退出由断连恢复(测试在 recovery_contract)。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::ipc::{
    encode_frame, validate_auth, validate_hello_v2, BridgeMsg, ControlMsg, CoreToBridgeMsg,
    CoreToControlMsg, EventPayload, OutcomePayload, Role, PROTOCOL_VERSION_V2, FRAME_MAGIC,
    MAX_FRAME_SIZE,
};
use crate::runtime::{host_identity, AccountActor, NativeResult, RuntimeConfig};
use crate::transport::{connect_client, generate_token, token_hex, PipeConnection, PipeServer, TransportError};
use caligo_model::{Direction, EventSource, MsgEvent, NativeMessageId, SendTextRequest, SessionIdentity};

/// 会话帧读取错误。
#[derive(Debug)]
pub enum SessionError {
    Transport(TransportError),
    Frame(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
    Closed,
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SessionError::Transport(e) => write!(f, "transport: {e}"),
            SessionError::Frame(e) => write!(f, "frame: {e}"),
            SessionError::Io(e) => write!(f, "io: {e}"),
            SessionError::Serde(e) => write!(f, "json: {e}"),
            SessionError::Closed => write!(f, "session closed"),
        }
    }
}

/// 读取一帧(8 字节头 + payload);`cancel` 置位时中断等待。
fn read_frame(
    conn: &PipeConnection,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, SessionError> {
    let mut head = [0u8; 8];
    conn.read_exact(&mut head, Some(cancel)).map_err(SessionError::Transport)?;
    let magic = u32::from_le_bytes(head[0..4].try_into().unwrap());
    if magic != FRAME_MAGIC {
        return Err(SessionError::Frame(format!("magic {magic:#010x}")));
    }
    let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(SessionError::Frame(format!("frame too large: {len}")));
    }
    let mut payload = vec![0u8; len];
    if len > 0 {
        conn.read_exact(&mut payload, Some(cancel)).map_err(SessionError::Transport)?;
    }
    Ok(payload)
}

fn send_json<T: Serialize>(conn: &PipeConnection, msg: &T) -> Result<(), SessionError> {
    let payload = serde_json::to_vec(msg).map_err(SessionError::Serde)?;
    let frame = encode_frame(&payload).map_err(|e| SessionError::Frame(e.to_string()))?;
    conn.write_all(&frame).map_err(SessionError::Transport)
}

fn recv_json<T: for<'de> Deserialize<'de>>(
    conn: &PipeConnection,
    cancel: &AtomicBool,
) -> Result<T, SessionError> {
    let payload = read_frame(conn, cancel)?;
    serde_json::from_slice(&payload).map_err(SessionError::Serde)
}

/// daemon 配置。`expect_client_pid` 为指定进程校验(0 = 不校验)。
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub pipe_prefix: String,
    pub journal_path: PathBuf,
    pub module_baseline: String,
    pub account: String,
    pub core_build: String,
    pub expect_client_pid: u32,
    pub runtime: RuntimeConfig,
}

/// 常驻 core(LAB 形态;计划 §7-D5)。
pub struct Daemon {
    pub config: DaemonConfig,
    auth_hex: String,
    core_nonce: u64,
    pub(crate) actor: Mutex<AccountActor>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) epoch: AtomicU64,
    /// 待派发队列(control 受理后经此转 bridge)。
    pub(crate) dispatch_out: Mutex<Vec<CoreToBridgeMsg>>,
    pub(crate) bridge_hello_seen: AtomicBool,
    /// bridge 会话读唤醒:control 推入派发后置位,使其阻塞读立即返回
    /// (read 以 100ms 粒度轮询该标志)。
    pub(crate) bridge_wake: Arc<AtomicBool>,
}

impl Daemon {
    pub fn start(config: DaemonConfig) -> Result<Arc<Self>, crate::runtime::OpenError> {
        let (actor, _report) =
            AccountActor::open(&config.journal_path, config.runtime.clone())?;
        let token = generate_token().map_err(|_| {
            crate::runtime::OpenError::Journal(crate::journal::JournalError::Io(
                std::io::Error::new(std::io::ErrorKind::Other, "no os rng"),
            ))
        })?;
        Ok(Arc::new(Self {
            auth_hex: token_hex(&token),
            core_nonce: u64::from_le_bytes(token[0..8].try_into().unwrap()),
            actor: Mutex::new(actor),
            stop: Arc::new(AtomicBool::new(false)),
            epoch: AtomicU64::new(1),
            dispatch_out: Mutex::new(Vec::new()),
            bridge_hello_seen: AtomicBool::new(false),
            bridge_wake: Arc::new(AtomicBool::new(false)),
            config,
        }))
    }

    /// (bridge, control) 管道名。
    pub fn pipe_names(&self) -> (String, String) {
        (
            format!("{}-bridge", self.config.pipe_prefix),
            format!("{}-control", self.config.pipe_prefix),
        )
    }

    pub fn auth_token(&self) -> &str {
        &self.auth_hex
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    /// 阻塞运行:双 accept 循环;停止后收尾 actor(stop→close)。
    pub fn run(self: &Arc<Self>) -> Result<(), SessionError> {
        let (bridge_name, control_name) = self.pipe_names();
        let bridge_srv = PipeServer::create(&bridge_name).map_err(|e| {
            eprintln!("[daemon] create {bridge_name}: {e}");
            SessionError::Transport(e)
        })?;
        let control_srv = PipeServer::create(&control_name).map_err(|e| {
            eprintln!("[daemon] create {control_name}: {e}");
            SessionError::Transport(e)
        })?;
        let t_bridge = {
            let d = self.clone();
            std::thread::spawn(move || d.accept_loop(&bridge_srv, Role::Bridge))
        };
        let t_control = {
            let d = self.clone();
            std::thread::spawn(move || d.accept_loop(&control_srv, Role::Control))
        };
        let r1 = t_bridge.join().expect("bridge accept thread");
        let r2 = t_control.join().expect("control accept thread");
        // 收尾:取消未执行 → 在途显式 unknown → Closed。
        let mut actor = self.actor.lock().unwrap();
        let _ = actor.stop();
        let _ = actor.close();
        r1.and(r2)
    }

    fn accept_loop(self: &Arc<Self>, srv: &PipeServer, role: Role) -> Result<(), SessionError> {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let conn = match srv.accept(&self.stop) {
                Ok(c) => c,
                Err(TransportError::TimedOut) => continue,
                Err(e) => return Err(SessionError::Transport(e)),
            };
            // PID 校验(指定进程;0 = 不校验)。
            if self.config.expect_client_pid != 0 {
                match conn.client_pid() {
                    Ok(pid) if pid == self.config.expect_client_pid => {}
                    Ok(pid) => {
                        return Err(SessionError::Frame(format!(
                            "client pid {pid} != designated {}",
                            self.config.expect_client_pid
                        )));
                    }
                    Err(e) => return Err(SessionError::Transport(e)),
                }
            }
            let serve_r = match role {
                Role::Bridge => self.serve_bridge(conn),
                Role::Control => self.serve_control(conn),
            };
            if let Err(e) = &serve_r {
                eprintln!("[daemon] {role:?} session error: {e}");
            }
            serve_r?;
            // 排水窗:最终应答(Stopped/拒绝 ack)需要客户端先读走;
            // DisconnectNamedPipe 会丢弃未读数据,立即断开会吞掉收尾应答。
            std::thread::sleep(std::time::Duration::from_millis(200));
            srv.disconnect();
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
        }
    }

    fn hello_ack(accepted: bool, reject: Option<String>, build: &str, epoch: u64) -> serde_json::Value {
        serde_json::json!({
            "t": "hello_ack",
            "d": {
                "protocol_version": PROTOCOL_VERSION_V2,
                "core_build": build,
                "connection_epoch": epoch,
                "accepted": accepted,
                "reject_reason": reject,
            }
        })
    }

    fn serve_bridge(self: &Arc<Self>, conn: PipeConnection) -> Result<(), SessionError> {
        // 握手:版本 → 认证 → 角色 → 基线/账号/代次。
        let hello: BridgeMsg = recv_json(&conn, &self.stop)?;
        let BridgeMsg::Hello {
            protocol_version,
            bridge_build,
            auth_token,
            role,
            session_generation,
            account,
            module_baseline,
        } = hello
        else {
            send_json(&conn, &Self::reject_val("first message must be hello"))?;
            return Err(SessionError::Frame("bridge hello expected".into()));
        };
        let epoch = self.epoch.load(Ordering::Relaxed);
        let reject = |reason: String| {
            let _ = send_json(
                &conn,
                &Self::hello_ack(false, Some(reason), &self.config.core_build, epoch),
            );
        };
        if let Err(e) = validate_hello_v2(protocol_version, role, Role::Bridge, &self.config.core_build) {
            reject(e.to_string());
            return Ok(());
        }
        if !validate_auth(&auth_token, &self.auth_hex) {
            reject("auth failed".into());
            return Ok(());
        }
        if module_baseline != self.config.module_baseline {
            reject(format!("baseline mismatch: {module_baseline}"));
            return Ok(());
        }
        if account != self.config.account {
            reject(format!("account mismatch: {account}"));
            return Ok(());
        }
        // 会话绑定(首次 bridge 接入 = Bound→Active)。
        {
            let mut actor = self.actor.lock().unwrap();
            let host = host_identity(
                conn.client_pid().unwrap_or(0),
                "",
                &module_baseline,
                &bridge_build,
                self.core_nonce,
            );
            let session = SessionIdentity {
                host,
                account: caligo_model::AccountId(account.clone()),
                session_generation,
            };
            if let Err(e) = actor.attach_session(session) {
                // 已绑定同代次 → 幂等(重连场景)。
                let same = actor
                    .session()
                    .map(|s| {
                        s.session_generation == session_generation && s.account.0 == account
                    })
                    .unwrap_or(false);
                if !same {
                    reject(format!("attach rejected: {e:?}"));
                    return Ok(());
                }
                let _ = actor.reconnect(self.conn_identity(epoch));
            } else if let Err(e) = actor.mark_connection_ready(self.conn_identity(epoch)) {
                reject(format!("connection rejected: {e:?}"));
                return Ok(());
            }
        }
        send_json(
            &conn,
            &Self::hello_ack(true, None, &self.config.core_build, epoch),
        )?;
        self.bridge_hello_seen.store(true, Ordering::Relaxed);

        // 组合唤醒观察者:stop 或 dispatch 到达都会打断 bridge 会话的阻塞读
        // (read 以 50ms 粒度轮询 recv_wake)。观察者随会话结束退出。
        let session_open = Arc::new(AtomicBool::new(true));
        let recv_wake = Arc::new(AtomicBool::new(false));
        {
            let s_open = session_open.clone();
            let s_stop = self.stop.clone();
            let s_dispatch = self.bridge_wake.clone();
            let s_wake = recv_wake.clone();
            std::thread::spawn(move || loop {
                if !s_open.load(Ordering::Relaxed) {
                    return;
                }
                if s_stop.load(Ordering::Relaxed) || s_dispatch.load(Ordering::Relaxed) {
                    s_wake.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            });
        }

        // 会话循环:泵出站(Dispatch)→ 阻塞等入站;派发/停止经 recv_wake
        // 唤醒阻塞读,消除"泵永不到来"的互等。
        loop {
            if self.stop.load(Ordering::Relaxed) {
                let _ = send_json(&conn, &CoreToBridgeMsg::Stopped {});
                return Ok(());
            }
            // 先清除唤醒,再泵出站:control 之后推入的派发会重新置位
            // bridge_wake(观察者随之置位 recv_wake),阻塞读最多 50ms 内返回。
            recv_wake.store(false, Ordering::Relaxed);
            let pending: Vec<CoreToBridgeMsg> =
                std::mem::take(&mut *self.dispatch_out.lock().unwrap());
            for m in pending {
                send_json(&conn, &m)?;
            }
            match recv_json::<BridgeMsg>(&conn, &recv_wake) {
                Ok(msg) => match msg {
                    BridgeMsg::NativeStarted { request_id } => {
                        let mut actor = self.actor.lock().unwrap();
                        let _ = actor.bridge_started(&request_id);
                    }
                    BridgeMsg::SendResult { request_id, outcome } => {
                        let result = match outcome {
                            OutcomePayload::Success { native_id } => NativeResult::Success {
                                native_id: native_id.map(NativeMessageId),
                            },
                            OutcomePayload::Failure { reason } => NativeResult::Failure { reason },
                            OutcomePayload::Unknown => NativeResult::Unknown,
                        };
                        let mut actor = self.actor.lock().unwrap();
                        let _ = actor.bridge_result(&request_id, result);
                    }
                    BridgeMsg::Event { event } => {
                        let mut actor = self.actor.lock().unwrap();
                        let receipt = actor.on_event(self.to_event(event));
                        let (seq, suppressed) = match receipt {
                            Ok(r) => match r {
                                crate::runtime::EventReceipt::Published { event_seq } => {
                                    (event_seq, false)
                                }
                                crate::runtime::EventReceipt::DuplicateSuppressed => {
                                    (0, true)
                                }
                                crate::runtime::EventReceipt::AuditedOldGeneration { event_seq } => {
                                    (event_seq, false)
                                }
                            },
                            Err(_) => (0, true),
                        };
                        send_json(&conn, &CoreToBridgeMsg::EventAck { event_seq: seq, suppressed })?;
                    }
                    BridgeMsg::Health { .. } => {
                        send_json(&conn, &CoreToBridgeMsg::HealthAck {})?;
                    }
                    BridgeMsg::Stop {} => {
                        let _ = send_json(&conn, &CoreToBridgeMsg::Stopped {});
                        session_open.store(false, Ordering::Relaxed);
                        return Ok(());
                    }
                    BridgeMsg::Hello { .. } => {
                        send_json(&conn, &CoreToBridgeMsg::Reject { reason: "duplicate hello".into() })?;
                    }
                },
                Err(SessionError::Transport(TransportError::TimedOut)) => {
                    if self.stop.load(Ordering::Relaxed) {
                        let _ = send_json(&conn, &CoreToBridgeMsg::Stopped {});
                        session_open.store(false, Ordering::Relaxed);
                        return Ok(());
                    }
                    continue;
                }
                Err(SessionError::Transport(TransportError::BrokenPipe)) => {
                    // bridge 断连:停止派发准入;允许重连(recovery 契约)。
                    let mut actor = self.actor.lock().unwrap();
                    actor.degrade("bridge disconnected");
                    session_open.store(false, Ordering::Relaxed);
                    return Ok(());
                }
                Err(e) => {
                    session_open.store(false, Ordering::Relaxed);
                    return Err(e);
                }
            }
        }
    }

    fn serve_control(self: &Arc<Self>, conn: PipeConnection) -> Result<(), SessionError> {
        let hello: ControlMsg = recv_json(&conn, &self.stop)?;
        let ControlMsg::Hello { protocol_version, auth_token, role, .. } = hello else {
            let _ = send_json(&conn, &CoreToControlMsg::Reject {
                reason: "first message must be hello".into(),
            });
            return Err(SessionError::Frame("control hello expected".into()));
        };
        let epoch = self.epoch.load(Ordering::Relaxed);
        let ack = |accepted: bool, reject: Option<String>, conn: &PipeConnection| -> Result<(), SessionError> {
            send_json(conn, &CoreToControlMsg::HelloAck {
                protocol_version: PROTOCOL_VERSION_V2,
                core_build: self.config.core_build.clone(),
                connection_epoch: epoch,
                accepted,
                reject_reason: reject,
            })
        };
        if let Err(e) = validate_hello_v2(protocol_version, role, Role::Control, &self.config.core_build) {
            ack(false, Some(e.to_string()), &conn)?;
            return Ok(());
        }
        if !validate_auth(&auth_token, &self.auth_hex) {
            ack(false, Some("auth failed".into()), &conn)?;
            return Ok(());
        }
        ack(true, None, &conn)?;

        loop {
            if self.stop.load(Ordering::Relaxed) {
                let _ = send_json(&conn, &CoreToControlMsg::Stopped {});
                return Ok(());
            }
            match recv_json::<ControlMsg>(&conn, &self.stop) {
                Ok(msg) => match msg {
                    ControlMsg::SendText { request_id, target, text, deadline_ms } => {
                        let mut actor = self.actor.lock().unwrap();
                        let Some(session) = actor.session().cloned() else {
                            send_json(&conn, &CoreToControlMsg::Reject { reason: "no active session".into() })?;
                            continue;
                        };
                        let req = SendTextRequest {
                            request_id: request_id.clone(),
                            session_identity: session,
                            target: caligo_target(&target),
                            text,
                            payload_hash: 0,
                            deadline_ms,
                        };
                        let req = SendTextRequest {
                            payload_hash: SendTextRequest::compute_payload_hash(
                                req.session_identity.session_generation,
                                &req.target,
                                &req.text,
                                deadline_ms,
                            ),
                            ..req
                        };
                        match actor.submit_send(req.clone()) {
                            Ok(state) => {
                                // 受理即派发(dispatch_intent 窗口显式化)。
                                if actor.dispatch_begin(&request_id).is_ok() {
                                    self.dispatch_out.lock().unwrap().push(CoreToBridgeMsg::Dispatch {
                                        request_id: request_id.clone(),
                                        target,
                                        text: req.text,
                                    });
                                    self.bridge_wake.store(true, Ordering::Relaxed);
                                }
                                send_json(&conn, &CoreToControlMsg::SendAccepted {
                                    request_id,
                                    state: format!("{state:?}"),
                                })?;
                            }
                            Err(e) => {
                                send_json(&conn, &CoreToControlMsg::Reject { reason: format!("{e:?}") })?;
                            }
                        }
                    }
                    ControlMsg::QueryRequest { request_id } => {
                        let actor = self.actor.lock().unwrap();
                        let (state, native_id) = match actor.request_summary(&request_id) {
                            Some(s) => (Some(format!("{:?}", s.state)), native_of_summary(&actor, &request_id)),
                            None => (None, None),
                        };
                        send_json(&conn, &CoreToControlMsg::QueryResult { request_id, state, native_id })?;
                    }
                    ControlMsg::CancelQueued { request_id } => {
                        let mut actor = self.actor.lock().unwrap();
                        let verdict = match actor.cancel_queued(&request_id) {
                            Ok(v) => match v {
                                crate::runtime::CancelOutcome::CancelledUnsent => "cancelled_unsent",
                                crate::runtime::CancelOutcome::TooLate => "too_late",
                                crate::runtime::CancelOutcome::NotFound => "not_found",
                            },
                            Err(_) => "rejected",
                        };
                        send_json(&conn, &CoreToControlMsg::CancelResult { request_id, verdict: verdict.into() })?;
                    }
                    ControlMsg::DrainEvents { max } => {
                        let mut actor = self.actor.lock().unwrap();
                        let events: Vec<EventPayload> = actor
                            .drain_deliverables(max)
                            .into_iter()
                            .map(daemon_event_payload)
                            .collect();
                        send_json(&conn, &CoreToControlMsg::EventsBatch { events })?;
                    }
                    ControlMsg::Health {} => {
                        send_json(&conn, &CoreToControlMsg::HealthAck {})?;
                    }
                    ControlMsg::Stop {} => {
                        self.stop.store(true, Ordering::Relaxed);
                        send_json(&conn, &CoreToControlMsg::Stopped {})?;
                        return Ok(());
                    }
                    ControlMsg::Hello { .. } => {
                        send_json(&conn, &CoreToControlMsg::Reject { reason: "duplicate hello".into() })?;
                    }
                },
                Err(SessionError::Transport(TransportError::TimedOut)) => continue,
                Err(SessionError::Transport(TransportError::BrokenPipe)) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    fn conn_identity(&self, epoch: u64) -> caligo_model::ConnectionIdentity {
        caligo_model::ConnectionIdentity {
            core_instance_nonce: self.core_nonce,
            connection_epoch: epoch,
            protocol_version: PROTOCOL_VERSION_V2,
        }
    }

    fn to_event(&self, p: EventPayload) -> MsgEvent {
        let session = caligo_target(&p.session);
        MsgEvent {
            event_seq: 0,
            session_generation: p.session_generation,
            session,
            direction: match p.direction.as_str() {
                "self_sent" => Direction::SelfSent,
                _ => Direction::Incoming,
            },
            sender: p.sender,
            native_id: NativeMessageId(p.native_id),
            text: p.text,
            platform_time: p.platform_time,
            observed_at_unix_ms: p.observed_at_unix_ms,
            source: match p.source.as_str() {
                "update" => EventSource::Update,
                "history_backfill" => EventSource::HistoryBackfill,
                _ => EventSource::Recv,
            },
        }
    }

    fn reject_val(reason: &str) -> serde_json::Value {
        serde_json::json!({ "t": "reject", "d": { "reason": reason } })
    }
}

fn native_of_summary(actor: &AccountActor, id: &str) -> Option<String> {
    match actor.query_request(id) {
        Some(caligo_model::ActionState::ConfirmedSuccess { native_id }) => {
            native_id.map(|n| n.0)
        }
        _ => None,
    }
}

fn daemon_event_payload(ev: MsgEvent) -> EventPayload {
    EventPayload {
        event_seq: ev.event_seq,
        session_generation: ev.session_generation,
        session: serde_json::json!({
            "account": ev.session.account.0,
            "kind": ev.session.kind.to_string(),
            "peer": ev.session.peer.0,
        }),
        direction: match ev.direction {
            Direction::Incoming => "incoming".into(),
            Direction::SelfSent => "self_sent".into(),
        },
        sender: ev.sender,
        native_id: ev.native_id.0,
        text: ev.text,
        platform_time: ev.platform_time,
        observed_at_unix_ms: ev.observed_at_unix_ms,
        source: match ev.source {
            EventSource::Recv => "recv".into(),
            EventSource::Update => "update".into(),
            EventSource::HistoryBackfill => "history_backfill".into(),
        },
    }
}

fn caligo_target(v: &serde_json::Value) -> caligo_model::SessionKey {
    let account = v.get("account").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let peer = v.get("peer").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let kind = match v.get("kind").and_then(|x| x.as_str()) {
        Some("group") => caligo_model::SessionKind::Group,
        _ => caligo_model::SessionKind::Private,
    };
    caligo_model::SessionKey::new(account, kind, peer)
}

/// 控制客户端(LAB 测试用):连接、握手与请求/应答。
pub struct ControlClient {
    pub conn: PipeConnection,
}

impl ControlClient {
    pub fn connect(name: &str, auth_token: &str, client_build: &str) -> Result<Self, SessionError> {
        let conn = connect_client(name).map_err(SessionError::Transport)?;
        send_json(&conn, &ControlMsg::Hello {
            protocol_version: PROTOCOL_VERSION_V2,
            client_build: client_build.to_string(),
            auth_token: auth_token.to_string(),
            role: Role::Control,
        })?;
        // 消费并核对 HelloAck(否则它会被当作第一条请求的应答)。
        match recv_json::<CoreToControlMsg>(&conn, &AtomicBool::new(false))? {
            CoreToControlMsg::HelloAck { accepted, reject_reason, .. } => {
                if !accepted {
                    return Err(SessionError::Frame(format!(
                        "hello rejected: {}",
                        reject_reason.unwrap_or_default()
                    )));
                }
            }
            other => {
                return Err(SessionError::Frame(format!(
                    "expected hello_ack, got {other:?}"
                )));
            }
        }
        Ok(Self { conn })
    }

    pub fn request(&mut self, msg: ControlMsg) -> Result<CoreToControlMsg, SessionError> {
        send_json(&self.conn, &msg)?;
        recv_json(&self.conn, &AtomicBool::new(false))
    }
}

/// bridge 侧 LAB 客户端(真机由 resident+worker 驱动;测试用同一管道语义)。
pub struct BridgeClient {
    pub conn: PipeConnection,
}

impl BridgeClient {
    pub fn connect(
        name: &str,
        auth_token: &str,
        bridge_build: &str,
        session_generation: u64,
        account: &str,
        module_baseline: &str,
    ) -> Result<Self, SessionError> {
        let conn = connect_client(name).map_err(SessionError::Transport)?;
        send_json(&conn, &BridgeMsg::Hello {
            protocol_version: PROTOCOL_VERSION_V2,
            bridge_build: bridge_build.to_string(),
            auth_token: auth_token.to_string(),
            role: Role::Bridge,
            session_generation,
            account: account.to_string(),
            module_baseline: module_baseline.to_string(),
        })?;
        // 消费并核对 HelloAck。
        match recv_json::<CoreToBridgeMsg>(&conn, &AtomicBool::new(false))? {
            CoreToBridgeMsg::HelloAck { accepted, reject_reason, .. } => {
                if !accepted {
                    return Err(SessionError::Frame(format!(
                        "hello rejected: {}",
                        reject_reason.unwrap_or_default()
                    )));
                }
            }
            other => {
                return Err(SessionError::Frame(format!(
                    "expected hello_ack, got {other:?}"
                )));
            }
        }
        Ok(Self { conn })
    }

    pub fn send(&self, msg: &BridgeMsg) -> Result<(), SessionError> {
        send_json(&self.conn, msg)
    }

    pub fn recv(&self) -> Result<CoreToBridgeMsg, SessionError> {
        recv_json(&self.conn, &AtomicBool::new(false))
    }
}
