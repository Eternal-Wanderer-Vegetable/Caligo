//! caligod — K4-D5 常驻 core 进程(计划 §7-D5)。
//!
//! 职责:打开 journal、创建双管道 server(bridge/control)、运行会话循环;
//! Ctrl+C 与 control Stop 走同一真实 Stopping 协议(actor.stop→close)。
//! 启动参数不携带认证凭据:token 每次启动由 OS 随机生成,打印到 stdout
//! 首行 JSON(仅供本地启动方读取;不进命令行、不进日志文件)。
//!
//! 用法:caligod --pipe-prefix <name> --journal <path> --baseline <id> --account <u>
//!            [--expect-pid <n>] [--core-build <s>]

use std::sync::atomic::{AtomicBool, Ordering};

/// Ctrl+C 处理器可见的进程级停止标志(与 control Stop 汇合)。
static CTRL_STOP: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> i32 {
    // 约束:控制台处理器内只做原子写,不分配/不等待(计划 §3.1 同源纪律)。
    CTRL_STOP.store(true, Ordering::Relaxed);
    1 // TRUE:已处理
}

#[derive(Debug)]
struct Args {
    pipe_prefix: String,
    journal: String,
    baseline: String,
    account: String,
    expect_pid: u32,
    core_build: String,
}

fn parse_args() -> Option<Args> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut a = Args {
        pipe_prefix: String::new(),
        journal: String::new(),
        baseline: "qq-9.9.33-52230".into(),
        account: String::new(),
        expect_pid: 0,
        core_build: caligo_core::CORE_BUILD.to_string(),
    };
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--pipe-prefix" => a.pipe_prefix = next(&mut i)?,
            "--journal" => a.journal = next(&mut i)?,
            "--baseline" => a.baseline = next(&mut i)?,
            "--account" => a.account = next(&mut i)?,
            "--expect-pid" => a.expect_pid = next(&mut i)?.parse().ok()?,
            "--core-build" => a.core_build = next(&mut i)?,
            _ => {}
        }
        i += 1;
    }
    if a.pipe_prefix.is_empty() || a.journal.is_empty() || a.account.is_empty() {
        eprintln!(
            "caligod --pipe-prefix <name> --journal <path> --baseline <id> --account <u> [--expect-pid <n>] [--core-build <s>]"
        );
        return None;
    }
    Some(a)
}

fn main() {
    let Some(args) = parse_args() else {
        std::process::exit(2);
    };
    let config = caligo_core::daemon::DaemonConfig {
        pipe_prefix: args.pipe_prefix,
        journal_path: args.journal.into(),
        module_baseline: args.baseline,
        account: args.account,
        core_build: args.core_build,
        expect_client_pid: args.expect_pid,
        runtime: Default::default(),
    };
    let daemon = match caligo_core::daemon::Daemon::start(config) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[caligod] journal open failed(不静默重建): {e}");
            std::process::exit(3);
        }
    };
    let (bridge_name, control_name) = daemon.pipe_names();
    // 启动信息首行(JSON):管道名 + 认证 token。启动方读取后应立即吞掉该输出。
    println!(
        "{}",
        serde_json::json!({
            "bridge_pipe": bridge_name,
            "control_pipe": control_name,
            "auth_token": daemon.auth_token(),
        })
    );

    // SAFETY: 处理器为进程生存期静态 fn;仅置原子标志。
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ctrl_handler), 1);
    }

    // daemon.run() 阻塞(accept 循环以 100ms 粒度轮询停止标志);
    // 主线程汇合 Ctrl+C 与 control Stop 两个来源。
    let runner = {
        let d = daemon.clone();
        std::thread::spawn(move || d.run())
    };
    loop {
        if CTRL_STOP.load(Ordering::Relaxed) {
            daemon.stop_flag().store(true, Ordering::Relaxed);
        }
        if runner.is_finished() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    match runner.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("[caligod] session error: {e}");
            std::process::exit(4);
        }
        Err(_) => {
            eprintln!("[caligod] run thread panicked");
            std::process::exit(5);
        }
    }
    println!("[caligod] stopped(closed;journal 保留)");
}
