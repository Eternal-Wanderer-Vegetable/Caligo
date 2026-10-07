//! K4-D7b daemon_client LAB 集成测试(research 构建)。
//!
//! 真实 caligod daemon(core)+ 真实 bridge worker(管道客户端)对接:
//! - 连接失败 → 退避重试 → 服务端就绪后接入(重连计数,**零 bootstrap 调用**);
//! - Dispatch 全链:NativeStarted → resident 提交 → owner drain → SendResult
//!   → actor 终态关联原生 ID → 控制面可查;
//! - Stopped → worker 干净退出。
#![cfg(feature = "research")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use caligo_bridge::daemon_client::{run_worker, DaemonClientConfig, WorkerExit};
use caligo_bridge::host_adapter::{HostAdapter, HostOp, HostOpResult};
use caligo_bridge::resident::{Resident, ResidentLimits};

use caligo_core::daemon::{ControlClient, Daemon, DaemonConfig};
use caligo_core::ipc::CoreToControlMsg;
use caligo_core::runtime::RuntimeConfig;

// ---- LAB 假宿主(owner = 测试线程;Sent 返回递增原生 ID) ----

static LAB_SENDS: AtomicU64 = AtomicU64::new(0);

/// 两用例共享进程级全局计数器与计时敏感的管道时序:串行执行消除
/// 并行互扰(单测内语义已各自覆盖;串行不减弱断言)。
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct LabHost;

impl HostAdapter for LabHost {
    fn owner_thread_id(&self) -> u64 {
        lab_tid()
    }
    fn current_thread_id(&self) -> u64 {
        lab_tid()
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
        Ok(match op {
            HostOp::ListenerAdd => HostOpResult::ListenerDeferred,
            HostOp::ListenerRemove { .. } => HostOpResult::ListenerRemoved,
            HostOp::Probe => HostOpResult::ProbeDone,
            HostOp::SendText { .. } => {
                let n = LAB_SENDS.fetch_add(1, Ordering::Relaxed) + 1;
                HostOpResult::Sent { native_id: Some(format!("NM-{n}")) }
            }
        })
    }
}

fn lab_tid() -> u64 {
    // SAFETY: 无副作用。
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
}

