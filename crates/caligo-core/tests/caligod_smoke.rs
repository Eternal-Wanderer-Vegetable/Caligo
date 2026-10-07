//! K4-D5 caligod 二进制冒烟测试:子进程启动 → stdout 首行取管道名与
//! token → control 连接 → Health → Stop → 退出码 0(真实 Stopping 协议)。

use std::io::BufRead;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn caligod_binary_runs_stops_cleanly() {
    let bin = env!("CARGO_BIN_EXE_caligod");
    let tag = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let journal_dir = std::env::temp_dir().join(format!("caligo-caligod-{tag}"));
    std::fs::create_dir_all(&journal_dir).unwrap();
    let journal = journal_dir.join("journal.log");
    let _ = std::fs::remove_file(&journal);

    let mut child = Command::new(bin)
        .args([
            "--pipe-prefix",
            &format!("\\\\.\\pipe\\caligo-k5-smoke-{tag}"),
            "--journal",
            journal.to_str().unwrap(),
            "--baseline",
            "qq-9.9.33-52230",
            "--account",
            "10001",
            "--expect-pid",
            &std::process::id().to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn caligod");

    // 首行 JSON:管道名 + token。
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read startup line");
    let info: serde_json::Value = serde_json::from_str(line.trim()).expect("startup json");
    let control_pipe = info["control_pipe"].as_str().expect("control_pipe").to_string();
    let auth = info["auth_token"].as_str().expect("auth_token").to_string();

    // 连接 control(轮询等待 server 就绪)。
    let mut client = None;
    for _ in 0..150 {
        match caligo_core::daemon::ControlClient::connect(&control_pipe, &auth, "smoke-client") {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut client = client.expect("control connect");

    // Health 探测 + Stop。
    let r = client.request(caligo_core::ipc::ControlMsg::Health {}).unwrap();
    assert!(matches!(r, caligo_core::ipc::CoreToControlMsg::HealthAck {}));
    let r = client.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    assert!(matches!(r, caligo_core::ipc::CoreToControlMsg::Stopped { .. }));

    // 等待退出:退出码 0 = 真实 Stopping 协议收尾。
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait().unwrap() {
            Some(s) => break s,
            None if std::time::Instant::now() > deadline => panic!("caligod did not exit"),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    assert_eq!(status.code(), Some(0), "clean stop must exit 0");
    let _ = reader;
    let _ = std::fs::remove_dir_all(&journal_dir);
}
