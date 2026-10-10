//! N3 现场验证入口(计划 §7-N3):在活实例上以 N3 会话层完整跑一轮
//! attach → 基线 CPB → 单条发送 → 等待 → fresh 差分 → 全字段报告。
//! 研究构建专用;注入纪律(manifest + 指定实例)与 g2 同。

use crate::g3_ops::G3NativeOps;
use crate::native_session::NativeOps;
use crate::native_receive::{
    collect_record_fields, scan_cpb_objects_inproc, RpmSelfReader, SeenRecords,
};
use crate::native_response::SeqAllocator;
use crate::native_session::NativeSession;

/// 报告路径 UTF-16 读取(g2 同款)。
fn report_path_str(path: *const u16) -> String {
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(path, len))
    }
}

fn stage(path: *const u16, stage_name: &str, ok: bool, detail: &str) {
    let p = report_path_str(path);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    let line = serde_json::json!({
        "ts": ts, "tid": tid, "stage": stage_name, "ok": ok, "detail": detail
    });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.to_string().as_bytes());
        let _ = f.write_all(b"\n");
    }
}

/// 发送一条群消息(已证形状)并返回 wire body。
fn encode_group(text: &str, group_code: u64) -> Result<Vec<u8>, String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    caligo_model::qq_wire::send::encode_group_text(group_code, text, 0, 1, ts)
        .map_err(|e| e.to_string())
}

/// ctx 布局(N3 现场验证):
/// +0x00 wrapper_base (u64)
/// +0x08 report_path (*const u16)
/// +0x10 group_code (u64)
/// +0x18 text_ptr (*const u16,NUL 结尾 UTF-16)
/// +0x20 observe_ms (u32)
#[repr(C)]
pub struct G3SessionCtx {
    pub wrapper_base: u64,
    pub report_path: *const u16,
    pub group_code: u64,
    pub text_ptr: *const u16,
    pub observe_ms: u32,
}

/// N3 会话验证运行(有界同步):
/// ① attach(probe_ready)→ ② CPB 基线 → ③ enqueue_send(已证形状)
/// → ④ 等待 observe_ms → ⑤ fresh 差分 → ⑥ 全字段报告。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`G3SessionCtx`]。
pub unsafe fn g3_session_run(ctx: *const G3SessionCtx) -> u32 {
    let wb = (*ctx).wrapper_base as usize;
    let report_path = (*ctx).report_path;
    let group_code = (*ctx).group_code;
    let observe_ms = ((*ctx).observe_ms as u64).min(60_000).max(2_000);
    let rp = report_path_str(report_path);

    // 解码 text。
    let text = {
        let mut len = 0usize;
        while len < 4096 && *(*ctx).text_ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts((*ctx).text_ptr, len))
    };

    // ① attach(probe_ready;零副作用)。
    let mut ops = G3NativeOps::new(wb);
    if let Err(e) = ops.probe_ready(0) {
        stage(report_path, "n3_attach", false, &format!("{e:?}"));
        return 1;
    }
    stage(report_path, "n3_attach", true, "probe_ready PASS");

    // ② 会话 + CPB 基线。
    // 服务对象地址 = getter 租约 + 锚点核对的真实产物(probe_service);
    // ops.submit 每次调用自行重新租约,此地址仅作 proof 证据登记。
    let service_addr = match ops.probe_service() {
        Ok(a) => a,
        Err(e) => {
            stage(report_path, "n3_service", false, &format!("{e:?}"));
            return 2;
        }
    };
    let mut session: NativeSession<SeqAllocator<Box<dyn FnMut(u64) -> Result<(), std::io::Error>>>> =
        NativeSession::new(SeqAllocator::new(1, Box::new(|_| Ok(()))));
    if let Err(e) = session.attach(&mut ops, crate::native_session::AttachProof {
        module_baseline: "qq-9.9.33-52230".into(),
        anchor_candidates: 1,
        service_addr,
        host_generation: 1,
        session_generation: 1,
    }) {
        stage(report_path, "n3_session_attach", false, &format!("{e:?}"));
        return 2;
    }
    stage(report_path, "n3_session_attach", true, &format!("Ready svc={service_addr:#x}"));

    // CPB 基线。
    let rd = crate::native_receive::RpmSelfReader;
    let base_objs = scan_cpb_objects_inproc(wb as u64);
    let base_recs = collect_record_fields(&rd, &base_objs, 8192);
    let mut seen = SeenRecords::new();
    seen.absorb(&base_recs);
    stage(report_path, "n3_baseline", true, &format!("cpb={} records={}", base_objs.len(), base_recs.len()));

    // ③ enqueue_send(已证形状;wire body 由 N3 sink 形状编码)。
    let body = match encode_group(&text, group_code) {
        Ok(b) => b,
        Err(e) => {
            stage(report_path, "n3_encode", false, &e);
            return 3;
        }
    };
    stage(report_path, "n3_encode", true, &format!("len={}", body.len()));
    let cookie_val = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut cookie = crate::native_response::Cookie([0u8; 16]);
    cookie.0[..8].copy_from_slice(&cookie_val.to_le_bytes());
    cookie.0[8..16].copy_from_slice(&(cookie_val.wrapping_mul(0x5DEECE66D)).to_le_bytes());
    match session.enqueue_send(&mut ops, "n3-verify-1", cookie, &body) {
        crate::native_session::SendAcceptance::Dispatched { seq, .. } => {
            stage(report_path, "n3_dispatched", true, &format!("seq={seq} body_len={}", body.len()));
        }
        other => {
            stage(report_path, "n3_dispatched", false, &format!("{other:?}"));
            return 4;
        }
    }

    // ④ 等待。
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(observe_ms);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // ⑤ fresh 差分。
    let after_objs = scan_cpb_objects_inproc(wb as u64);
    let after_recs = collect_record_fields(&rd, &after_objs, 8192);
    let fresh = seen.absorb(&after_recs);
    stage(report_path, "n3_fresh", true, &format!("cpb={} fresh={}", after_objs.len(), fresh.len()));

    // ⑥ 写报告。
    let mut found = false;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&rp) {
        use std::io::Write;
        for (obj, rf) in &fresh {
            let hit = rf.text.as_deref() == Some(text.as_str());
            if hit { found = true; }
            let entry = serde_json::json!({
                "kind": "n3_record",
                "obj": obj,
                "sender": rf.sender_uid.as_deref().unwrap_or(""),
                "peer": rf.peer_uid.as_deref().unwrap_or(""),
                "text": rf.text.as_deref().unwrap_or(""),
                "seq": rf.seq,
                "time": rf.platform_time,
                "hit": rf.text.as_deref() == Some(text.as_str()),
            });
            let _ = writeln!(f, "{entry}");
        }
    }
    stage(report_path, "n3_done", found, &format!("fresh={} text_found={found}", fresh.len()));
    if found { 0 } else { 5 }
}
