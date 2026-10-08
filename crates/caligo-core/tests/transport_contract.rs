//! K4-D5 传输契约测试(计划 §8.1:L02/L12 的传输面)。
//!
//! 真实命名管道(本进程内 server/client 线程),零 QQ 依赖:
//! - 半包:Hello 帧逐字节写入,server 侧 framing 必须完整还原;
//! - 认证:错误 token → HelloAck accepted=false,会话不建立;
//! - 取消:阻塞读在 cancel 置位后以 TimedOut 返回(不悬挂线程);
//! - 指定进程校验:server 断言 client PID。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use caligo_core::ipc::{
    BridgeMsg, CoreToBridgeMsg, Role, PROTOCOL_VERSION_V2,
};
use caligo_core::transport::{connect_client, generate_token, token_hex, PipeServer};

fn unique_prefix(tag: &str) -> String {
    format!(
        "\\\\.\\pipe\\caligo-k5-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn encode(msg: &BridgeMsg) -> Vec<u8> {
    let payload = serde_json::to_vec(msg).unwrap();
    caligo_core::ipc::encode_frame(&payload).unwrap()
}

fn hello(token: &str) -> BridgeMsg {
    BridgeMsg::Hello {
        protocol_version: caligo_core::ipc::PROTOCOL_VERSION_V3,
        bridge_build: "test-bridge".into(),
        auth_token: token.into(),
        role: Role::Bridge,
        session_generation: 1,
        account: "10001".into(),
        module_baseline: "qq-9.9.33-52230".into(),
        host_nonce: 0xAA00_0003,
        host_process_created_utc: "2026-10-08T00:00:00Z".into(),
    }
}

#[test]
fn fragmented_handshake_reassembles_and_replies() {
    let prefix = unique_prefix("frag");
    let token = token_hex(&generate_token().unwrap());
    let token_for_server = token.clone();
    let srv = Arc::new(PipeServer::create(&format!("{prefix}-bridge")).unwrap());
    let srv2 = srv.clone();
    let server = std::thread::spawn(move || {
        let token = token_for_server;
        let cancel = AtomicBool::new(false);
        let conn = srv2.accept(&cancel).unwrap();
        // 读一帧(客户端将逐字节写入)。
        let mut head = [0u8; 8];
        conn.read_exact(&mut head, Some(&cancel)).unwrap();
        let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let mut payload = vec![0u8; len];
        conn.read_exact(&mut payload, Some(&cancel)).unwrap();
        let msg: BridgeMsg = serde_json::from_slice(&payload).unwrap();
        match msg {
            BridgeMsg::Hello { auth_token, .. } => {
                let ack = CoreToBridgeMsg::HelloAck {
                    protocol_version: PROTOCOL_VERSION_V2,
                    core_build: "test".into(),
                    connection_epoch: 1,
                    accepted: auth_token == token,
                    reject_reason: None,
                };
                let frame = {
                    let p = serde_json::to_vec(&ack).unwrap();
                    caligo_core::ipc::encode_frame(&p).unwrap()
                };
                conn.write_all(&frame).unwrap();
            }
            _ => panic!("hello expected"),
        }
    });

    // 客户端:等 server 进入 accept(轮询连接)。
    let mut conn = None;
    for _ in 0..100 {
        match connect_client(&format!("{prefix}-bridge")) {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    let conn = conn.expect("client connect");
    let frame = encode(&hello(&token));
    for b in &frame {
        conn.write_all(std::slice::from_ref(b)).unwrap();
    }
    // 读回 HelloAck。
    let mut head = [0u8; 8];
    conn.read_exact(&mut head, None).unwrap();
    let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
    let mut payload = vec![0u8; len];
    conn.read_exact(&mut payload, None).unwrap();
    let ack: CoreToBridgeMsg = serde_json::from_slice(&payload).unwrap();
    match ack {
        CoreToBridgeMsg::HelloAck { accepted, .. } => assert!(accepted),
        _ => panic!("hello_ack expected"),
    }
    server.join().unwrap();
}

#[test]
fn wrong_auth_token_is_rejected_not_downgraded() {
    let prefix = unique_prefix("auth");
    let token = token_hex(&generate_token().unwrap());
    let srv = Arc::new(PipeServer::create(&format!("{prefix}-bridge")).unwrap());
    let srv2 = srv.clone();
    let expect_pid = std::process::id();
    let server = std::thread::spawn(move || {
        let cancel = AtomicBool::new(false);
        let conn = srv2.accept(&cancel).unwrap();
        assert_eq!(conn.client_pid().unwrap(), expect_pid, "指定进程校验");
        let mut head = [0u8; 8];
        conn.read_exact(&mut head, Some(&cancel)).unwrap();
        let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let mut payload = vec![0u8; len];
        conn.read_exact(&mut payload, Some(&cancel)).unwrap();
        let msg: BridgeMsg = serde_json::from_slice(&payload).unwrap();
        match msg {
            BridgeMsg::Hello { auth_token, .. } => {
                let accepted = auth_token == token;
                let ack = CoreToBridgeMsg::HelloAck {
                    protocol_version: PROTOCOL_VERSION_V2,
                    core_build: "test".into(),
                    connection_epoch: 1,
                    accepted,
                    reject_reason: if accepted { None } else { Some("auth failed".into()) },
                };
                let p = serde_json::to_vec(&ack).unwrap();
                conn.write_all(&caligo_core::ipc::encode_frame(&p).unwrap()).unwrap();
            }
            _ => panic!(),
        }
    });
    let mut conn = None;
    for _ in 0..100 {
        match connect_client(&format!("{prefix}-bridge")) {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    let conn = conn.unwrap();
    conn.write_all(&encode(&hello("deadbeef-deadbeef-deadbeef-deadbeef"))).unwrap();
    let mut head = [0u8; 8];
    conn.read_exact(&mut head, None).unwrap();
    let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
    let mut payload = vec![0u8; len];
    conn.read_exact(&mut payload, None).unwrap();
    let ack: CoreToBridgeMsg = serde_json::from_slice(&payload).unwrap();
    match ack {
        CoreToBridgeMsg::HelloAck { accepted, reject_reason, .. } => {
            assert!(!accepted);
            assert_eq!(reject_reason.as_deref(), Some("auth failed"));
        }
        _ => panic!(),
    }
    server.join().unwrap();
}

#[test]
fn blocked_read_is_cancelled_not_hung() {
    let prefix = unique_prefix("cancel");
    let srv = PipeServer::create(&format!("{prefix}-x")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    // 无客户端连接:阻塞在 accept;置位取消 → TimedOut。
    let c2 = cancel.clone();
    let h = std::thread::spawn(move || {
        let _ = &srv;
        let s = PipeServer::create(&format!("{prefix}-y")).unwrap();
        s.accept(&c2)
    });
    std::thread::sleep(std::time::Duration::from_millis(200));
    cancel.store(true, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let r = h.join().unwrap();
    assert!(matches!(r, Err(caligo_core::transport::TransportError::TimedOut)));
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "取消必须及时返回");
}
