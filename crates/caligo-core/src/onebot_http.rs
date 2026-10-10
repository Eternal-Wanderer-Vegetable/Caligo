//! OneBot HTTP transport(N4;计划 §7-N4 顺序第一档:HTTP API)。
//!
//! std 零依赖实现:POST JSON 动作 → [`crate::onebot`] 动作层 → JSON 响应。
//! 安全边界:请求体上限(默认 1 MiB,超限 413);仅 POST;路径不限制
//! (OneBot HTTP API 惯例为 POST /);鉴权(access token)随配置交付。
//!
//! 事件上报(HTTP POST 上报)与 WS 在后续批次;本模块只做动作端点。

use crate::onebot::{parse_action, ActionResponse};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

pub const DEFAULT_MAX_BODY: usize = 1024 * 1024;

/// 纯函数:动作 JSON 文本 → HTTP 响应(状态行 + JSON 体)。
/// handler 由调用方注入(动作层 + sink + store 的组装点)。
pub fn handle_http_body(
    body: &[u8],
    max_body: usize,
    handler: impl FnOnce(&str, &Value, Option<Value>) -> ActionResponse,
) -> (u16, String) {
    if body.len() > max_body {
        return (
            413,
            r#"{"status":"failed","retcode":1413,"wording":"请求体超限"}"#.to_string(),
        );
    }
    let text = match std::str::from_utf8(body) {
        Ok(t) => t,
        Err(_) => {
            return (
                400,
                r#"{"status":"failed","retcode":1440,"wording":"非 UTF-8 请求体"}"#.to_string(),
            )
        }
    };
    match parse_action(text) {
        Ok((action, params, echo)) => {
            let resp = handler(&action, &params, echo);
            (200, serde_json::to_string(&resp).unwrap_or_default())
        }
        Err(e) => {
            let resp = ActionResponse::failed(crate::onebot::RET_BAD_REQUEST, &e, None);
            (400, serde_json::to_string(&resp).unwrap_or_default())
        }
    }
}

/// 解析 HTTP 请求头,返回 (method, path, content_length)。
fn parse_request_head(buf: &[u8]) -> Option<(String, String, usize)> {
    let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let head = std::str::from_utf8(&buf[..header_end]).ok()?;
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut content_length = 0usize;
    for l in lines {
        let lower = l.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().ok()?;
        }
    }
    Some((method, path, content_length))
}

/// 处理一条已连接的 HTTP 会话(一请求一响应;Connection: close)。
/// 返回 (method, path, 请求体) 供日志;错误以 HTTP 状态码响应。
pub fn serve_one(
    mut stream: TcpStream,
    max_body: usize,
    handler: impl FnOnce(&str, &Value, Option<Value>) -> ActionResponse,
) -> (String, String, Vec<u8>) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let (method, path, content_length) = loop {
        match stream.read(&mut tmp) {
            Ok(0) | Err(_) => return ("GET".into(), "/".into(), Vec::new()),
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some((m, p, cl)) = parse_request_head(&buf) {
                    if buf.len() >= cl.min(max_body) + (buf.len() - cl).max(0) || buf.len() >= 4 + cl {
                        break (m, p, cl);
                    }
                    if cl > max_body {
                        let body = format!(
                            r#"{{"status":"failed","retcode":1413,"wording":"请求体超限 {cl}"}}"#
                        );
                        respond(&mut stream, 413, &body);
                        return (m, p, Vec::new());
                    }
                } else if buf.len() > 64 * 1024 {
                    return ("GET".into(), "/".into(), Vec::new());
                }
            }
        }
    };
    if method != "POST" {
        respond(&mut stream, 405, r#"{"status":"failed","retcode":1405,"wording":"仅 POST"}"#);
        return (method, path, Vec::new());
    }
    // 定位 body:headers 之后的 content_length 字节。
    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(buf.len());
    let body = buf[header_end..].to_vec();
    let (code, resp) = handle_http_body(&body, max_body, handler);
    respond(&mut stream, code, &resp);
    (method, path, body)
}

fn respond(stream: &mut TcpStream, code: u16, body: &str) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// accept 循环(阻塞;单线程串行处理 —— OneBot HTTP API 的动作天然串行)。
/// `stop` 返回 true 时退出。返回处理的请求数(诊断用)。
pub fn serve_loop(
    listener: &TcpListener,
    max_body: usize,
    stop: impl Fn() -> bool,
    handler: &mut dyn FnMut(&str, &Value, Option<Value>) -> ActionResponse,
) -> usize {
    let mut served = 0usize;
    listener.set_nonblocking(true).ok();
    loop {
        if stop() {
            return served;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                serve_one(stream, max_body, |a, p, e| handler(a, p, e));
                served += 1;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return served,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_store::MessageStore;
    use crate::onebot::{handle_action, StoreReader};
    use std::io::Write as _;

    fn handler(action: &str, params: &Value, echo: Option<Value>) -> ActionResponse {
        let mut store = MessageStore::new(1);
        // 测试 sink:记录并受理。
        struct SpySink(std::sync::Mutex<Vec<String>>);
        impl caligo_model::onebot_traits::SendSink for SpySink {
            fn send(
                &mut self,
                p: &caligo_model::onebot_traits::SendPayload,
            ) -> Result<bool, String> {
                self.0.lock().unwrap().push(p.text.clone());
                Ok(true)
            }
        }
        let mut spy = SpySink(std::sync::Mutex::new(Vec::new()));
        handle_action(action, params, echo, &mut spy, &store)
    }

    fn http_post(port: u16, body: &str) -> (u16, String) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        s.read_to_string(&mut resp).unwrap();
        let code: u16 = resp
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let body_start = resp.find("\r\n\r\n").map(|p| p + 4).unwrap_or(0);
        (code, resp[body_start..].to_string())
    }

    #[test]
    fn http_roundtrip_get_version_info() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve_loop(&listener, DEFAULT_MAX_BODY, || false, &mut handler);
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let (code, body) = http_post(port, r#"{"action":"get_version_info","echo":"x"}"#);
        assert_eq!(code, 200);
        assert!(body.contains(r#""status":"ok""#));
        assert!(body.contains(r#""echo":"x""#));
        assert!(body.contains("v11"));
    }

    #[test]
    fn http_bad_json_400_and_get_405() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            serve_loop(&listener, DEFAULT_MAX_BODY, || false, &mut handler);
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let (code, body) = http_post(port, "not json");
        assert_eq!(code, 400);
        assert!(body.contains("1440"));
        // GET → 405(std 读到 Connection: close 前)。
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut resp = String::new();
        s.read_to_string(&mut resp).unwrap();
        assert!(resp.starts_with("HTTP/1.1 405"));
    }
}
