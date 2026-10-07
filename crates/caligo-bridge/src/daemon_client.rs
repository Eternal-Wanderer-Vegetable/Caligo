//! K4-D7b bridge 侧 daemon 客户端(仅 research 构建)。
//!
//! 把 daemon_chain 的 LAB 链延伸进 QQ 进程:常驻 worker 线程(自有线程,
//! 只做管道 I/O 与 resident 提交,**零 QQ/V8 API** —— 计划 §3.1)连接
//! caligod 的 bridge 管道,说 IPC v2:
//! - Hello(认证/基线/账号/会话代次)→ HelloAck;
//! - Dispatch → NativeStarted → `resident.submit(SendText)` → `wake()`
//!   (uv_async_send,唯一合法跨线程面)→ owner pump drain → 结果经
//!   result 通道回传 → SendResult;
//! - Event 上行(带未确认环形窗口;EventAck 前移;重连重放未确认项);
//! - 心跳 5s(3 次未响应即断开重连);重连退避 1/2/4…30s;
//! - **重连绝不重新 bootstrap**(`bootstraps_attempted` 恒 0,由计数证明);
//! - 认证/身份被拒 → 不可重试,立即退出(不暴力重连)。

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use caligo_model::framing::{encode_frame, FrameDecoder, FrameError};
use caligo_model::ipc_v2::{BridgeMsg, CoreToBridgeMsg, EventPayload, OutcomePayload, Role, PROTOCOL_VERSION_V2};

use crate::resident::{Resident, SendOutcome};

/// worker 配置。
#[derive(Debug, Clone)]
pub struct DaemonClientConfig {
    pub pipe_name: String,
    pub auth_token: String,
    pub bridge_build: String,
    pub session_generation: u64,
    pub account: String,
    pub module_baseline: String,
    /// 心跳间隔(计划 §6.4 建议 5s)。
    pub heartbeat_ms: u64,
    /// 重连退避起点(计划 §6.4:1/2/4…上限 30s)。
    pub reconnect_backoff_ms: u64,
}

impl Default for DaemonClientConfig {
    fn default() -> Self {
        Self {
            pipe_name: String::new(),
            auth_token: String::new(),
            bridge_build: concat!("caligo-bridge ", env!("CARGO_PKG_VERSION")).into(),
            session_generation: 0,
            account: String::new(),
            module_baseline: String::new(),
            heartbeat_ms: 5_000,
            reconnect_backoff_ms: 1_000,
        }
    }
}

/// worker 计数(资源与行为审计;`bootstraps_attempted` 恒 0 是重连判据)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WorkerCounters {
    pub connects: u64,
    pub connect_failures: u64,
    pub reconnects: u64,
    pub hellos_accepted: u64,
    pub hello_rejected: u64,
    pub dispatches: u64,
    pub results_sent: u64,
    pub events_sent: u64,
    pub events_acked: u64,
    pub events_gap_dropped: u64,
    pub heartbeats_sent: u64,
    pub heartbeat_timeouts: u64,
    pub protocol_errors: u64,
    pub clean_stops: u64,
    pub bootstraps_attempted: u64,
}

/// 进程级原子计数(worker 在 QQ 内运行,导出侧可随时取证)。
#[derive(Debug, Default)]
pub struct AtomicCounters {
    pub connects: AtomicU64,
    pub connect_failures: AtomicU64,
    pub reconnects: AtomicU64,
    pub hellos_accepted: AtomicU64,
    pub hello_rejected: AtomicU64,
    pub dispatches: AtomicU64,
    pub results_sent: AtomicU64,
    pub events_sent: AtomicU64,
    pub events_acked: AtomicU64,
    pub events_gap_dropped: AtomicU64,
    pub heartbeats_sent: AtomicU64,
    pub heartbeat_timeouts: AtomicU64,
    pub protocol_errors: AtomicU64,
    pub clean_stops: AtomicU64,
    pub bootstraps_attempted: AtomicU64,
}

