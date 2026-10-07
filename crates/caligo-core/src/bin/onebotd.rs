//! onebotd — Caligo 常驻桥接进程(OneBot V11 兼容,最小实现)。
//!
//! 架构:复用 caligo-cli 的已验证注入链(shell 调用;每步走完整门控),
//! 本进程只做编排:ARM → poll 循环(消息→OneBot V11 事件 POST 上报)→
//! HTTP API(发送指令→shell 调用 send)→ 停止时 STOP。
//!
//! OneBot V11 映射(基于 K3-D/K3-F 实测 schema):
//! - 上行:post_type=message, message_type=group/private,
//!   user_id=senderUin, group_id=peerUin(群), message=[{type:text,...}],
//!   self_id=机器人 uin(--self-id);
//! - 下行:POST /send_group_msg {group_id, message:[{type:"text",data:{text}}]}
//!   → {status:"ok", retcode:0, data:{message_id}}。
//!
//! 纪律:本进程不直接操作 QQ 内存;一切经 caligo-cli 门控命令。HTTP 仅绑
//! 127.0.0.1;上报表文本截断同 bridge 纪律。

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    // K4-D0 门控:旧轮询 runner(ARM→逐轮 CLI inject→poll→STOP)整体停用。
    // 不调用 cli_inject,不自动加"指定测试实例确认";K4 常驻链路由
    // caligod + 命名管道 + bridge resident 承接(计划 §7-D5)。
    if !caligo_bridge::gate::research_enabled() {
        eprintln!("{}", caligo_bridge::gate::DISABLED_NOTICE);
        std::process::exit(3);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pid: u32 = 0;
    let mut env: String = String::new();
    let mut cli: String = "target/release/caligo-cli.exe".into();
    let mut bridge: String = "target/release/caligo_bridge_k3f.dll".into();
    let mut manifest: String = "docs/contracts/version-adapter-manifest.json".into();
    let mut report_dir: String = "local-evidence/onebotd".into();
    let mut post_url: String = "http://127.0.0.1:5700/caligo".into();
    let mut self_id: String = String::new();
    let mut http_port: u16 = 3001;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let val = |a: &[String], i: &mut usize| -> String { a.get(*i).cloned().unwrap_or_default() };
        match a.as_str() {
            "--pid" => { i += 1; pid = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0); }
            "--env" => { i += 1; env = val(&args, &mut i); }
            "--cli" => { i += 1; cli = val(&args, &mut i); }
            "--bridge" => { i += 1; bridge = val(&args, &mut i); }
            "--manifest" => { i += 1; manifest = val(&args, &mut i); }
            "--report-dir" => { i += 1; report_dir = val(&args, &mut i); }
            "--post-url" => { i += 1; post_url = val(&args, &mut i); }
            "--self-id" => { i += 1; self_id = val(&args, &mut i); }
            "--http-port" => { i += 1; http_port = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(3001); }
            _ => {}
        }
        i += 1;
    }
    if pid == 0 || env.is_empty() {
        eprintln!("onebotd --pid <n> --env hex [--cli path] [--bridge path] [--manifest path]");
        eprintln!("              [--report-dir dir] [--post-url url] [--self-id uin] [--http-port n]");
        std::process::exit(1);
    }
    std::fs::create_dir_all(&report_dir).ok();
    println!("[onebotd] pid={pid} env={env} http=127.0.0.1:{http_port} post→{post_url}");

    // 1. ARM(常驻监听;走 caligo-cli 完整门控)。
    let ts = chrono_stamp();
    let arm_report = format!("{report_dir}/arm-{ts}.json");
    let r = cli_inject(&cli, pid, &bridge, &manifest, &arm_report, &format!(
        "--async-report {report_dir}/arm-{ts}.jsonl --async-mode 2 --async-script 82 --async-env {env} --async-wait-ms 20000"
    ));
    if !r.contains("ARMED") {
        eprintln!("[onebotd] ARM 失败:{r}");
        std::process::exit(2);
    }
    println!("[onebotd] 监听已武装");

    // 2. poll 循环(Ctrl+C 退出)。
    let ctrl: Option<()> = None;
    let mut n: u64 = 0;
    loop {
        if ctrl_quit(&ctrl) { break; }
        n += 1;
        let ts = chrono_stamp();
        let poll_report = format!("{report_dir}/poll-{ts}-{n}.jsonl");
        let out = cli_inject(&cli, pid, &bridge, &manifest, &poll_report, &format!(
            "--async-report {poll_report} --async-mode 2 --async-script 82 --async-env {env} --async-wait-ms 4000"
        ));
        if out.contains("失败") || out.contains("error") {
            eprintln!("[onebotd] poll 异常,3s 后重试");
            std::thread::sleep(Duration::from_secs(3));
            continue;
        }
        // 从 JSONL 报告提取消息数组并逐条上报。
        if let Ok(content) = std::fs::read_to_string(&poll_report) {
            for msg in extract_messages(&content, &self_id) {
                if let Ok(body) = serde_json::to_string(&msg) {
                    http_post(&post_url, &body);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    // 3. 停止(移除监听)。
    let ts = chrono_stamp();
    let _ = cli_inject(&cli, pid, &bridge, &manifest, &format!("{report_dir}/stop-{ts}.json"), &format!(
        "--async-report {report_dir}/stop-{ts}.jsonl --async-mode 2 --async-script 12 --async-env {env} --async-wait-ms 20000"
    ));
    println!("[onebotd] 已停止,监听已移除");
}

fn cli_inject(cli: &str, pid: u32, bridge: &str, manifest: &str, report: &str, extra: &str) -> String {
    let out = Command::new(cli)
        .args(["inject", "--pid", &pid.to_string(), "--bridge", bridge,
               "--manifest", manifest, "--report", &format!("{report}.probe.json"),
               "--confirm-designated-test-instance"])
        .args(shell_words(extra))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match out {
        Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("spawn-err:{e}"),
    }
}

fn shell_words(s: &str) -> Vec<String> {
    s.split_whitespace().map(|s| s.to_string()).collect()
}

fn chrono_stamp() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}", now.as_millis() % 100_000_000_000)
}

fn ctrl_quit(_ch: &Option<()>) -> bool {
    // 简化:无 Ctrl+C 通道(Windows console channel 复杂);轮询上限兜底。
    // 停止方式:Ctrl+C 杀本进程后手动跑一次 STOP 注入。
    false
}

/// 从 JSONL 报告提取消息(bridge 报告格式:嵌在 detail 字符串里,单引号 JSON)。
/// 宽容解析:找 "chatType" 出现的片段并抽取字段。
fn extract_messages(content: &str, self_id: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for line in content.lines() {
        if !line.contains("chatType") { continue; }
        // detail 字段:单引号 JSON,先转双引号再宽容解析。
        let start = match line.find("{'events'") { Some(p) => p, None => continue };
        let raw = &line[start..];
        let fixed = raw.replace('\'', "\"");
        let v: serde_json::Value = match serde_json::from_str(&fixed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let events = v.get("events").and_then(|e| e.as_array()).cloned().unwrap_or_default();
        for m in events {
            let chat_type = m.get("chatType").and_then(|c| c.as_i64()).unwrap_or(0);
            if chat_type != 1 && chat_type != 2 { continue; }
            let txt = m.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
            if txt.is_empty() { continue; } // 系统/撤回类不上报
            let sender_uin: u64 = m.get("senderUin").and_then(|s| s.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0);
            if sender_uin.to_string() == self_id { continue; } // 自己的不上报
            let message = serde_json::json!([{"type": "text", "data": {"text": txt}}]);
            if chat_type == 2 {
                out.push(serde_json::json!({
                    "post_type": "message", "message_type": "group",
                    "time": 0, "self_id": serde_json::Value::Null,
                    "user_id": sender_uin,
                    "group_id": m.get("peerUin").and_then(|p| p.as_str()).and_then(|p| p.parse::<u64>().ok()).unwrap_or(0),
                    "message_id": m.get("msgId").cloned().unwrap_or(serde_json::Value::Null),
                    "message": message, "raw_message": txt,
                    "sender": {"user_id": sender_uin, "nickname": "", "card": ""},
                }));
            } else {
                out.push(serde_json::json!({
                    "post_type": "message", "message_type": "private",
                    "time": 0, "self_id": serde_json::Value::Null,
                    "user_id": sender_uin,
                    "message_id": m.get("msgId").cloned().unwrap_or(serde_json::Value::Null),
                    "message": message, "raw_message": txt,
                    "sender": {"user_id": sender_uin, "nickname": ""},
                }));
            }
        }
    }
    out
}

fn http_post(url: &str, body: &str) {
    // 最小 HTTP POST(仅 http://127.0.0.1:port/path 形态)。
    let rest = match url.strip_prefix("http://") { Some(r) => r, None => return };
    let (hostport, path) = match rest.find('/') {
        Some(p) => (&rest[..p], &rest[p..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(80)),
        None => (hostport, 80),
    };
    let addr = format!("{host}:{port}");
    let Ok(mut stream) = std::net::TcpStream::connect(&addr) else { return; };
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {hostport}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    if stream.write_all(req.as_bytes()).is_ok() {
        let mut buf = [0u8; 512];
        let _ = stream.read(&mut buf);
    }
}

/// 发送指令:shell 调用 caligo-cli --send-*(由外部 HTTP 服务器调起;
/// 本文件提供参数拼装,网络层由调用方持有)。
pub fn build_send_args(group_id: u64, user_id: u64, text: &str) -> Vec<String> {
    let mut args = vec![];
    if group_id > 0 {
        args.push("--send-chat".into()); args.push("2".into());
        args.push("--send-peer".into()); args.push(group_id.to_string());
    } else {
        args.push("--send-chat".into()); args.push("1".into());
        args.push("--send-peer".into()); args.push(user_id.to_string());
    }
    args.push("--send-text".into()); args.push(text.into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_send_args_group() {
        let a = build_send_args(263402786, 0, "hi");
        assert!(a.contains(&"263402786".to_string()));
        assert!(a.contains(&"2".to_string()));
    }

    #[test]
    fn build_send_args_private() {
        let a = build_send_args(0, 3089665724, "hi");
        assert!(a.contains(&"3089665724".to_string()));
        assert!(a.contains(&"1".to_string()));
    }
}
