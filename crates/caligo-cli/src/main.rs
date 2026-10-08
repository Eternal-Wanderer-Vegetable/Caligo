//! caligo-cli:K1 开发 probe 与运行入口。
//!
//! 子命令(全部只读,除 `inject` 外):
//! - `verify-manifest <manifest.json>`:核对冻结版本模块的 SHA-256/大小(manifest 门)。
//! - `process`:枚举 QQ 进程与已加载的 Tencent/*.node 模块(只读观察)。
//! - `exports <module> [--all]`:解析 PE 导出表(静态调查)。
//! - `rtti <module> --contains <substr>`:MSVC RTTI 扫描,定位类 vtable RVA(K2-02 WU1)。
//! - `envscan --pid <n> --vtable <rva[:off]>…`:外部只读内存扫描定位 Environment
//!   候选(K2-02 WU2;ReadProcessMemory,零注入)。
//! - `inject --pid <n> --bridge <path> --manifest <path> --report <path>
//!   --confirm-designated-test-instance`:门控加载 bridge 并取回探测报告。
//!   唯一写动作入口:要求 manifest 全部核对通过 **且** 执行者显式确认该实例。
//!   `--intr-*` 系列触发 WU3 RequestInterrupt 实验(见 docs/research §10)。
//!
//! 退出码:0 成功;1 用法/IO 错误;2 校验不通过(拒绝)。

mod disasm;
mod envscan;
mod observe_msf;
mod pe;
mod qqentry;
mod rtti;
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
  caligo-cli bytes <module-path> <rva-hex | export=NAME> [--len <n>]
  caligo-cli disasm <module-path> <rva-hex | export=NAME> [--len <n>]
  caligo-cli rtti <module-path> --contains <substr>
  caligo-cli envscan --pid <n> --vtable <rva-hex[:member-off-hex]> [--include-mapped]
                     [--context <bytes>] [--out <report.json>]
  caligo-cli observe-msf --pid <n> [--out <report.json>] [--self-test]
  caligo-cli inject --pid <n> --bridge <bridge.dll> --manifest <manifest.json>
                    --report <report.json> --confirm-designated-test-instance
                    [--obs-report <obs.json>] [--env-report <env.jsonl>]
                    [--g1-report <g1.jsonl>]
                    [--register-entry] [--wait-ms <ms>]
                    [--intr-report <intr.jsonl> --intr-env <ptr-hex>]
                    [--intr-vftable <va-hex>] [--intr-wait-ms <ms>]
                    [--async-report <async.jsonl> --async-mode <0|1|2>
                     --async-env <ptr-hex>] [--async-wait-ms <ms>]

说明:
  inject 是唯一会产生加载动作的命令;它要求 manifest 核对全部通过,
  且执行者显式确认目标实例已按 docs/research/test-scope.md 指定。
  bytes/rtti 是离线静态分析工具(读文件字节)。
  envscan 是外部只读内存扫描(ReadProcessMemory,零注入;仍须记录目标实例)。
  observe-msf 是 P1 受控观测(外部只读,零注入/零写入/零 QQ 函数调用;
  不消耗实例的首次 bootstrap 机会;输出记录 PID 与进程创建时间)。
  --intr-env 0 为干跑(只验证链路,不调用 RequestInterrupt)。
  --async-mode 0/1/2 = 干跑 / uv_async 原子载荷 / JS 枚举 major(K2-03)。
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
        Some("bytes") => cmd_bytes(&args[1..]),
        Some("disasm") => cmd_disasm(&args[1..]),
        Some("identifiers") => cmd_identifiers(&args[1..]),
        Some("rtti") => cmd_rtti(&args[1..]),
        Some("envscan") => cmd_envscan(&args[1..]),
        Some("inject") => cmd_inject(&args[1..]),
        Some("qq-entry") => qqentry::cmd_qq_entry(&args[1..]),
        Some("qq-entry-stop") => qqentry::cmd_qq_entry_stop(&args[1..]),
        Some("qq-status") => qqentry::cmd_qq_status(&args[1..]),
        Some("daemon-control") => cmd_daemon_control(&args[1..]),
        Some("observe-msf") => {
            match observe_msf::cmd_observe_msf(&args[1..]) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("observe-msf error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
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
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
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
                    println!("         expected size {} actual size {size}", m.size_bytes);
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
        println!(
            "PID={} path={} started={}",
            p.pid,
            p.exe_path,
            p.started_utc.as_deref().unwrap_or("?")
        );
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
                l.contains("napi")
                    || l.contains("node_")
                    || l.contains("nodeget")
                    || l.contains("registermodule")
                    || l.starts_with("caligo")
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

fn to_absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().map_err(|e| format!("current_dir: {e}"))?;
        Ok(cwd.join(path))
    }
}