static GLOBAL_COUNTERS: AtomicCounters = AtomicCounters {
    connects: AtomicU64::new(0),
    connect_failures: AtomicU64::new(0),
    reconnects: AtomicU64::new(0),
    hellos_accepted: AtomicU64::new(0),
    hello_rejected: AtomicU64::new(0),
    dispatches: AtomicU64::new(0),
    results_sent: AtomicU64::new(0),
    events_sent: AtomicU64::new(0),
    events_acked: AtomicU64::new(0),
    events_gap_dropped: AtomicU64::new(0),
    heartbeats_sent: AtomicU64::new(0),
    heartbeat_timeouts: AtomicU64::new(0),
    protocol_errors: AtomicU64::new(0),
    clean_stops: AtomicU64::new(0),
    bootstraps_attempted: AtomicU64::new(0),
};

/// 计数快照(导出侧取证;字段与顺序即 JSONL 输出顺序)。
pub fn counters_snapshot() -> WorkerCounters {
    let g = &GLOBAL_COUNTERS;
    WorkerCounters {
        connects: g.connects.load(Ordering::Relaxed),
        connect_failures: g.connect_failures.load(Ordering::Relaxed),
        reconnects: g.reconnects.load(Ordering::Relaxed),
        hellos_accepted: g.hellos_accepted.load(Ordering::Relaxed),
        hello_rejected: g.hello_rejected.load(Ordering::Relaxed),
        dispatches: g.dispatches.load(Ordering::Relaxed),
        results_sent: g.results_sent.load(Ordering::Relaxed),
        events_sent: g.events_sent.load(Ordering::Relaxed),
        events_acked: g.events_acked.load(Ordering::Relaxed),
        events_gap_dropped: g.events_gap_dropped.load(Ordering::Relaxed),
        heartbeats_sent: g.heartbeats_sent.load(Ordering::Relaxed),
        heartbeat_timeouts: g.heartbeat_timeouts.load(Ordering::Relaxed),
        protocol_errors: g.protocol_errors.load(Ordering::Relaxed),
        clean_stops: g.clean_stops.load(Ordering::Relaxed),
        bootstraps_attempted: g.bootstraps_attempted.load(Ordering::Relaxed),
    }
}

impl WorkerCounters {
    fn publish(&self) {
        let g = &GLOBAL_COUNTERS;
        g.connects.store(self.connects, Ordering::Relaxed);
        g.connect_failures.store(self.connect_failures, Ordering::Relaxed);
        g.reconnects.store(self.reconnects, Ordering::Relaxed);
        g.hellos_accepted.store(self.hellos_accepted, Ordering::Relaxed);
        g.hello_rejected.store(self.hello_rejected, Ordering::Relaxed);
        g.dispatches.store(self.dispatches, Ordering::Relaxed);
        g.results_sent.store(self.results_sent, Ordering::Relaxed);
        g.events_sent.store(self.events_sent, Ordering::Relaxed);
        g.events_acked.store(self.events_acked, Ordering::Relaxed);
        g.events_gap_dropped.store(self.events_gap_dropped, Ordering::Relaxed);
        g.heartbeats_sent.store(self.heartbeats_sent, Ordering::Relaxed);
        g.heartbeat_timeouts.store(self.heartbeat_timeouts, Ordering::Relaxed);
        g.protocol_errors.store(self.protocol_errors, Ordering::Relaxed);
        g.clean_stops.store(self.clean_stops, Ordering::Relaxed);
        g.bootstraps_attempted.store(self.bootstraps_attempted, Ordering::Relaxed);
    }

