//! caligo-cli:K1 开发 probe 与运行入口。
//!
//! 子命令(全部只读,除 `inject` 外):
//! - `verify-manifest <manifest.json>`:核对冻结版本模块的 SHA-256/大小(manifest 门)。
//! - `process`:枚举 QQ 进程与已加载的 Tencent/*.node 模块(只读观察)。
//! - `exports <module> [--all]`:解析 PE 导出表(静态调查)。
//! - `inject --pid <n> --bridge <path> --manifest <path> --report <path>
//!   --confirm-designated-test-instance`:门控加载 bridge 并取回探测报告。
//!   唯一写动作入口:要求 manifest 全部核对通过 **且** 执行者显式确认该实例。
//!
//! 退出码:0 成功;1 用法/IO 错误;2 校验不通过(拒绝)。

mod pe;
mod winutil;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const USAGE: &str = "\
caligo-cli <K1 probe>

用法:
  caligo-cli verify-manifest <manifest.json>
  caligo-cli process
  caligo-cli exports <module-path> [--all]
  caligo-cli inject --pid <n> --bridge <bridge.dll> --manifest <manifest.json>
                    --report <report.json> --confirm-designated-test-instance
                    [--wait-ms <ms>]

说明:
  inject 是唯一会产生加载动作的命令;它要求 manifest 核对全部通过,
  且执行者显式确认目标实例已按 docs/research/test-scope.md 指定。
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("verify-manifest") if args.len() == 2 => {
            let path = PathBuf::from(&args[1]);
            match verify_manifest(&path) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::from(2),
                Err(e) => {
                    eprintln!("verify-manifest error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("process") if args.len() == 1 => match cmd_process() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("process error: {e}");
                ExitCode::FAILURE
            }
        },
        Some("exports") => cmd_exports(&args[1..]),
        Some("inject") => cmd_inject(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Deserialize)]
struct Manifest {
    baseline_id: String,
    #[serde(default)]
    qq: Option<ManifestQq>,
    modules: Vec<ManifestModule>,
    #[serde(default)]
    policy: Option<ManifestPolicy>,
}

#[derive(Deserialize)]
struct ManifestQq {
    #[serde(default)]
    file_version: Option<String>,
    #[serde(default)]
    product_version: Option<String>,
    #[serde(default)]
    version_dir: Option<String>,
}

#[derive(Deserialize)]
struct ManifestModule {
    role: String,
    path: String,
    sha256: String,
    size_bytes: u64,
    #[serde(default = "default_true")]
    hash_checked: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct ManifestPolicy {
    #[serde(default)]
    on_mismatch: Option<String>,
    #[serde(default)]
    auto_multi_version: Option<bool>,
}

fn sha256_file(path: &Path) -> Result<(String, u64), String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total: u64 = 0;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02X}")).collect();
    Ok((hex, total))
}

/// 核对 manifest;返回 true 表示全部 hash_checked 模块匹配。
fn verify_manifest(path: &Path) -> Result<bool, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_str(&raw).map_err(|e| format!("parse manifest: {e}"))?;
    println!("baseline_id: {}", manifest.baseline_id);
    if let Some(qq) = &manifest.qq {
        println!(
            "qq: {} / {} @ {}",
            qq.file_version.as_deref().unwrap_or("?"),
            qq.product_version.as_deref().unwrap_or("?"),
            qq.version_dir.as_deref().unwrap_or("?")
        );
    }
    if let Some(policy) = &manifest.policy {
        println!(
            "policy: on_mismatch={} auto_multi_version={}",
            policy.on_mismatch.as_deref().unwrap_or("?"),
            policy.auto_multi_version.unwrap_or(false)
        );
    }
    let mut all_ok = true;
    for m in &manifest.modules {
        if !m.hash_checked {
            println!("[SKIP   ] {} ({})", m.path, m.role);
            continue;
        }
        match sha256_file(Path::new(&m.path)) {
            Ok((hex, size)) => {
                let hash_ok = hex.eq_ignore_ascii_case(&m.sha256);
                let size_ok = size == m.size_bytes;
                let status = if hash_ok && size_ok { "PASS" } else { "FAIL" };
                println!("[{status}] {} ({})", m.path, m.role);
                if !hash_ok {
                    println!("         expected sha256 {}", m.sha256);
                    println!("         actual   sha256 {hex}");
                }
                if !size_ok {
                    println!(
                        "         expected size {} actual size {size}",
                        m.size_bytes
                    );
                }
                all_ok &= hash_ok && size_ok;
            }
            Err(e) => {
                println!("[MISSING] {} ({}): {e}", m.path, m.role);
                all_ok = false;
            }
        }
    }
    println!(
        "RESULT: {}",
        if all_ok {
            "PASS — 基线匹配,attach 门允许进入下一层确认"
        } else {
            "REJECT — 基线不匹配,按策略拒绝 attach/发送"
        }
    );
    Ok(all_ok)
}