fn unique_prefix(tag: &str) -> String {
    format!(
        "\\\\.\\pipe\\caligo-k5-dcl-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

/// start 不创建管道(run 才创建):重连用例借此制造"服务端未就绪"窗口。
fn start_daemon(
    prefix: &str,
) -> (Arc<Daemon>, impl FnOnce() -> std::thread::JoinHandle<Result<(), caligo_core::daemon::SessionError>>) {
    let dir = std::env::temp_dir().join(format!("caligo-dcl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = DaemonConfig {
        pipe_prefix: prefix.to_string(),
        journal_path: dir.join("journal.log"),
        module_baseline: "qq-9.9.33-52230".into(),
        account: "10001".into(),
        core_build: caligo_core::CORE_BUILD.into(),
        expect_client_pid: 0, // LAB worker 与测试同进程;PID 校验在此禁用
        runtime: RuntimeConfig::default(),
    };
    {
        let probe = dir.join("probe-write.txt");
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&probe)
        {
            Ok(_) => eprintln!("[dbg] probe open OK: {}", probe.display()),
            Err(e) => eprintln!("[dbg] probe open FAILED: {e}; path={}", probe.display()),
        }
    }
    let journal_dbg = config.journal_path.clone();
    let daemon = match Daemon::start(config) {
        Ok(d) => d,
        Err(e) => panic!(
            "daemon start failed: {e}; dir={:?} exists={} journal={:?}",
            dir,
            dir.exists(),
            journal_dbg
        ),
    };
    (
        daemon.clone(),
        move || {
            std::thread::spawn(move || daemon.run())
        },
    )
}

fn worker_config(prefix: &str, auth: &str) -> DaemonClientConfig {
    DaemonClientConfig {
        pipe_name: format!("{prefix}-bridge"),
        auth_token: auth.into(),
        bridge_build: "lab-bridge 0.1.0".into(),
        session_generation: 1,
        account: "10001".into(),
        module_baseline: "qq-9.9.33-52230".into(),
        heartbeat_ms: 500,
        reconnect_backoff_ms: 50,
    }
}

#[test]
fn reconnect_then_hello_then_clean_stop_without_bootstrap() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("reconnect");
    // daemon start(journal 就绪,管道未创建)→ worker 先行 → 服务端延迟就绪。
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let (w_tx, w_rx) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(cfg, resident, &wake_false, _rx_placeholder(), _ev_placeholder(), stop);
            w_tx.send(r).unwrap();
        });
    }
    // 服务端 300ms 后就绪(worker 此间经历连接失败/退避重试)。
    std::thread::sleep(Duration::from_millis(300));
    let runner = make_runner();

    let deadline = Instant::now() + Duration::from_secs(5);
    // worker 计数在退出时才返回;接入成功以 control 可用为证。
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = None;
    while Instant::now() < deadline {
        match ControlClient::connect(&control_name, &auth, "lab-client") {
            Ok(c) => {
                control = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut control = control.expect("control connected(worker 已接入)");

    // Health 往返(worker 心跳在跑,daemon 活着)。
    let r = control.request(caligo_core::ipc::ControlMsg::Health {}).unwrap();
    assert!(matches!(r, CoreToControlMsg::HealthAck {}));

    // Stop:daemon 真实停止协议 → worker 收 Stopped → 干净退出。
    let r = control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    assert!(matches!(r, CoreToControlMsg::Stopped { .. }));
    let (counters, exit) = match w_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(v) => v,
        Err(_) => {
            let snap = caligo_bridge::daemon_client::counters_snapshot();
            panic!("worker exit timeout; snapshot={snap:?}");
        }
    };
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert!(counters.hellos_accepted >= 1, "至少一次接入成功");
    assert!(
        counters.connect_failures + counters.reconnects >= 1,
        "服务端未就绪期应有失败/重连"
    );
    assert_eq!(
        counters.bootstraps_attempted, 0,
        "重连绝不重新 bootstrap —— 核心判据"
    );
}

#[test]
fn dispatch_roundtrip_through_real_worker() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("dispatch");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    // 真实流程中由 caligo_qq_daemon_client_start 调用;LAB 显式执行
    // (owner = 测试线程,与 drain 一致)。
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<caligo_bridge::resident::SendOutcome>();
    let (w_tx, w_rx) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(cfg, resident, &wake_true, rx, _ev_placeholder(), stop);
            w_tx.send(r).unwrap();
        });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match ControlClient::connect(&control_name, &auth, "lab-client") {
            Ok(c) => {
                control = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut control = control.expect("control connected");

    // 控制面提交发送 → daemon Dispatch → worker 提交 resident。
    // 会话绑定由 worker 的 Hello 完成;等待 SendText 被受理(worker 接入前
    // daemon 尚无 session,重试窗口内受理即成功)。
    let mut accepted = false;
    let acc_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < acc_deadline {
        let r = control
            .request(caligo_core::ipc::ControlMsg::SendText {
                request_id: "R1".into(),
                target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
                text: "CALIGO-K5-DCL-001".into(),
                deadline_ms: 30_000,
            })
            .unwrap();
        if matches!(r, CoreToControlMsg::SendAccepted { .. }) {
            accepted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(accepted, "SendText 未在窗口内被受理");

    // 测试线程 = owner:轮询提交 → drain → 结果回传(模拟 drain 钩子接线)。
    let round_deadline = Instant::now() + Duration::from_secs(5);
    let mut confirmed = false;
    while Instant::now() < round_deadline {
        // resident.drain 只能在 owner 线程(LAB 假宿主以本线程为 owner)。
        let _ = resident.drain();
        for outcome in resident.take_results() {
            let _ = tx.send(outcome);
        }
        if let CoreToControlMsg::QueryResult { state, native_id, .. } =
            control.request(caligo_core::ipc::ControlMsg::QueryRequest { request_id: "R1".into() }).unwrap()
        {
            if native_id.as_deref() == Some("NM-1") || state.as_deref().map(|s| s.contains("ConfirmedSuccess")).unwrap_or(false) {
                confirmed = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        confirmed,
        "R1 未在窗口内确认成功(NM-1); snapshot={:?}",
        caligo_bridge::daemon_client::counters_snapshot()
    );

    // 收尾:Stop → worker CoreStopped。
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    let (counters, exit) = match w_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(v) => v,
        Err(_) => {
            let snap = caligo_bridge::daemon_client::counters_snapshot();
            panic!("worker exit timeout; snapshot={snap:?}");
        }
    };
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert!(counters.dispatches >= 1);
    assert!(counters.results_sent >= 1);
    assert_eq!(counters.bootstraps_attempted, 0);
}

fn wake_false() -> bool {
    false
}

fn wake_true() -> bool {
    true
}

fn pipe_names_from_prefix(prefix: &str) -> (String, String) {
    (format!("{prefix}-bridge"), format!("{prefix}-control"))
}

// run_worker 的 rx 参数占位(第一个用例不消费结果)。
fn _rx_placeholder() -> mpsc::Receiver<caligo_bridge::resident::SendOutcome> {
    let (_t, r) = mpsc::channel();
    r
}

fn _ev_placeholder() -> mpsc::Receiver<caligo_bridge::resident::OwnedEvent> {
    let (_t, r) = mpsc::channel();
    r
}
