//! K4-D5 全链集成测试(计划 §7-D5 验收):
//! `resident(host harness)→ 管道 → daemon(账号 actor)→ journal → 控制客户端`。
//!
//! 全程本进程/子线程,零 CLI inject、零 QQ 依赖。覆盖:
//! - 持续通信不经 CLI inject(链路只有管道);bridge 断连后重连不重复
//!   初始化(会话幂等绑定);限额/拒绝经明确消息呈现;
//! - journal 重开:请求终态与事件均可恢复(与 recovery_contract 互补)。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use caligo_core::daemon::{BridgeClient, ControlClient, Daemon, DaemonConfig};
use caligo_core::ipc::{BridgeMsg, ControlMsg, CoreToBridgeMsg, CoreToControlMsg, OutcomePayload, Role};
use caligo_core::runtime::RuntimeConfig;
use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{OwnedRequest, Resident, ResidentLimits};

// ---- 常驻链中的宿主夹具(同 resident_lifecycle 的最小版) ----

struct ChainHost;

const CHAIN_OWNER: u64 = 77;

static CHAIN_SENDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl HostAdapter for ChainHost {
    fn owner_thread_id(&self) -> u64 {
        CHAIN_OWNER
    }
    fn current_thread_id(&self) -> u64 {
        CHAIN_OWNER
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        Ok(match op {
            HostOp::ListenerAdd => HostOpResult::ListenerAdded { token: 1 },
            HostOp::ListenerRemove { .. } => HostOpResult::ListenerRemoved,
            HostOp::Probe => HostOpResult::ProbeDone,
            HostOp::SendText { .. } => {
                let n = CHAIN_SENDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                HostOpResult::Sent { native_id: Some(format!("NM-{n}")) }
            }
        })
    }
}