    pub fn as_json(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("{\"connects\":"); out.push_str(&self.connects.to_string());
        out.push_str(",\"connect_failures\":"); out.push_str(&self.connect_failures.to_string());
        out.push_str(",\"reconnects\":"); out.push_str(&self.reconnects.to_string());
        out.push_str(",\"hellos_accepted\":"); out.push_str(&self.hellos_accepted.to_string());
        out.push_str(",\"hello_rejected\":"); out.push_str(&self.hello_rejected.to_string());
        out.push_str(",\"dispatches\":"); out.push_str(&self.dispatches.to_string());
        out.push_str(",\"results_sent\":"); out.push_str(&self.results_sent.to_string());
        out.push_str(",\"events_sent\":"); out.push_str(&self.events_sent.to_string());
        out.push_str(",\"events_acked\":"); out.push_str(&self.events_acked.to_string());
        out.push_str(",\"events_gap_dropped\":"); out.push_str(&self.events_gap_dropped.to_string());
        out.push_str(",\"heartbeats_sent\":"); out.push_str(&self.heartbeats_sent.to_string());
        out.push_str(",\"heartbeat_timeouts\":"); out.push_str(&self.heartbeat_timeouts.to_string());
        out.push_str(",\"protocol_errors\":"); out.push_str(&self.protocol_errors.to_string());
        out.push_str(",\"clean_stops\":"); out.push_str(&self.clean_stops.to_string());
        out.push_str(",\"bootstraps_attempted\":"); out.push_str(&self.bootstraps_attempted.to_string());
        out.push('}');
        out
    }
}

/// worker 退出原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerExit {
    /// 收到 Stopped(core 侧真实停止)。
    CoreStopped,
    /// stop 标志置位(本侧关闭协议)。
    LocalStop,
    /// Hello 被拒(认证/身份):不可重试。
    HelloRejected,
}

/// 上行事件未确认窗口:满(项数或字节)→ 记 Gap 丢最旧(计数如实)。
const EVENT_WINDOW_MAX_ITEMS: usize = 256;
const EVENT_WINDOW_MAX_BYTES: usize = 8 * 1024 * 1024;

/// 最小管道客户端:OVERLAPPED 读写字节模式管道(与 core::transport 同语义;
/// bridge 不能依赖 core —— 独立实现,CLG1 帧取自 model)。
struct PipeConn {
    handle: *mut core::ffi::c_void,
}
// SAFETY: 管道句柄非线程亲和;worker 单线程独占使用。
unsafe impl Send for PipeConn {}

#[derive(Debug)]
enum PipeError {
    /// 附操作名与 Win32 错误码(计数器/日志用)。
    Win32 {
        op: &'static str,
        code: u32,
    },
    TimedOut,
    BrokenPipe,
}
impl PipeError {
    #[allow(dead_code)]
    fn describe(&self) -> String {
        match self {
            PipeError::Win32 { op, code } => format!("{op}: Win32 error {code}"),
            PipeError::TimedOut => "timed out".into(),
            PipeError::BrokenPipe => "pipe broken".into(),
        }
    }
}