/// 离线静态分析:十六进制转储模块内指定 RVA 或导出函数起始的字节。只读文件。
fn cmd_bytes(args: &[String]) -> ExitCode {
    let mut module: Option<&str> = None;
    let mut target: Option<&str> = None;
    let mut len: usize = 96;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--len" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(v) => len = v,
                    None => {
                        eprintln!("--len 需要数字参数");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other if module.is_none() => module = Some(other),
            other if target.is_none() => target = Some(other),
            other => {
                eprintln!("多余参数: {other}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let (Some(module), Some(target)) = (module, target) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let bytes = match std::fs::read(module) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {module}: {e}");
            return ExitCode::FAILURE;
        }
    };

    // 解析目标:十六进制 RVA,或 export=导出名。
    let rva: u32 = if let Some(name) = target.strip_prefix("export=") {
        match std::fs::read(module)
            .map_err(|e| e.to_string())
            .and_then(|b| pe::parse_export_table(&b).map(|t| (t, b)))
        {
            Ok((table, _)) => match table.exports.iter().find(|e| e.name == name) {
                Some(e) => e.function_rva,
                None => {
                    eprintln!("导出 {name} 未找到于 {module}");
                    return ExitCode::FAILURE;
                }
            },
            Err(e) => {
                eprintln!("解析导出表: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else if let Some(hex) = target.strip_prefix("0x").or(target.strip_prefix("0X")) {
        match u32::from_str_radix(hex, 16) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("RVA 解析失败: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        eprintln!("目标须为 rva-hex 或 export=NAME");
        return ExitCode::FAILURE;
    };

    let offset = match pe::rva_to_file_offset(&bytes, rva) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let end = (offset + len).min(bytes.len());
    println!("file: {module}");
    println!(
        "rva: {rva:#010x}  file_offset: {offset:#010x}  bytes: {}",
        end - offset
    );
    for (row, chunk) in bytes[offset..end].chunks(16).enumerate() {
        let hex: String = chunk.iter().map(|b| format!("{b:02X} ")).collect();
        let ascii: String = chunk
            .iter()
            .map(|b| {
                if b.is_ascii_graphic() || *b == b' ' {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  {:+06x}  {:<48} {ascii}", row * 16, hex);
    }
    ExitCode::SUCCESS
}

/// 离线静态挖掘:提取映像内的标识符样式 C 字符串(候选 napi 绑定名),带 RVA。
/// 仅打印匹配 `^[A-Za-z_][A-Za-z0-9_.$-]{min-1,}$` 的串;只读文件。
fn cmd_identifiers(args: &[String]) -> ExitCode {
    let mut module: Option<&str> = None;
    let mut min_len: usize = 6;
    let mut contains: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--min" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(v) => min_len = v,
                    None => {
                        eprintln!("--min 需要数字参数");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--contains" => {
                i += 1;
                match args.get(i) {
                    Some(v) => contains = Some(v.to_ascii_lowercase()),
                    None => {
                        eprintln!("--contains 需要参数");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other if module.is_none() => module = Some(other),
            other => {
                eprintln!("多余参数: {other}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let Some(module) = module else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let bytes = match std::fs::read(module) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {module}: {e}");
            return ExitCode::FAILURE;
        }
    };

    // RVA→文件偏移表(用段表把文件偏移映回 RVA)。
    let e_lfanew = match u32_at(&bytes, 0x3C) {
        Some(v) => v as usize,
        None => {
            eprintln!("not an MZ image");
            return ExitCode::FAILURE;
        }
    };
    let coff = e_lfanew + 4;
    let number_of_sections = match u16_at(&bytes, coff + 2) {
        Some(v) => v as usize,
        None => {
            eprintln!("truncated COFF");
            return ExitCode::FAILURE;
        }
    };
    let size_of_optional = match u16_at(&bytes, coff + 16) {
        Some(v) => v as usize,
        None => {
            eprintln!("truncated COFF");
            return ExitCode::FAILURE;
        }
    };
    let sec_base = coff + 20 + size_of_optional;
    let mut sections = Vec::with_capacity(number_of_sections);
    for s in 0..number_of_sections {
        let base = sec_base + s * 40;
        let raw_size = u32_at(&bytes, base + 16).unwrap_or(0) as usize;
        let raw_ptr = u32_at(&bytes, base + 20).unwrap_or(0) as usize;
        let va = u32_at(&bytes, base + 12).unwrap_or(0);
        sections.push((raw_ptr, raw_ptr + raw_size, va));
    }
    let file_off_to_rva = |off: usize| -> u32 {
        for (start, end, va) in &sections {
            if off >= *start && off < *end {
                return va + (off - *start) as u32;
            }
        }
        0
    };

    let is_ident_start = |b: u8| b.is_ascii_alphabetic() || b == b'_';
    let is_ident =
        |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'$' || b == b'-';

    let mut count = 0usize;
    let mut idx = 0;
    while idx < bytes.len() {
        // 定位一个候选串起点。
        if is_ident_start(bytes[idx]) {
            let start = idx;
            let mut end = idx;
            while end < bytes.len() && is_ident(bytes[end]) {
                end += 1;
            }
            let len = end - start;
            // 串必须以 NUL 结尾才是 C 字符串。
            if len >= min_len && bytes.get(end) == Some(&0) {
                let s = &bytes[start..end];
                let ok_contains = match &contains {
                    Some(c) => s
                        .to_ascii_lowercase()
                        .windows(c.len())
                        .any(|w| w == c.as_bytes()),
                    None => true,
                };
                if ok_contains {
                    println!(
                        "{:#010x}  {}",
                        file_off_to_rva(start),
                        String::from_utf8_lossy(s)
                    );
                    count += 1;
                }
            }
            idx = end.max(start + 1);
        } else {
            idx += 1;
        }
    }
    eprintln!("# {count} identifier-style C strings from {module}");
    ExitCode::SUCCESS
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

/// 离线静态分析:反汇编模块内指定 RVA 或导出函数的代码(iced-x86,只读文件)。
fn cmd_disasm(args: &[String]) -> ExitCode {
    let mut module: Option<&str> = None;
    let mut target: Option<&str> = None;
    let mut len: usize = 160;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--len" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(v) => len = v,
                    None => {
                        eprintln!("--len 需要数字参数");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other if module.is_none() => module = Some(other),
            other if target.is_none() => target = Some(other),
            other => {
                eprintln!("多余参数: {other}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let (Some(module), Some(target)) = (module, target) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(module) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {module}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let rva: u32 = if let Some(name) = target.strip_prefix("export=") {
        match pe::parse_export_table(&bytes) {
            Ok(table) => match table.exports.iter().find(|e| e.name == name) {
                Some(e) => e.function_rva,
                None => {
                    eprintln!("导出 {name} 未找到于 {module}");
                    return ExitCode::FAILURE;
                }
            },
            Err(e) => {
                eprintln!("解析导出表: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else if let Some(hex) = target.strip_prefix("0x").or(target.strip_prefix("0X")) {
        match u32::from_str_radix(hex, 16) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("RVA 解析失败: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        eprintln!("目标须为 rva-hex 或 export=NAME");
        return ExitCode::FAILURE;
    };
    let offset = match pe::rva_to_file_offset(&bytes, rva) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    println!("file: {module}  rva: {rva:#010x}  len: {len}");
    match disasm::disasm_at(&bytes, offset, rva, len) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("disasm: {e}");
            ExitCode::FAILURE
        }
    }
}

/// K2-02 WU1:MSVC RTTI 离线扫描——TD → COL → vtable RVA。
fn cmd_rtti(args: &[String]) -> ExitCode {
    let mut module: Option<&str> = None;
    let mut contains: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--contains" => {
                i += 1;
                match args.get(i) {
                    Some(v) => contains = Some(v.clone()),
                    None => {
                        eprintln!("--contains 需要参数");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other if module.is_none() => module = Some(other),
            other => {
                eprintln!("多余参数: {other}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let (Some(module), Some(contains)) = (module, contains) else {
        eprintln!("rtti 需要 <module> 与 --contains\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(module) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {module}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let started = std::time::Instant::now();
    match rtti::scan(&bytes, &contains) {
        Ok(hits) => {
            println!(
                "file: {module}  scan: {:.2}s  TD hits: {}",
                started.elapsed().as_secs_f64(),
                hits.len()
            );
            let mut total_vtables = 0;
            for td in &hits {
                println!(
                    "TD rva={:#010x}  name={}",
                    td.td_rva, td.name
                );
                for col in &td.cols {
                    println!(
                        "  COL rva={:#010x}  member_offset={:#x}  cd_offset={:#x}  vtables={}",
                        col.col_rva,
                        col.member_offset,
                        col.cd_offset,
                        col.vtables.len()
                    );
                    for vt in &col.vtables {
                        println!(
                            "    vtable rva={:#010x}  [-1]→COL  first_fn_rva={}",
                            vt.vtable_rva,
                            match vt.first_fn_rva {
                                Some(r) => format!("{r:#010x}"),
                                None => "n/a".into(),
                            }
                        );
                        total_vtables += 1;
                    }
                }
                if td.cols.is_empty() {
                    println!("  (无结构校验通过的 COL)");
                }
            }
            eprintln!("# {total_vtables} vtable(s) across {} TD(s)", hits.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("rtti scan: {e}");
            ExitCode::FAILURE
        }
    }
}

/// K2-02 WU2:外部只读内存扫描定位候选 node::Environment。
fn cmd_envscan(args: &[String]) -> ExitCode {
    let mut pid: Option<u32> = None;
    let mut targets: Vec<envscan::ScanTarget> = Vec::new();
    let mut include_mapped = false;
    let mut context_bytes: usize = 0x100;
    let mut out_path: Option<PathBuf> = None;
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
            "--vtable" => {
                let Some(spec) = next(&mut i) else {
                    eprintln!("--vtable 需要参数 rva[:member-offset](十六进制)");
                    return ExitCode::FAILURE;
                };
                let (rva_s, off_s) = match spec.split_once(':') {
                    Some((r, o)) => (r, o),
                    None => (spec.as_str(), "0"),
                };
                let parsed = (|| {
                    let rva = u32::from_str_radix(
                        rva_s.strip_prefix("0x").unwrap_or(rva_s),
                        16,
                    ).ok()?;
                    let off = u32::from_str_radix(
                        off_s.strip_prefix("0x").unwrap_or(off_s),
                        16,
                    ).ok()?;
                    Some(envscan::ScanTarget {
                        vtable_rva: rva,
                        member_offset: off,
                    })
                })();
                match parsed {
                    Some(t) => targets.push(t),
                    None => {
                        eprintln!("--vtable 解析失败: {spec}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--include-mapped" => include_mapped = true,
            "--context" => match next(&mut i).and_then(|s| s.parse().ok()) {
                Some(v) => context_bytes = v,
                None => {
                    eprintln!("--context 需要数字参数");
                    return ExitCode::FAILURE;
                }
            },
            "--out" => match next(&mut i) {
                Some(v) => out_path = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--out 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            other => {
                eprintln!("未知参数: {other}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let (Some(pid), false) = (pid, targets.is_empty()) else {
        eprintln!("envscan 需要 --pid 与至少一个 --vtable\n{USAGE}");
        return ExitCode::FAILURE;
    };

    println!(
        "[envscan] pid={pid} targets={} context={context_bytes:#x} include_mapped={include_mapped}",
        targets.len()
    );
    println!("[envscan] 只读外部扫描(ReadProcessMemory,零注入)…");
    let started = std::time::Instant::now();
    let report = match unsafe { envscan::scan_process(pid, &targets, context_bytes, include_mapped) } {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[envscan] 失败: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "[envscan] 扫描完成:{:.2}s regions={} bytes={:.2} GiB read_failed={} hits={}",
        started.elapsed().as_secs_f64(),
        report.regions_scanned,
        report.bytes_scanned as f64 / (1024.0 * 1024.0 * 1024.0),
        report.regions_read_failed,
        report.hits.len()
    );
    let json = serde_json::to_string_pretty(&report).unwrap_or_default();
    if let Some(p) = &out_path {
        if let Some(parent) = p.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("报告目录创建失败 {}: {e}", parent.display());
                return ExitCode::FAILURE;
            }
        }
        if let Err(e) = std::fs::write(p, &json) {
            eprintln!("报告写盘失败 {}: {e}", p.display());
            return ExitCode::FAILURE;
        }
        println!("[envscan] 报告已写入 {}", p.display());
    }
    // 摘要输出(完整 JSON 见报告文件/重定向)。
    for h in &report.hits {
        println!(
            "HIT va={:#018x} vtable_rva={:#x} member_off={:#x} candidate_env={:#018x} region={} protect={:#x} module={} ctx={}",
            h.va,
            h.vtable_rva,
            h.member_offset,
            h.candidate_env,
            h.region_type,
            h.protect,
            h.in_module.as_deref().unwrap_or("-"),
            h.context_len,
        );
    }
    if report.hits.is_empty() {
        println!("(无命中 — 检查 vtable RVA 是否来自同一冻结版本,或加 --include-mapped)");
    }
    ExitCode::SUCCESS
}

fn cmd_inject(args: &[String]) -> ExitCode {
    // K4-D0 门控:先于一切参数解析与进程操作。旧注入路线(加载 DLL、远程线程、
    // async/exec 载荷、逐轮 poll)在普通构建中默认拒绝;恢复条件见
    // docs/research/k4-execution-scope.md 与 D6 现场准入。
    if !caligo_bridge::gate::research_enabled() {
        eprintln!("{}", caligo_bridge::gate::DISABLED_NOTICE);
        return ExitCode::from(3);
    }
    let mut pid: Option<u32> = None;
    let mut bridge: Option<PathBuf> = None;
    let mut manifest: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut obs_report: Option<PathBuf> = None;
    let mut g1_report: Option<PathBuf> = None;
    let mut env_report: Option<PathBuf> = None;
    let mut register_entry = false;
    let mut confirmed = false;
    let mut wait_ms: u32 = 20_000;
    let mut intr_report: Option<PathBuf> = None;
    let mut intr_env: Option<usize> = None;
    let mut intr_vftable: usize = 0;
    let mut intr_wait_ms: u32 = 10_000;
    let mut async_report: Option<PathBuf> = None;
    let mut async_mode: Option<u32> = None;
    let mut async_env: Option<usize> = None;
    let mut async_script: u32 = 0;
    let mut async_wait_ms: u32 = 10_000;
    let mut send_peer: Option<String> = None;
    let mut send_chat: u32 = 2;
    let mut send_text: Option<String> = None;
    let mut exec_js: Option<PathBuf> = None;
    let mut exec_env: Option<usize> = None;
    let mut exec_ctx_hint: Option<usize> = None;
    let mut exec_wait_ms: u32 = 30_000;

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
            "--g1-report" => match next(&mut i) {
                Some(v) => g1_report = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--g1-report 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--obs-report" => match next(&mut i) {
                Some(v) => obs_report = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--obs-report 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--env-report" => match next(&mut i) {
                Some(v) => env_report = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--env-report 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--register-entry" => register_entry = true,
            "--intr-report" => match next(&mut i) {
                Some(v) => intr_report = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--intr-report 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--intr-env" => match next(&mut i).as_deref().and_then(|s| {
                usize::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
            }) {
                Some(v) => intr_env = Some(v),
                None => {
                    eprintln!("--intr-env 需要十六进制指针参数");
                    return ExitCode::FAILURE;
                }
            },
            "--intr-vftable" => match next(&mut i).as_deref().and_then(|s| {
                usize::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
            }) {
                Some(v) => intr_vftable = v,
                None => {
                    eprintln!("--intr-vftable 需要十六进制参数");
                    return ExitCode::FAILURE;
                }
            },
            "--intr-wait-ms" => match next(&mut i).and_then(|s| s.parse().ok()) {
                Some(v) => intr_wait_ms = v,
                None => {
                    eprintln!("--intr-wait-ms 需要数字参数");
                    return ExitCode::FAILURE;
                }
            },
            "--async-report" => match next(&mut i) {
                Some(v) => async_report = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--async-report 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--async-mode" => match next(&mut i).and_then(|s| s.parse::<u32>().ok()) {
                Some(v @ 0..=3) => async_mode = Some(v),
                _ => {
                    eprintln!("--async-mode 需要 0/1/2/3");
                    return ExitCode::FAILURE;
                }
            },
            "--async-env" => match next(&mut i).as_deref().and_then(|s| {
                usize::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
            }) {
                Some(v) => async_env = Some(v),
                None => {
                    eprintln!("--async-env 需要十六进制指针参数");
                    return ExitCode::FAILURE;
                }
            },
            "--async-script" => match next(&mut i).and_then(|s| s.parse::<u32>().ok()) {
                Some(v @ 0..=85) => async_script = v,
                _ => {
                    eprintln!("--async-script 需要 0-27(…26 DOM 侦察 27 DOM 注入)");
                    return ExitCode::FAILURE;
                }
            },
            "--async-wait-ms" => match next(&mut i).and_then(|s| s.parse().ok()) {
                Some(v) => async_wait_ms = v,
                None => {
                    eprintln!("--async-wait-ms 需要数字参数");
                    return ExitCode::FAILURE;
                }
            },
            "--send-peer" => match next(&mut i) {
                Some(v) => send_peer = Some(v),
                None => {
                    eprintln!("--send-peer 需要参数");
                    return ExitCode::FAILURE;
                }
            },
            "--send-chat" => match next(&mut i).and_then(|s| s.parse::<u32>().ok()) {
                Some(v @ 1..=2) => send_chat = v,
                _ => {
                    eprintln!("--send-chat 需要 1(私聊)或 2(群聊)");
                    return ExitCode::FAILURE;
                }
            },
            "--send-text" => match next(&mut i) {
                Some(v) => send_text = Some(v),
                None => {
                    eprintln!("--send-text 需要参数");
                    return ExitCode::FAILURE;
                }
            },
            "--exec-js" => match next(&mut i) {
                Some(v) => exec_js = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--exec-js 需要路径参数");
                    return ExitCode::FAILURE;
                }
            },
            "--exec-env" => match next(&mut i).as_deref().and_then(|s| {
                usize::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
            }) {
                Some(v) => exec_env = Some(v),
                None => {
                    eprintln!("--exec-env 需要十六进制指针参数");
                    return ExitCode::FAILURE;
                }
            },
            "--exec-ctx" => match next(&mut i).as_deref().and_then(|s| {
                usize::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
            }) {
                Some(v) => exec_ctx_hint = Some(v),
                None => {
                    eprintln!("--exec-ctx 需要十六进制参数");
                    return ExitCode::FAILURE;
                }
            },
            "--exec-wait-ms" => match next(&mut i).and_then(|s| s.parse().ok()) {
                Some(v) => exec_wait_ms = v,
                None => {
                    eprintln!("--exec-wait-ms 需要数字参数");
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

    let (Some(pid), Some(bridge), Some(manifest), Some(report)) = (pid, bridge, manifest, report)
    else {
        eprintln!("inject 需要 --pid/--bridge/--manifest/--report\n{USAGE}");
        return ExitCode::FAILURE;
    };

    // 路径必须绝对化:相对路径会在目标进程的工作目录下解析(LoadLibraryW 与
    // bridge 内的 std::fs::write 都按目标进程 CWD 解释),导致加载失败或写错位置。
    let bridge = match to_absolute(&bridge) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("--bridge 路径无效: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report = match to_absolute(&report) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("--report 路径无效: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(parent) = report.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("报告目录创建失败 {}: {e}", parent.display());
            return ExitCode::FAILURE;
        }
    }
    let g1_report = match g1_report {
        Some(p) => match p.canonicalize() {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("--g1-report 路径无效: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let obs_report = match obs_report {
        Some(p) => {
            let abs = match to_absolute(&p) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("--obs-report 路径无效: {e}");
                    return ExitCode::FAILURE;
                }
            };
            if let Some(parent) = abs.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("obs 报告目录创建失败 {}: {e}", parent.display());
                    return ExitCode::FAILURE;
                }
            }
            Some(abs)
        }
        None => None,
    };
    let env_report = match env_report {
        Some(p) => {
            let abs = match to_absolute(&p) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("--env-report 路径无效: {e}");
                    return ExitCode::FAILURE;
                }
            };
            if let Some(parent) = abs.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("env 报告目录创建失败 {}: {e}", parent.display());
                    return ExitCode::FAILURE;
                }
            }
            Some(abs)
        }
        None => None,
    };
    // WU3 RequestInterrupt 实验:--intr-report 与 --intr-env 必须成对。
    let intr = match (intr_report, intr_env) {
        (Some(p), Some(env)) => {
            let abs = match to_absolute(&p) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("--intr-report 路径无效: {e}");
                    return ExitCode::FAILURE;
                }
            };
            if let Some(parent) = abs.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("intr 报告目录创建失败 {}: {e}", parent.display());
                    return ExitCode::FAILURE;
                }
            }
            if env != 0 && intr_vftable == 0 {
                eprintln!(
                    "[gate] --intr-env 给出了非零候选且未提供 --intr-vftable。\n\
                     真实调用必须核对 vfptr(拒绝盲调);干跑请用 --intr-env 0。"
                );
                return ExitCode::from(2);
            }
            Some(winutil::IntrRequest {
                env,
                expected_vftable: intr_vftable,
                report: abs,
                wait_ms: intr_wait_ms,
            })
        }
        (Some(_), None) => {
            eprintln!("--intr-report 需要 --intr-env 成对出现");
            return ExitCode::FAILURE;
        }
        (None, Some(_)) => {
            eprintln!("--intr-env 需要 --intr-report 成对出现");
            return ExitCode::FAILURE;
        }
        (None, None) => None,
    };
    // K2-03 事件循环点载荷:--async-report + --async-mode + --async-env 成组。
    let async_req = match (async_report, async_mode, async_env) {
        (Some(p), Some(mode), Some(env)) => {
            let abs = match to_absolute(&p) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("--async-report 路径无效: {e}");
                    return ExitCode::FAILURE;
                }
            };
            if let Some(parent) = abs.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("async 报告目录创建失败 {}: {e}", parent.display());
                    return ExitCode::FAILURE;
                }
            }
            if env != 0 && mode == 0 {
                eprintln!("[gate] --async-env 与 --async-mode 0 同时给出;mode 0 忽略 env(按干跑执行)");
            }
            if env == 0 && mode >= 1 {
                eprintln!("[gate] mode 1/2 需要 --async-env(K2-02 扫描的候选地址)");
                return ExitCode::from(2);
            }
            if env != 0 && mode >= 1 {
                println!(
                    "[gate] K2-03/04 mode {mode}:将在指定实例上对 env {env:#x} 投递 uv_async 载荷{}",
                    if mode == 2 {
                        match async_script {
                            1 => "(含 load 探针:首次调用 QQ 业务函数,无参一次)",
                            _ => "(含 JS 枚举)",
                        }
                    } else {
                        ""
                    }
                );
            }
            if async_script == 1 && mode == 2 {
                println!("[gate] K2-04:load 探针为首次调用 QQ 业务函数,按设计记录 local-evidence/k2-04/design.md 执行");
            }
            // K3-F 参数化发送:PARAM_SEND 需要 --send-* 三件套。
            let params: Vec<u8> = if async_script == caligo_bridge::asyncrun::SCRIPT_K3_PARAM_SEND {
                match (&send_peer, &send_text) {
                    (Some(p), Some(t)) => serde_json::json!({
                        "peer": p, "chat": send_chat, "text": t
                    })
                    .to_string()
                    .into_bytes(),
                    _ => {
                        eprintln!(
                            "[gate] PARAM_SEND 需要 --send-peer/--send-chat/--send-text 三件套"
                        );
                        return ExitCode::from(2);
                    }
                }
            } else {
                Vec::new()
            };
            Some(winutil::AsyncRequest {
                env,
                mode,
                script: async_script,
                report: abs,
                wait_ms: async_wait_ms,
                params,
            })
        }
        (Some(_), None, _) | (_, Some(_), None) | (_, None, Some(_))
        | (None, Some(_), Some(_)) => {
            eprintln!("--async-report/--async-mode/--async-env 必须成组出现");
            return ExitCode::FAILURE;
        }
        (None, None, None) => None,
    };
    // K3-E 通用 JS 执行:--exec-js + --exec-env 成对(--exec-wait-ms 可选)。
    let exec_req = match (exec_js, exec_env) {
        (Some(js), Some(env)) => {
            let abs = match to_absolute(&js) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("--exec-js 路径无效: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let src = match std::fs::read(&abs) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("--exec-js 读取失败 {}: {e}", abs.display());
                    return ExitCode::FAILURE;
                }
            };
            let rep = abs.with_extension("report.jsonl");
            println!("[gate] K3-E exec:JS {} 字节 → env {env:#x}{}", src.len(), match exec_ctx_hint { Some(c) => format!(" ctxhint {c:#x}"), None => String::new() });
            Some(winutil::ExecRequest {
                env,
                ctx_hint: exec_ctx_hint.unwrap_or(0),
                js: src,
                report: rep,
                wait_ms: exec_wait_ms,
            })
        }
        (Some(_), None) => {
            eprintln!("--exec-js 需要 --exec-env 成对出现");
            return ExitCode::FAILURE;
        }
        (None, Some(_)) => {
            eprintln!("--exec-env 需要 --exec-js 成对出现");
            return ExitCode::FAILURE;
        }
        (None, None) => None,
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

    // 加载 + probe(可选 obs / register / env / intr / async / exec)。
    println!("[inject] 加载 bridge 并调用 caligo_probe_run …");
    let outcome = unsafe {
        winutil::inject_and_probe(
            pid,
            &bridge,
            &report,
            obs_report.as_deref(),
            env_report.as_deref(),
            register_entry,
            wait_ms,
            intr.as_ref(),
            async_req.as_ref(),
            exec_req.as_ref(),
            g1_report.as_deref(),
        )
    };
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
            if let Some(reg_code) = o.register_exit_code {
                println!(
                    "[register] caligo_register_entry:exit_code={} ({})",
                    reg_code,
                    match reg_code {
                        0 => "OK",
                        1 => "ERR_ALREADY(本实例已注册)",
                        2 => "ERR_NO_QQNT",
                        3 => "ERR_NO_MAGIC",
                        _ => "UNKNOWN",
                    }
                );
            }
            if let Some(env_code) = o.env_exit_code {
                println!(
                    "[env] caligo_env_start:exit_code={} ({})",
                    env_code,
                    match env_code {
                        caligo_bridge::envrun::env_code::OK => "OK(链路线程已启动)",
                        caligo_bridge::envrun::env_code::ERR_NULL_PATH => "ERR_NULL_PATH",
                        caligo_bridge::envrun::env_code::ERR_NO_QQNT => "ERR_NO_QQNT",
                        caligo_bridge::envrun::env_code::ERR_THREAD => "ERR_THREAD",
                        _ => "UNKNOWN",
                    }
                );
            }
            if let (Some(obs_path), Some(obs_code)) = (obs_report.as_deref(), o.obs_exit_code) {
                println!(
                    "[obs] caligo_obs_run 完成:exit_code={} ({})",
                    obs_code,
                    match obs_code {
                        caligo_bridge::probe_code::OK => "OK",
                        caligo_bridge::probe_code::ERR_NULL_PATH => "ERR_NULL_PATH",
                        caligo_bridge::probe_code::ERR_BAD_PATH => "ERR_BAD_PATH",
                        caligo_bridge::probe_code::ERR_WRITE_FAILED => "ERR_WRITE_FAILED",
                        _ => "UNKNOWN",
                    }
                );
                match std::fs::read_to_string(obs_path) {
                    Ok(json) => {
                        println!("=== obs report ({}) ===", obs_path.display());
                        println!("{json}");
                        // F1-R5:活内存代码转储落盘 + 反汇编(内存域真相)。
                        if let Ok(rep) =
                            serde_json::from_str::<caligo_bridge::obs::ObsReport>(&json)
                        {
                            for dump in &rep.code_dumps {
                                if dump.hex.is_empty() {
                                    continue;
                                }
                                let bytes = match decode_hex(&dump.hex) {
                                    Ok(b) => b,
                                    Err(e) => {
                                        eprintln!("hex 解码失败({}): {e}", dump.label);
                                        continue;
                                    }
                                };
                                let bin = obs_path.with_file_name(format!(
                                    "{}-{}-{:#x}.bin",
                                    obs_path
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("obs"),
                                    dump.label,
                                    dump.rva
                                ));
                                if let Err(e) = std::fs::write(&bin, &bytes) {
                                    eprintln!("转储写盘失败 {}: {e}", bin.display());
                                    continue;
                                }
                                println!(
                                    "=== live code {} @ {:#x} ({} bytes) ===",
                                    dump.label,
                                    dump.rva,
                                    bytes.len()
                                );
                                match disasm::disasm_at(&bytes, 0, dump.rva, bytes.len()) {
                                    Ok(text) => print!("{text}"),
                                    Err(e) => eprintln!("disasm: {e}"),
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("obs 报告读取失败 {}: {e}", obs_path.display());
                        return ExitCode::FAILURE;
                    }
                }
            }
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
            // env 链路线程异步执行:等待其完成(最多 30 秒),然后打印 JSONL 报告。
            if let Some(env_path) = env_report.as_deref() {
                println!("[env] 等待自建环境链路完成(最多 30s)…");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                loop {
                    if let Ok(content) = std::fs::read_to_string(env_path) {
                        if content.contains("\"stage\":\"done\"") {
                            break;
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                match std::fs::read_to_string(env_path) {
                    Ok(content) => {
                        println!("=== env report ({}) ===", env_path.display());
                        println!("{content}");
                    }
                    Err(e) => {
                        eprintln!("env 报告读取失败 {}: {e}", env_path.display());
                        return ExitCode::FAILURE;
                    }
                }
            }
            // WU3 interrupt 结果(JSONL 阶段报告)。
            if let (Some(req), Some(code)) = (intr.as_ref(), o.intr_exit_code) {
                println!(
                    "[intr] caligo_interrupt_run:exit_code={} ({})",
                    code,
                    match code {
                        0 => "OK(流程完成,触发与否见报告)",
                        1 => "ERR_NULL_PATH",
                        2 => "ERR_BAD_PATH",
                        3 => "ERR_NO_QQNT",
                        4 => "ERR_ENV_INVALID(候选页不可读,拒绝调用)",
                        5 => "ERR_EXPORT_MISSING",
                        6 => "ERR_VFTABLE_MISMATCH(vfptr 不符,拒绝调用)",
                        _ => "UNKNOWN",
                    }
                );
                match std::fs::read_to_string(&req.report) {
                    Ok(content) => {
                        println!("=== intr report ({}) ===", req.report.display());
                        println!("{content}");
                    }
                    Err(e) => {
                        eprintln!("intr 报告读取失败 {}: {e}", req.report.display());
                        return ExitCode::FAILURE;
                    }
                }
            }
            // K2-03 async 结果(JSONL 阶段报告)。
            if let (Some(req), Some(code)) = (async_req.as_ref(), o.async_exit_code) {
                println!(
                    "[async] caligo_async_run:exit_code={} ({})",
                    code,
                    match code {
                        0 => "OK(流程完成,触发与否见报告)",
                        1 => "ERR_NULL_PATH",
                        2 => "ERR_BAD_PATH",
                        3 => "ERR_ENV_INVALID(布局链断裂,拒绝继续)",
                        4 => "ERR_BAD_MODE",
                        5 => "ERR_UV(loop 不活/init 失败)",
                        6 => "ERR_NO_CAPTURE(中断点未捕获 entered 上下文)",
                        _ => "UNKNOWN",
                    }
                );
                match std::fs::read_to_string(&req.report) {
                    Ok(content) => {
                        println!("=== async report ({}) ===", req.report.display());
                        println!("{content}");
                    }
                    Err(e) => {
                        eprintln!("async 报告读取失败 {}: {e}", req.report.display());
                        return ExitCode::FAILURE;
                    }
                }
            }
            // K3-E exec 结果(JSONL 阶段报告)。
            if let (Some(req), Some(code)) = (exec_req.as_ref(), o.exec_exit_code) {
                println!(
                    "[exec] caligo_exec_run:exit_code={} ({})",
                    code,
                    match code {
                        0 => "OK(流程完成,结果见报告 result 行)",
                        1 => "ERR_NULL",
                        2 => "ERR_NO_QQNT",
                        3 => "ERR_NO_CONTEXT",
                        _ => "UNKNOWN",
                    }
                );
                let rep = &req.report;
                match std::fs::read_to_string(rep) {
                    Ok(content) => {
                        println!("=== exec report ({}) ===", rep.display());
                        println!("{content}");
                    }
                    Err(e) => {
                        eprintln!("exec 报告读取失败 {}: {e}", rep.display());
                        return ExitCode::FAILURE;
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[inject] 失败:{e}");
            eprintln!("恢复:以正常退出该测试 QQ 实例收尾(docs/research/recovery-notes.md §2)。");
            ExitCode::FAILURE
        }
    }
}



// ---------------------------------------------------------------------------
// K4-D7b:caligod 控制面客户端(纯 IPC,无 QQ 访问;不门控)。
// daemon-control --pipe <control-pipe> --auth <token> <health|stop|query <id>>
// ---------------------------------------------------------------------------

fn cmd_daemon_control(args: &[String]) -> ExitCode {
    let mut pipe: Option<String> = None;
    let mut auth = String::new();
    let mut cmd: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--pipe" => pipe = next(&mut i),
            "--auth" => auth = next(&mut i).unwrap_or_default(),
            other => cmd.push(other.to_string()),
        }
        i += 1;
    }
    let (Some(pipe), action) = (pipe, cmd.first().map(String::as_str)) else {
        eprintln!("daemon-control --pipe <control-pipe> --auth <token> <health|stop|query <id>|drain [n]>");
        return ExitCode::FAILURE;
    };
    // 连接 + Hello(重试等待 server 就绪)。短名自动补管道根(规避跨 shell
    // 反斜杠转换;与 daemon 侧 pipe_names 同规则)。
    let pipe = if pipe.starts_with("\\\\.\\pipe\\") {
        pipe
    } else {
        format!("\\\\.\\pipe\\{}", pipe.trim_start_matches('\\'))
    };
    let mut client = None;
    for _ in 0..100 {
        match caligo_core::daemon::ControlClient::connect(&pipe, &auth, "caligo-cli daemon-control") {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    let mut client = match client {
        Some(c) => c,
        None => {
            eprintln!("connect failed(server 未就绪或认证拒绝)");
            return ExitCode::FAILURE;
        }
    };
    use caligo_core::ipc::{ControlMsg, CoreToControlMsg};
    let req = match action {
        Some("health") => ControlMsg::Health {},
        Some("stop") => ControlMsg::Stop {},
        Some("query") => ControlMsg::QueryRequest {
            request_id: cmd.get(1).cloned().unwrap_or_default(),
        },
        Some("drain") => ControlMsg::DrainEvents {
            max: cmd.get(1).and_then(|s| s.parse().ok()).unwrap_or(16),
        },
        // D7-b:发送链路探针。格式:send <request-id> <kind> <peer> <text...>
        // LAB/现场验证 Dispatch→SendResult 全链;真实业务发送属 D9 准入。
        Some("send") => {
            let id = cmd.get(1).cloned().unwrap_or_default();
            let kind = cmd.get(2).cloned().unwrap_or_else(|| "private".into());
            let peer = cmd.get(3).cloned().unwrap_or_default();
            let text = cmd[4..].join(" ");
            if id.is_empty() || peer.is_empty() {
                eprintln!("send 需要: send <id> <private|group> <peer> <text...>");
                return ExitCode::FAILURE;
            }
            ControlMsg::SendText {
                request_id: id,
                target: serde_json::json!({"account":"10001","kind":kind,"peer":peer}),
                text,
                deadline_ms: 30_000,
            }
        }
        _ => {
            eprintln!("未知动作: {action:?}");
            return ExitCode::FAILURE;
        }
    };
    match client.request(req) {
        Ok(reply) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&reply).unwrap_or_else(|_| format!("{reply:?}"))
            );
            match action {
                Some("stop") if matches!(reply, CoreToControlMsg::Stopped { .. }) => ExitCode::SUCCESS,
                Some("health") if matches!(reply, CoreToControlMsg::HealthAck { .. }) => ExitCode::SUCCESS,
                _ => ExitCode::SUCCESS,
            }
        }
        Err(e) => {
            eprintln!("request: {e}");
            ExitCode::FAILURE
        }
    }
}
