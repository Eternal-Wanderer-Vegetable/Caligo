//! IPC 契约测试(K1 范围):半包、版本、基线、账号、限额。
//!
//! 对应计划 §10 `crates/caligo-core/tests/ipc_contract.rs`(K1-K4 逐阶段扩充;
//! K4 加入队列满、迟到回执、core 崩溃重连等场景)。

use caligo_core::ipc::{
    encode_frame, validate_hello, FrameDecoder, FrameError, HandshakeExpectation, Hello, HelloAck,
    MAX_FRAME_SIZE, PROTOCOL_VERSION,
};
use caligo_core::CORE_BUILD;

fn sample_hello() -> Hello {
    Hello {
        protocol_version: PROTOCOL_VERSION,
        bridge_build: "caligo-bridge 0.1.0".into(),
        module_baseline: "qq-9.9.33-52230".into(),
        run_id: 7,
        account: "10001".into(),
        capabilities: vec!["probe-readonly".into()],
    }
}

fn expectation() -> HandshakeExpectation<'static> {
    HandshakeExpectation {
        protocol_version: PROTOCOL_VERSION,
        module_baseline: "qq-9.9.33-52230",
        account: None,
    }
}

#[test]
fn frame_roundtrip_preserves_payload() {
    let payload = b"hello caligo".to_vec();
    let frame = encode_frame(&payload).unwrap();
    let mut dec = FrameDecoder::new();
    let mut out = Vec::new();
    dec.push(&frame, &mut out).unwrap();
    assert_eq!(out, vec![payload]);
}

#[test]
fn half_frame_is_buffered_until_rest_arrives() {
    // T12 场景(半帧):先给一半,不应产出也不应报错;补齐后应产出完整帧。
    let frame = encode_frame(&[0xABu8; 64]).unwrap();
    let split = 5; // 落在帧头中间
    let mut dec = FrameDecoder::new();
    let mut out = Vec::new();
    dec.push(&frame[..split], &mut out).unwrap();
    assert!(out.is_empty());
    dec.push(&frame[split..], &mut out).unwrap();
    assert_eq!(out, [vec![0xABu8; 64]]);
}

#[test]
fn multiple_frames_in_one_chunk_are_all_decoded() {
    let a = encode_frame(b"first").unwrap();
    let b = encode_frame(b"second").unwrap();
    let mut dec = FrameDecoder::new();
    let mut out = Vec::new();
    let mut both = a;
    both.extend_from_slice(&b);
    dec.push(&both, &mut out).unwrap();
    assert_eq!(out, vec![b"first".to_vec(), b"second".to_vec()]);
}

#[test]
fn oversize_frame_is_rejected() {
    // T12 场景(限额):声明长度超过 1 MiB 立即报错,不无限缓冲。
    let mut frame = Vec::new();
    frame.extend_from_slice(&caligo_core::ipc::FRAME_MAGIC.to_le_bytes());
    frame.extend_from_slice(&((MAX_FRAME_SIZE + 1) as u32).to_le_bytes());
    let mut dec = FrameDecoder::new();
    let mut out = Vec::new();
    assert_eq!(
        dec.push(&frame, &mut out),
        Err(FrameError::FrameTooLarge(MAX_FRAME_SIZE + 1))
    );
}

#[test]
fn magic_mismatch_is_rejected() {
    let mut garbage = vec![0u8; 8];
    garbage[0] = 0xDE;
    let mut dec = FrameDecoder::new();
    let mut out = Vec::new();
    assert!(matches!(
        dec.push(&garbage, &mut out),
        Err(FrameError::MagicMismatch(_))
    ));
}

#[test]
fn handshake_accepts_matching_hello() {
    let ack = validate_hello(&sample_hello(), &expectation(), CORE_BUILD).unwrap();
    assert!(ack.accepted);
    assert_eq!(ack.run_id, 7);
    assert_eq!(ack.core_build, CORE_BUILD);
    assert_eq!(
        ack,
        HelloAck {
            protocol_version: PROTOCOL_VERSION,
            core_build: CORE_BUILD.into(),
            run_id: 7,
            accepted: true,
            reject_reason: None,
        }
    );
}

#[test]
fn handshake_rejects_wrong_protocol_version() {
    // T02 场景(版本不匹配):拒绝,不降级协商。
    let mut hello = sample_hello();
    hello.protocol_version = PROTOCOL_VERSION + 1;
    let err = validate_hello(&hello, &expectation(), CORE_BUILD).unwrap_err();
    assert!(matches!(
        err,
        caligo_core::ipc::RejectReason::ProtocolVersionMismatch { .. }
    ));
}

#[test]
fn handshake_rejects_wrong_module_baseline() {
    // T01 场景(模块摘要不匹配)的逻辑层:拒绝 attach,不猜偏移。
    let mut hello = sample_hello();
    hello.module_baseline = "qq-9.9.99-99999".into();
    let err = validate_hello(&hello, &expectation(), CORE_BUILD).unwrap_err();
    assert!(matches!(
        err,
        caligo_core::ipc::RejectReason::ModuleBaselineMismatch { .. }
    ));
}

#[test]
fn handshake_rejects_wrong_account_when_bound() {
    // T02 场景(账号不匹配):与其他账号隔离。
    let mut hello = sample_hello();
    hello.account = "99999".into();
    let expect = HandshakeExpectation {
        protocol_version: PROTOCOL_VERSION,
        module_baseline: "qq-9.9.33-52230",
        account: Some("10001"),
    };
    let err = validate_hello(&hello, &expect, CORE_BUILD).unwrap_err();
    assert!(matches!(
        err,
        caligo_core::ipc::RejectReason::AccountMismatch { .. }
    ));
}