impl PipeConn {
    fn connect(name: &str) -> Result<Self, PipeError> {
        // 短名自动补 Win32 管道根(与 core::daemon::pipe_names 同规则)——
        // D7-b 现场教训:配置里的短名在 QQ 进程内解析失败,worker 永远
        // 退避重连一个不存在的名字。
        const ROOT: &str = "\\\\.\\pipe\\";
        let name = if name.starts_with(ROOT) {
            name.to_string()
        } else {
            format!("{ROOT}{}", name.trim_start_matches('\\'))
        };
        let wide: Vec<u16> = OsStr::new(&name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: name 在调用期间有效;句柄由 self 管理。
        let handle = unsafe {
            windows_sys::Win32::Storage::FileSystem::CreateFileW(
                wide.as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE,
                windows_sys::Win32::Storage::FileSystem::FILE_SHARE_NONE,
                std::ptr::null(),
                windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING,
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err(PipeError::Win32 {
                op: "CreateFileW",
                // SAFETY: GetLastError 无副作用。
                code: unsafe { windows_sys::Win32::Foundation::GetLastError() },
            });
        }
        Ok(Self { handle })
    }

    fn read_some(&self, buf: &mut [u8], cancel: &AtomicBool) -> Result<usize, PipeError> {
        // SAFETY: OVERLAPPED/事件在本函数内创建并关闭;取消后回收。
        unsafe {
            let event = windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
            );
            if event.is_null() || event == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                return Err(PipeError::Win32 { op: "CreateEventW", code: windows_sys::Win32::Foundation::GetLastError() });
            }
            let mut ov: windows_sys::Win32::System::IO::OVERLAPPED = std::mem::zeroed();
            ov.hEvent = event;
            let mut got = 0u32;
            let ok = windows_sys::Win32::Storage::FileSystem::ReadFile(
                self.handle,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut got,
                &mut ov,
            );
            if ok == 0 {
                let e = windows_sys::Win32::Foundation::GetLastError();
                if e != windows_sys::Win32::Foundation::ERROR_IO_PENDING {
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                        return Err(PipeError::BrokenPipe);
                    }
                    return Err(PipeError::Win32 { op: "ReadFile", code: e });
                }
            }
            loop {
                let mut transferred = 0u32;
                if windows_sys::Win32::System::IO::GetOverlappedResult(self.handle, &mut ov, &mut transferred, 0) != 0 {
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    return Ok(transferred as usize);
                }
                let e = windows_sys::Win32::Foundation::GetLastError();
                if e != windows_sys::Win32::Foundation::ERROR_IO_INCOMPLETE {
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                        return Err(PipeError::BrokenPipe);
                    }
                    return Err(PipeError::Win32 { op: "GetOverlappedResult", code: e });
                }
                let w = windows_sys::Win32::System::Threading::WaitForSingleObject(event, 100);
                if w == 0 {
                    continue;
                }
                if w == 0x00000102 {
                    // WAIT_TIMEOUT:轮询 cancel;置位 → 取消并回收(计划 §5.3)。
                    // 部分读必须交付(同 core::transport 的帧流撕毁修复)。
                    if cancel.load(Ordering::Relaxed) {
                        windows_sys::Win32::System::IO::CancelIoEx(self.handle, &mut ov);
                        let mut t = 0u32;
                        let ok = windows_sys::Win32::System::IO::GetOverlappedResult(self.handle, &mut ov, &mut t, 1);
                        windows_sys::Win32::Foundation::CloseHandle(event);
                        if ok != 0 && t > 0 {
                            return Ok(t as usize);
                        }
                        return Err(PipeError::TimedOut);
                    }
                    continue;
                }
                windows_sys::Win32::Foundation::CloseHandle(event);
                return Err(PipeError::Win32 { op: "WaitForSingleObject", code: windows_sys::Win32::Foundation::GetLastError() });
            }
        }
    }

    fn write_all(&self, mut bytes: &[u8]) -> Result<(), PipeError> {
        // SAFETY: 同 read_some(每次写独立 OVERLAPPED,写完即回收)。
        unsafe {
            while !bytes.is_empty() {
                let event = windows_sys::Win32::System::Threading::CreateEventW(
                    std::ptr::null(),
                    1,
                    0,
                    std::ptr::null(),
                );
                if event.is_null() || event == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                    return Err(PipeError::Win32 { op: "CreateEventW", code: windows_sys::Win32::Foundation::GetLastError() });
                }
                let mut ov: windows_sys::Win32::System::IO::OVERLAPPED = std::mem::zeroed();
                ov.hEvent = event;
                let mut wrote = 0u32;
                let ok = windows_sys::Win32::Storage::FileSystem::WriteFile(
                    self.handle,
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    &mut wrote,
                    &mut ov,
                );
                if ok == 0 {
                    let e = windows_sys::Win32::Foundation::GetLastError();
                    if e != windows_sys::Win32::Foundation::ERROR_IO_PENDING {
                        windows_sys::Win32::Foundation::CloseHandle(event);
                        if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                            return Err(PipeError::BrokenPipe);
                        }
                        return Err(PipeError::Win32 { op: "WriteFile", code: e });
                    }
                }
                let mut transferred = 0u32;
                if windows_sys::Win32::System::IO::GetOverlappedResult(self.handle, &mut ov, &mut transferred, 1) == 0 {
                    let e = windows_sys::Win32::Foundation::GetLastError();
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                        return Err(PipeError::BrokenPipe);
                    }
                    return Err(PipeError::Win32 { op: "GetOverlappedResult(write)", code: e });
                }
                windows_sys::Win32::Foundation::CloseHandle(event);
                bytes = &bytes[transferred as usize..];
            }
        }
        Ok(())
    }
}