fn cmd_process() -> Result<(), String> {
    let procs = winutil::list_processes("QQ.exe")?;
    if procs.is_empty() {
        println!("未观察到 QQ 进程。");
        return Ok(());
    }
    println!("QQ 进程数: {}", procs.len());
    for p in &procs {
        println!("PID={} path={} started={}", p.pid, p.exe_path, p.started_utc.as_deref().unwrap_or("?"));
        let mut shown = 0;
        for m in &p.modules {
            let lower = m.to_ascii_lowercase();
            if lower.contains("\\tencent\\") || lower.ends_with(".node") {
                println!("  module: {m}");
                shown += 1;
            }
        }
        if shown == 0 {
            println!("  (未过滤到 Tencent/.node 模块)");
        }
    }
    Ok(())
}

fn cmd_exports(args: &[String]) -> ExitCode {
    let mut all = false;
    let mut path: Option<&str> = None;
    for a in args {
        match a.as_str() {
            "--all" => all = true,
            other => path = Some(other),
        }
    }
    let Some(path) = path else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match pe::parse_export_table(&bytes) {
        Ok(table) => {
            println!("file: {path}");
            println!("machine: {}", table.machine);
            if let Some(ts) = &table.timestamp_utc {
                println!("timestamp: {ts}");
            }
            println!("dll_name: {:?}", table.dll_name);
            println!(
                "exports: {} named / {} total (ordinal base {})",
                table.number_of_names, table.number_of_functions, table.name_ordinal_base
            );
            let interesting = |name: &str| {
                let l = name.to_ascii_lowercase();
                l.contains("napi") || l.contains("node_") || l.contains("nodeget")
                    || l.contains("registermodule") || l.starts_with("caligo")
            };
            let filtered: Vec<_> = table
                .exports
                .iter()
                .filter(|e| all || interesting(&e.name))
                .collect();
            for e in &filtered {
                println!(
                    "  ord={:>6} rva={:#010x} {}{}",
                    e.ordinal,
                    e.function_rva,
                    e.name,
                    if e.forwarded { " (forwarder)" } else { "" }
                );
            }
            if !all {
                println!(
                    "(显示 {} 条 napi/node 相关导出;--all 查看全部 {} 条)",
                    filtered.len(),
                    table.exports.len()
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("parse export table: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_inject(args: &[String]) -> ExitCode {
    let mut pid: Option<u32> = None;
    let mut bridge: Option<PathBuf> = None;
    let mut manifest: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut confirmed = false;
    let mut wait_ms: u32 = 20_000;

    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--pid" => match next(&mut i).and_then(|s| s.parse().ok()) {
                Some(v) => pid = Some(v),
                None => {
                    eprintln!("--pid 需要数字参数");
                    return ExitCode::FAILURE;
                }
            },
            "--bridge" => match next(&mut i) {
                Some(v) => bridge = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--bridge 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--manifest" => match next(&mut i) {
                Some(v) => manifest = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--manifest 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--report" => match next(&mut i) {
                Some(v) => report = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--report 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--wait-ms" => match next(&mut i).and_then(|s| s.parse().ok()) {
                Some(v) => wait_ms = v,
                None => {
                    eprintln!("--wait-ms 需要数字参数");
                    return ExitCode::FAILURE;
                }
            },
            "--confirm-designated-test-instance" => confirmed = true,
            other => {
                eprintln!("未知参数: {other}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    let (Some(pid), Some(bridge), Some(manifest), Some(report)) =
        (pid, bridge, manifest, report)
    else {
        eprintln!("inject 需要 --pid/--bridge/--manifest/--report\n{USAGE}");
        return ExitCode::FAILURE;
    };

    // 门 1:manifest 摘要核对(计划 §5:版本与模块摘要确认 → 接入)。
    println!("[gate 1] 核对模块基线 …");
    match verify_manifest(&manifest) {
        Ok(true) => println!("[gate 1] PASS"),
        Ok(false) => {
            eprintln!("[gate 1] REJECT — 拒绝 attach(基线不匹配,不猜偏移)");
            return ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("[gate 1] error: {e}");
            return ExitCode::FAILURE;
        }
    }

    // 门 2:执行者显式确认测试实例(test-scope §4:运行实例不自动成为实验目标)。
    if !confirmed {
        eprintln!(
            "[gate 2] REJECT — 缺少 --confirm-designated-test-instance。\n\
             运行中的 QQ 实例不会被自动选为实验目标;请先按 docs/research/test-scope.md \
             指定并记录测试实例(PID + 创建时间),再带该参数重试。"
        );
        return ExitCode::from(2);
    }
    println!("[gate 2] PASS — 执行者已确认 PID {pid} 为指定测试实例");

    // 加载 + probe。
    println!("[inject] 加载 bridge 并调用 caligo_probe_run …");
    let outcome = unsafe { winutil::inject_and_probe(pid, &bridge, &report, wait_ms) };
    match outcome {
        Ok(o) => {
            println!(
                "[inject] 完成:remote_base={:#x} probe_exit_code={} ({})",
                o.remote_base,
                o.probe_exit_code,
                match o.probe_exit_code {
                    caligo_bridge::probe_code::OK => "OK",
                    caligo_bridge::probe_code::ERR_NULL_PATH => "ERR_NULL_PATH",
                    caligo_bridge::probe_code::ERR_BAD_PATH => "ERR_BAD_PATH",
                    caligo_bridge::probe_code::ERR_WRITE_FAILED => "ERR_WRITE_FAILED",
                    _ => "UNKNOWN",
                }
            );
            match std::fs::read_to_string(&report) {
                Ok(json) => {
                    println!("=== probe report ({}) ===", report.display());
                    println!("{json}");
                    // 握手第三层:报告中的协议版本须与 core 期望一致。
                    match serde_json::from_str::<caligo_bridge::ProbeReport>(&json) {
                        Ok(rep) => {
                            if rep.protocol_version == caligo_core::ipc::PROTOCOL_VERSION {
                                println!(
                                    "[handshake] PASS — protocol_version={} bridge_build={:?}",
                                    rep.protocol_version, rep.bridge_build
                                );
                            } else {
                                eprintln!(
                                    "[handshake] REJECT — protocol_version {} != core 期望 {}",
                                    rep.protocol_version,
                                    caligo_core::ipc::PROTOCOL_VERSION
                                );
                                return ExitCode::from(2);
                            }
                        }
                        Err(e) => {
                            eprintln!("probe 报告解析失败: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("probe 报告读取失败 {}: {e}", report.display());
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[inject] 失败:{e}");
            eprintln!(
                "恢复:以正常退出该测试 QQ 实例收尾(docs/research/recovery-notes.md §2)。"
            );
            ExitCode::FAILURE
        }
    }
}