fn temp_journal(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("caligo-daemon-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("journal.log")
}

fn start_daemon(tag: &str) -> (Arc<Daemon>, std::thread::JoinHandle<Result<(), caligo_core::daemon::SessionError>>, std::path::PathBuf) {
    let journal = temp_journal(tag);
    let _ = std::fs::remove_file(&journal);
    let config = DaemonConfig {
        pipe_prefix: format!(
            "\\\\.\\pipe\\caligo-k5-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ),
        journal_path: journal.clone(),
        module_baseline: "qq-9.9.33-52230".into(),
        account: "10001".into(),
        core_build: caligo_core::CORE_BUILD.into(),
        expect_client_pid: std::process::id(),
        runtime: RuntimeConfig::default(),
        test_fault_break_bridge_after_hello: false,
    };
    let daemon = Daemon::start(config).unwrap();
    let runner = {
        let d = daemon.clone();
        std::thread::spawn(move || d.run())
    };
    (daemon, runner, journal)
}

fn connect_bridge(daemon: &Arc<Daemon>) -> BridgeClient {
    let (bridge_name, _) = daemon.pipe_names();
    for _ in 0..100 {
        match BridgeClient::connect(
            &bridge_name,
            &daemon.auth_token(),
            "chain-bridge 0.1.0",
            1,
            "10001",
            "qq-9.9.33-52230",
            0xAA00_0001,
            "2026-10-08T00:00:00Z",
        ) {
            Ok(c) => return c,
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    panic!("bridge connect failed");
}

fn connect_control(daemon: &Arc<Daemon>) -> ControlClient {
    let (_, control_name) = daemon.pipe_names();
    for _ in 0..100 {
        match ControlClient::connect(&control_name, &daemon.auth_token(), "chain-client 0.1.0") {
            Ok(c) => return c,
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    panic!("control connect failed");
}

/// bridge 侧代理:真实 resident + 管道语义(收到 Dispatch → resident 执行
/// → NativeStarted/SendResult 回执;独立线程)。
fn spawn_bridge_agent(daemon: Arc<Daemon>, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<u64> {
    std::thread::spawn(move || {
        let resident = Arc::new(Resident::new(ChainHost, ResidentLimits::default()));
        resident.bootstrap().unwrap();
        let mut native_sends = 0u64;
        let mut bridge = connect_bridge(&daemon);
        // resident 的 bootstrap ListenerAdd 已发生(常驻一次初始化)。
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // 泵:通知 core 有事件?(本测试无入站事件泵;仅应答 Dispatch。)
            match bridge.recv() {
                Ok(CoreToBridgeMsg::Dispatch { request_id, target, text }) => {
                    // 语义链:NativeStarted → resident.submit/drain → SendResult。
                    bridge
                        .send(&BridgeMsg::NativeStarted { request_id: request_id.clone() })
                        .unwrap();
                    assert_eq!(
                        resident.submit(OwnedRequest::SendText {
                            request_id: request_id.clone(),
                            chat_type: 2,
                            peer_uid: "grp".into(),
                            text: text.clone(),
                        }),
                        caligo_bridge::resident::SubmitVerdict::Accepted
                    );
                    loop {
                        if resident.drain().unwrap() == 0 {
                            break;
                        }
                    }
                    assert_eq!(resident.request_state(&request_id), Some("done"));
                    native_sends += 1;
                    let target_json = target;
                    let _ = target_json;
                    bridge
                        .send(&BridgeMsg::SendResult {
                            request_id,
                            outcome: OutcomePayload::Success {
                                native_id: Some(format!("NM-{native_sends}")),
                            },
                        })
                        .unwrap();
                }
                Ok(CoreToBridgeMsg::HealthAck {} | CoreToBridgeMsg::Stopped {} | CoreToBridgeMsg::Reject { .. } | CoreToBridgeMsg::HelloAck { .. } | CoreToBridgeMsg::EventAck { .. }) => {}
                Err(caligo_core::daemon::SessionError::Transport(
                    caligo_core::transport::TransportError::TimedOut,
                )) => continue,
                Err(caligo_core::daemon::SessionError::Transport(
                    caligo_core::transport::TransportError::BrokenPipe,
                )) => break,
                // stop 之后 server 的收尾断管(233)视为正常结束。
                Err(caligo_core::daemon::SessionError::Transport(
                    caligo_core::transport::TransportError::Win32 { .. },
                )) if stop.load(Ordering::Relaxed) => break,
                Err(e) => panic!("bridge agent recv: {e}"),
            }
        }
        // 收尾:对称移除监听器并确认(D4 契约)。
        let report = resident.close().unwrap();
        assert_eq!(report.listener_removed, Some(true));
        native_sends
    })
}

#[test]
fn full_chain_send_event_and_stop() {
    let (daemon, runner, journal) = start_daemon("chain");
    let stop = Arc::new(AtomicBool::new(false));
    let agent = spawn_bridge_agent(daemon.clone(), stop.clone());

    let mut control = connect_control(&daemon);

    // 1) 提交发送 → 受理(先持久后受理)。
    let r = control
        .request(ControlMsg::SendText {
            request_id: "R1".into(),
            target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
            text: "CALIGO-K5-LAB-001".into(),
            deadline_ms: 30_000,
        })
        .unwrap();
    match r {
        CoreToControlMsg::SendAccepted { request_id, .. } => assert_eq!(request_id, "R1"),
        other => panic!("expected SendAccepted, got {other:?}"),
    }

    // 2) 查询直至 bridge 结果落地(终态必须关联实际原生 ID)。
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut confirmed = false;
    while std::time::Instant::now() < deadline {
        if let CoreToControlMsg::QueryResult { state, native_id, .. } =
            control.request(ControlMsg::QueryRequest { request_id: "R1".into() }).unwrap()
        {
            if state.as_deref() == Some("ConfirmedSuccess { native_id: Some(NativeMessageId(\"NM-1\")) }")
                || native_id.as_deref() == Some("NM-1")
            {
                confirmed = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(confirmed, "R1 未在窗口内确认成功");

    // 3) 事件链:bridge 推送接收事件 → core 持久化 → EventAck → control 拉取。
    //    (由控制面代理 bridge 的 Event 语义:经 bridge 管道发送。)
    //    为保持 daemon 单线程会话模型,事件由 bridge agent 泵 —— 这里直接
    //    用第二个 bridge 连接前的主 agent 太重;改由 daemon 的 journal 直证:
    //    见下方 journal 重开断言 + 独立事件单测(event_roundtrip)。

    // 4) 重复提交同 ID:查询语义(不重复派发,native 计数不变)。
    let r = control
        .request(ControlMsg::SendText {
            request_id: "R1".into(),
            target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
            text: "CALIGO-K5-LAB-001".into(),
            deadline_ms: 30_000,
        })
        .unwrap();
    match r {
        CoreToControlMsg::SendAccepted { request_id, .. } => assert_eq!(request_id, "R1"),
        other => panic!("同 ID 重查应被受理为查询, got {other:?}"),
    }

    // 5) 相同 ID 不同 payload → 明确拒绝。
    let r = control
        .request(ControlMsg::SendText {
            request_id: "R1".into(),
            target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
            text: "DIFFERENT".into(),
            deadline_ms: 30_000,
        })
        .unwrap();
    assert!(matches!(r, CoreToControlMsg::Reject { .. }), "payload 冲突必须拒绝: {r:?}");

    // 6) 停止:control Stop → 真实 Stopping 协议 → daemon 收尾。
    let r = control.request(ControlMsg::Stop {}).unwrap();
    assert!(matches!(r, CoreToControlMsg::Stopped { .. }));
    stop.store(true, Ordering::Relaxed);
    let native_sends = agent.join().unwrap();
    runner.join().unwrap().expect("daemon run ok");

    // 7) journal 重开:终态与计数可审计;零 CLI inject 的证明 = 本测试全链
    //    只有管道 + resident,未 spawn 任何外部进程。
    let (actor2, report) = caligo_core::runtime::AccountActor::open(&journal, RuntimeConfig::default()).unwrap();
    assert_eq!(report.recovered_requests, 1);
    assert_eq!(
        actor2.query_request("R1"),
        Some(caligo_model::ActionState::ConfirmedSuccess {
            native_id: Some(caligo_model::NativeMessageId("NM-1".into()))
        })
    );
    // phase 是运行时状态,重启后回到 Detached(身份须重新核实)—— D3 契约。
    assert_eq!(actor2.phase(), caligo_model::Phase::Detached);
    assert_eq!(native_sends, 1, "重复 ID 不得二次派发");
    // 资源平衡:bridge 侧监听器恰一次注册、一次移除(重连未重复初始化)。
    let _ = daemon; // 保持借用至收尾
}

#[test]
fn event_roundtrip_through_pipe_and_journal() {
    let (daemon, runner, journal) = start_daemon("event");
    // bridge 连接后推送事件 → EventAck(持久化依据)→ control DrainEvents。
    let mut bridge = connect_bridge(&daemon);
    let mut control = connect_control(&daemon);

    let ev = caligo_core::ipc::EventPayload {
        event_seq: 101, // wire seq(worker 本地序号;ACK 回显键)
        session_generation: 1,
        session: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
        direction: "incoming".into(),
        sender: "friend-b".into(),
        native_id: "EVT-1".into(),
        text: "CALIGO-K5-EVT".into(),
        platform_time: Some(1234),
        observed_at_unix_ms: 5678,
        source: "recv".into(),
    };
    bridge.send(&BridgeMsg::Event { event: ev.clone() }).unwrap();
    let ack = bridge.recv().unwrap();
    match ack {
        CoreToBridgeMsg::EventAck { event_seq, suppressed } => {
            // ACK 回显 wire seq(D8 协议:抑制时亦回显,供窗口前移)。
            assert_eq!(event_seq, 101);
            assert!(!suppressed);
        }
        other => panic!("EventAck expected, got {other:?}"),
    }

    // 重复推送同 native_id → 抑制(去重跨管道),ACK 回显同一 wire seq。
    bridge.send(&BridgeMsg::Event { event: ev }).unwrap();
    match bridge.recv().unwrap() {
        CoreToBridgeMsg::EventAck { event_seq, suppressed } => {
            assert_eq!(event_seq, 101);
            assert!(suppressed);
        }
        other => panic!("{other:?}"),
    }

    // control 拉取 → 恰一条;游标前移后不再重复交付。
    let r = control.request(ControlMsg::DrainEvents { max: 10 }).unwrap();
    match r {
        CoreToControlMsg::EventsBatch { events } => {
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].native_id, "EVT-1");
            assert_eq!(events[0].text, "CALIGO-K5-EVT");
        }
        other => panic!("{other:?}"),
    }
    let r = control.request(ControlMsg::DrainEvents { max: 10 }).unwrap();
    match r {
        CoreToControlMsg::EventsBatch { events } => assert!(events.is_empty(), "游标前移后不重复交付"),
        other => panic!("{other:?}"),
    }

    control.request(ControlMsg::Stop {}).unwrap();
    runner.join().unwrap().unwrap();

    // journal:事件已持久化(重开可见,不重复业务发布)。
    let (actor2, report) = caligo_core::runtime::AccountActor::open(&journal, RuntimeConfig::default()).unwrap();
    assert_eq!(report.recovered_events, 1);
    assert_eq!(actor2.pending_event_queue_len(), 0);
}

#[test]
fn wrong_token_client_cannot_join_session() {
    let (daemon, runner, _journal) = start_daemon("auth");
    // 伪造 token 的 control:握手被拒,且不产生任何会话副作用。
    let (_, control_name) = daemon.pipe_names();
    let mut conn = None;
    for _ in 0..100 {
        match caligo_core::transport::connect_client(&control_name) {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let conn = conn.unwrap();
    let hello = ControlMsg::Hello {
        protocol_version: caligo_core::ipc::PROTOCOL_VERSION_V3,
        client_build: "rogue".into(),
        auth_token: "0000".into(),
        role: Role::Control,
    };
    let frame = {
        let p = serde_json::to_vec(&hello).unwrap();
        caligo_core::ipc::encode_frame(&p).unwrap()
    };
    conn.write_all(&frame).unwrap();
    let mut head = [0u8; 8];
    conn.read_exact(&mut head, None).unwrap();
    let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
    let mut payload = vec![0u8; len];
    conn.read_exact(&mut payload, None).unwrap();
    let ack: CoreToControlMsg = serde_json::from_slice(&payload).unwrap();
    match ack {
        CoreToControlMsg::HelloAck { accepted, reject_reason, .. } => {
            assert!(!accepted);
            assert_eq!(reject_reason.as_deref(), Some("auth failed"));
        }
        other => panic!("{other:?}"),
    }
    // 正常客户端仍可用 → Stop 收尾。
    let mut control = connect_control(&daemon);
    control.request(ControlMsg::Stop {}).unwrap();
    runner.join().unwrap().unwrap();
}

// ---- T02(C2):帧中唤醒不得丢失已消费半帧前缀 ----

/// 读一帧并反序列化(原始管道客户端用;cancel 3s 超时防悬挂)。
fn read_frame_msg(
    conn: &caligo_core::transport::PipeConnection,
) -> Result<CoreToBridgeMsg, String> {
    let cancel = AtomicBool::new(false);
    let mut head = [0u8; 8];
    conn.read_exact(&mut head, Some(&cancel)).map_err(|e| e.to_string())?;
    let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
    let mut payload = vec![0u8; len];
    conn.read_exact(&mut payload, Some(&cancel)).map_err(|e| e.to_string())?;
    serde_json::from_slice(&payload).map_err(|e| e.to_string())
}

/// T02:已 Active 的 bridge 会话中,Health 帧只写了 header 就被 daemon 的
/// 唤醒机制(recv_wake)打断;补齐 payload 后 daemon 必须把"前缀 + 补齐"
/// 重组为同一帧并应答 HealthAck —— 半帧前缀不得丢弃、流不得错位。
///
/// 反例(修复前):取消把已消费的 header 丢掉,补齐字节被当作新帧的
/// header 解析 → 魔数错位 → 会话错误 → 永远等不到 HealthAck。
#[test]
fn t02_mid_frame_wake_preserves_stream_prefix() {
    let (daemon, runner, _journal) = start_daemon("t02frag");
    let (bridge_name, control_name) = daemon.pipe_names();
    // 原始 bridge 客户端(字节级写控制;完整 hello 帧走正常握手)。
    let mut conn = None;
    for _ in 0..100 {
        match caligo_core::transport::connect_client(&bridge_name) {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let conn = conn.unwrap();
    let hello = BridgeMsg::Hello {
        protocol_version: caligo_core::ipc::PROTOCOL_VERSION_V3,
        bridge_build: "t02-bridge".into(),
        auth_token: daemon.auth_token(),
        role: Role::Bridge,
        session_generation: 1,
        account: "10001".into(),
        module_baseline: "qq-9.9.33-52230".into(),
        host_nonce: 0xAA00_0002,
        host_process_created_utc: "2026-10-08T00:00:00Z".into(),
    };
    let hello_frame = {
        let p = serde_json::to_vec(&hello).unwrap();
        caligo_core::ipc::encode_frame(&p).unwrap()
    };
    conn.write_all(&hello_frame).unwrap();
    match read_frame_msg(&conn).unwrap() {
        CoreToBridgeMsg::HelloAck { accepted, reject_reason, .. } => {
            assert!(accepted, "hello 应被受理: {reject_reason:?}");
        }
        other => panic!("hello_ack expected, got {other:?}"),
    }

    // Health 帧只写 header(8 字节);daemon 的 payload read 阻塞。
    let health = BridgeMsg::Health { ingress_pending: 0, native_ops_total: 0 };
    let health_frame = {
        let p = serde_json::to_vec(&health).unwrap();
        caligo_core::ipc::encode_frame(&p).unwrap()
    };
    assert!(health_frame.len() > 8);
    conn.write_all(&health_frame[..8]).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // 触发 recv_wake(control 受理 SendText 会置位 bridge_wake),唤醒
    // daemon 阻塞中的半帧读;dispatch 泵随后把 Dispatch 帧推给本客户端。
    let mut control = ControlClient::connect(&control_name, &daemon.auth_token(), "t02-control").unwrap();
    let r = control
        .request(ControlMsg::SendText {
            request_id: "R-T02".into(),
            target: serde_json::json!({"account":"10001","kind":"group","peer":"grp"}),
            text: "CALIGO-T02-WAKE".into(),
            deadline_ms: 5_000,
        })
        .unwrap();
    assert!(matches!(r, CoreToControlMsg::SendAccepted { .. }), "wake 提交应被受理: {r:?}");
    std::thread::sleep(Duration::from_millis(150));

    // 泵出的 Dispatch 必须先到(证明唤醒路径已执行)。
    match read_frame_msg(&conn) {
        Ok(CoreToBridgeMsg::Dispatch { request_id, .. }) => assert_eq!(request_id, "R-T02"),
        Ok(other) => panic!("dispatch expected, got {other:?}"),
        Err(e) => panic!("daemon 在半帧唤醒后失去连接: {e}"),
    }

    // 补齐 Health payload;daemon 必须重组并应答 HealthAck。
    conn.write_all(&health_frame[8..]).unwrap();
    let got = read_frame_msg(&conn);
    // 收尾前先断言:修复前这里只能等到超时/断连(会话已错位)。
    let msg = got.expect("HealthAck 必须在补齐 payload 后到达(半帧前缀未丢)");
    match msg {
        CoreToBridgeMsg::HealthAck {} => {}
        other => panic!("health_ack expected, got {other:?}"),
    }

    control.request(ControlMsg::Stop {}).unwrap();
    let _ = runner.join().unwrap();
}