impl Drop for PipeConn {
    fn drop(&mut self) {
        // SAFETY: 句柄唯一所有者。
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

fn send_json(conn: &PipeConn, msg: &BridgeMsg) -> Result<(), PipeError> {
    let payload = serde_json::to_vec(msg).map_err(|_| PipeError::BrokenPipe)?;
    let frame = encode_frame(&payload).map_err(|_| PipeError::BrokenPipe)?;
    conn.write_all(&frame)
}

/// 阻塞收一帧;`tick` 置位时以 TimedOut 返回(回收后可继续)。
fn recv_json(
    conn: &PipeConn,
    dec: &mut FrameDecoder,
    tick: &AtomicBool,
) -> Result<CoreToBridgeMsg, PipeError> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = conn.read_some(&mut buf, tick)?;
        if n == 0 {
            return Err(PipeError::BrokenPipe);
        }
        match dec.push(&buf[..n], &mut out) {
            Ok(()) => {}
            Err(FrameError::MagicMismatch(_) | FrameError::FrameTooLarge(_)) => {
                return Err(PipeError::BrokenPipe); // 流错位:断开重连
            }
        }
        if let Some(payload) = out.pop() {
            return serde_json::from_slice(&payload)
                .map_err(|_| PipeError::BrokenPipe);
        }
    }
}

/// worker 主循环(自有线程内运行)。`result_rx` 由 owner 侧 drain hook 供给;
/// `wake` = uv_async_send 包装。返回 (计数, 退出原因)。
pub fn run_worker<A: crate::host_adapter::HostAdapter>(
    cfg: DaemonClientConfig,
    resident: Arc<Resident<A>>,
    wake: &dyn Fn() -> bool,
    result_rx: mpsc::Receiver<SendOutcome>,
    stop: Arc<AtomicBool>,
) -> (WorkerCounters, WorkerExit) {
    let mut c = WorkerCounters::default();
    let mut backoff = cfg.reconnect_backoff_ms.max(100);
    macro_rules! publish {
        () => {
            c.publish();
        };
    }
    loop {
        publish!(); // 实时计数:导出侧快照随时可取证(退出时再终发布)
        if stop.load(Ordering::Relaxed) {
            publish!();
            return (c, WorkerExit::LocalStop);
        }
        // —— 连接 ——
        let conn = match PipeConn::connect(&cfg.pipe_name) {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("[dcl] connect failed: {}", e.describe());
                c.connect_failures += 1;
                if c.connects > 0 {
                    c.reconnects += 1;
                }
                std::thread::sleep(Duration::from_millis(backoff));
                backoff = (backoff * 2).min(30_000);
                continue;
            }
        };
        c.connects += 1;
        backoff = cfg.reconnect_backoff_ms.max(100);
        // —— Hello(身份/认证;被拒不重试)——
        if send_json(
            &conn,
            &BridgeMsg::Hello {
                protocol_version: PROTOCOL_VERSION_V2,
                bridge_build: cfg.bridge_build.clone(),
                auth_token: cfg.auth_token.clone(),
                role: Role::Bridge,
                session_generation: cfg.session_generation,
                account: cfg.account.clone(),
                module_baseline: cfg.module_baseline.clone(),
            },
        )
        .is_err()
        {
            c.connect_failures += 1;
            std::thread::sleep(Duration::from_millis(backoff));
            continue;
        }
        let mut dec = FrameDecoder::new();
        let tick = Arc::new(AtomicBool::new(false));
        // 心跳看门狗:100ms 置位 tick,使阻塞 recv 周期返回。
        let watchdog_stop = Arc::new(AtomicBool::new(false));
        {
            let tick = tick.clone();
            let wstop = watchdog_stop.clone();
            std::thread::spawn(move || {
                while !wstop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    tick.store(true, Ordering::Relaxed);
                }
            });
        }
        let session = session_loop(
            &conn,
            &mut dec,
            &cfg,
            &resident,
            wake,
            &result_rx,
            &tick,
            &stop,
            &mut c,
        );
        watchdog_stop.store(true, Ordering::Relaxed);
        match session {
            SessionEnd::CoreStopped => {
                c.clean_stops += 1;
                publish!();
                return (c, WorkerExit::CoreStopped);
            }
            SessionEnd::LocalStop => {
                publish!();
                return (c, WorkerExit::LocalStop);
            }
            SessionEnd::HelloRejected => {
                c.hello_rejected += 1;
                publish!();
                return (c, WorkerExit::HelloRejected);
            }
            SessionEnd::Broken => {
                c.reconnects += 1;
                std::thread::sleep(Duration::from_millis(backoff));
                backoff = (backoff * 2).min(30_000);
            }
        }
    }
}

enum SessionEnd {
    CoreStopped,
    LocalStop,
    HelloRejected,
    Broken,
}

#[allow(clippy::too_many_arguments)]
fn session_loop<A: crate::host_adapter::HostAdapter>(
    conn: &PipeConn,
    dec: &mut FrameDecoder,
    cfg: &DaemonClientConfig,
    resident: &Arc<Resident<A>>,
    wake: &dyn Fn() -> bool,
    result_rx: &mpsc::Receiver<SendOutcome>,
    tick: &Arc<AtomicBool>,
    stop: &Arc<AtomicBool>,
    c: &mut WorkerCounters,
) -> SessionEnd {
    // 未确认事件窗口(EventAck 前移;重连重放)。
    let mut unacked: VecDeque<(u64, EventPayload)> = VecDeque::new();
    let mut unacked_bytes: usize = 0;
    let mut next_local_seq: u64 = 1;
    let mut acked_high: u64 = 0;
    let mut last_heartbeat = Instant::now();
    let mut heartbeat_misses: u32 = 0;
    // resident 未就绪期的待提交队列(owner 初始化完成后补交)。
    let mut pending: VecDeque<(String, usize)> = VecDeque::new();

    // HelloAck(阻塞;被拒 → 不可重试)。
    loop {
        match recv_json(conn, dec, tick) {
            Ok(CoreToBridgeMsg::HelloAck { accepted, reject_reason, .. }) => {
                if accepted {
                    c.hellos_accepted += 1;
                    break;
                }
                let _ = reject_reason;
                return SessionEnd::HelloRejected;
            }
            Ok(_) => {
                c.protocol_errors += 1;
                return SessionEnd::HelloRejected;
            }
            Err(PipeError::TimedOut) => {
                if stop.load(Ordering::Relaxed) {
                    return SessionEnd::LocalStop;
                }
                continue;
            }
            Err(_) => return SessionEnd::Broken,
        }
    }

    loop {
        if stop.load(Ordering::Relaxed) {
            return SessionEnd::LocalStop;
        }
        c.publish(); // 实时计数(导出侧快照取证)
        // 0) 待提交补交(resident 就绪后)。
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
        // 1) 结果回传(Failure/Unknown/Success 都立即上报)。
        while let Ok(outcome) = result_rx.try_recv() {
            let msg = match outcome {
                SendOutcome::Success { request_id, native_id } => BridgeMsg::SendResult {
                    request_id,
                    outcome: OutcomePayload::Success { native_id },
                },
                SendOutcome::Failure { request_id, reason } => BridgeMsg::SendResult {
                    request_id,
                    outcome: OutcomePayload::Failure { reason },
                },
                SendOutcome::Unknown { request_id } => BridgeMsg::SendResult {
                    request_id,
                    outcome: OutcomePayload::Unknown,
                },
            };
            if send_json(conn, &msg).is_err() {
                return SessionEnd::Broken;
            }
            c.results_sent += 1;
        }
        // 2) 事件上行(重放未确认 → 新事件;窗口满 → 记 Gap 丢最旧)。
        let mut replay: Vec<u64> = unacked.iter().map(|(s, _)| *s).collect();
        replay.retain(|s| *s > acked_high);
        if !replay.is_empty() {
            let items: Vec<EventPayload> = unacked
                .iter()
                .filter(|(s, _)| *s > acked_high)
                .map(|(_, e)| e.clone())
                .collect();
            for e in items {
                if send_json(conn, &BridgeMsg::Event { event: e }).is_err() {
                    return SessionEnd::Broken;
                }
                c.events_sent += 1;
            }
        }
        for ev in resident.take_events() {
            // 方向判据:senderUin == 本账号 → SelfSent(不伪装 incoming);
            // 会话种类:chatType 1=private 2=group(opaque 透传;peer=peerUid)。
            let is_self = !cfg.account.is_empty() && ev.sender_uin == cfg.account;
            let payload = EventPayload {
                event_seq: 0, // core 持久化时分配;本地 seq 仅用于 ACK 关联
                session_generation: cfg.session_generation,
                session: serde_json::json!({
                    "account": cfg.account,
                    "kind": if ev.chat_type == 2 { "group" } else { "private" },
                    "peer": ev.peer_uid,
                }),
                direction: if is_self { "self_sent" } else { "incoming" }.into(),
                sender: ev.sender_uin.clone(),
                native_id: ev.native_id.clone(),
                text: ev.text.clone(),
                platform_time: ev.msg_time,
                observed_at_unix_ms: 0,
                source: match ev.source {
                    crate::resident::EventSourceKind::Recv => "recv",
                    crate::resident::EventSourceKind::Update => "update",
                }
                .into(),
            };
            let seq = next_local_seq;
            next_local_seq += 1;
            let bytes = payload.native_id.len() + 128;
            while unacked.len() + 1 > EVENT_WINDOW_MAX_ITEMS
                || unacked_bytes + bytes > EVENT_WINDOW_MAX_BYTES
            {
                if let Some((s, _)) = unacked.pop_front() {
                    unacked_bytes -= 64; // 近似回收;精确字节在 push 时累计
                    c.events_gap_dropped += 1;
                    let _ = s;
                }
            }
            unacked.push_back((seq, payload));
            unacked_bytes += bytes;
            if send_json(conn, &BridgeMsg::Event { event: unacked.back().unwrap().1.clone() }).is_err() {
                return SessionEnd::Broken;
            }
            c.events_sent += 1;
        }
        // 3) 心跳(3 次未响应 → 主动断开重连;计划 §6.4)。
        if last_heartbeat.elapsed() >= Duration::from_millis(cfg.heartbeat_ms) {
            if send_json(conn, &BridgeMsg::Health {
                ingress_pending: resident.pending_ingress(),
                native_ops_total: resident.counters().native_ops_total,
            })
            .is_err()
            {
                return SessionEnd::Broken;
            }
            c.heartbeats_sent += 1;
            heartbeat_misses += 1;
            if heartbeat_misses >= 3 {
                c.heartbeat_timeouts += 1;
                return SessionEnd::Broken;
            }
            last_heartbeat = Instant::now();
            // 心跳兼作事件轮询唤醒源(5s 节奏;D8 接收环排空依赖周期性泵)。
            wake();
        }
        // 4) 阻塞收(tick 周期返回)。
        match recv_json(conn, dec, tick) {
            Ok(CoreToBridgeMsg::Dispatch { request_id, target: _, text }) => {
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
            }
            Ok(CoreToBridgeMsg::EventAck { event_seq, suppressed }) => {
                if suppressed {
                    // 重复已抑制:按已确认处理(不重放)。
                }
                acked_high = acked_high.max(event_seq);
                while unacked.front().map(|(s, _)| *s <= acked_high).unwrap_or(false) {
                    if let Some((_, e)) = unacked.pop_front() {
                        unacked_bytes -= e.native_id.len() + 128;
                        c.events_acked += 1;
                    }
                }
                heartbeat_misses = 0;
            }
            Ok(CoreToBridgeMsg::HealthAck {}) => {
                heartbeat_misses = 0;
            }
            Ok(CoreToBridgeMsg::Stopped {}) => {
                return SessionEnd::CoreStopped;
            }
            Ok(CoreToBridgeMsg::Reject { .. } | CoreToBridgeMsg::HelloAck { .. }) => {
                c.protocol_errors += 1;
            }
            Err(PipeError::TimedOut) => {
                tick.store(false, Ordering::Relaxed);
            }
            Err(_) => return SessionEnd::Broken,
        }
    }
}
