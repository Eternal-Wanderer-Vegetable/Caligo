# ---- qq_v8: D9 send script template + send/poll-result helpers ----
p = 'crates/caligo-bridge/src/qq_v8.rs'
s = open(p, encoding='utf-8').read()

# poll JS: extend to include sendResults (K3 lastSend analog, keyed by mid)
old_poll = s.find('pub const D8_POLL_JS')
end_poll = s.find('"\";', old_poll)
assert old_poll > 0 and end_poll > old_poll
new_poll = '''pub const D8_POLL_JS: &str = r"JS
(function () {
  try {
    var G = globalThis;
    if (!G.__caligo_res) return 'NOT_ARMED';
    var R = G.__caligo_res;
    var out = JSON.stringify({ sid: R.sid, n: R.recv.length, items: R.recv, sendResults: R.sendResults || {} });
    R.recv = [];
    return out.slice(0, 900000);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
JS;"'''
s = s[:old_poll] + new_poll + s[end_poll + len('"\";'):]

# send template (K3 resident-send logic; mid 由 JS 生成并回传;Promise 结果写 R.sendResults[mid])
send = '''
/// D9 发送模板:占位 __CHAT__(数字)、__PEER__(JSON 字符串)、__TEXT__
/// (JSON 字符串)。fired 后 Promise 结果写入 R.sendResults[mid](done 带
/// 结果对象 / rejected 带错误),由轮询回收 —— §6.6:fired 非终态。
pub fn build_send_js(chat_type: u32, peer_uid: &str, text: &str) -> String {
    let peer_json = serde_json::to_string(peer_uid).unwrap_or_else(|_| "\\"\\"".into());
    let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\\"\\"".into());
    format!(
        r#"JS
(function () {{
  try {{
    var G = globalThis;
    if (!G.__caligo_res) return JSON.stringify({{ err: 'NOT_ARMED' }});
    var R = G.__caligo_res;
    R.sendResults = R.sendResults || {{}};
    var chat = {chat};
    var peerUid = {peer};
    var sv = 0;
    try {{ sv = R.session.getMSFService().getServerTime(); }} catch (e) {{}}
    var mid = String(R.ms.generateMsgUniqueId(chat, sv));
    var peer = {{ chatType: chat, guildId: mid, peerUid: peerUid }};
    var elems = [{{ elementType: 1, textElement: {{ content: {text} }} }}];
    var pr = R.ms.sendMsg('0', peer, elems, new Map());
    R.sendResults[mid] = {{ status: 'pending', ts: Date.now() }};
    if (pr && typeof pr.then === 'function') {{
      pr.then(function (res) {{
        try {{
          var rec = JSON.parse(JSON.stringify(res));
          var nid = null;
          try {{ nid = String(rec.msgId || (rec.msgList && rec.msgList[0] && rec.msgList[0].msgId) || ''); }} catch (e) {{}}
          R.sendResults[mid] = {{ status: 'done', msgId: nid, result: JSON.stringify(rec).slice(0, 2000) }};
        }} catch (e) {{ R.sendResults[mid] = {{ status: 'done-raw' }}; }}
      }}, function (err) {{
        R.sendResults[mid] = {{ status: 'rejected', error: String(err).slice(0, 300) }};
      }});
    }} else {{
      R.sendResults[mid] = {{ status: 'done-raw' }};
    }}
    return JSON.stringify({{ fired: true, mid: mid }});
  }} catch (e) {{ return JSON.stringify({{ err: String(e).slice(0, 300) }}); }}
}})()
JS;"#,
        chat = chat_type,
        peer = peer_json,
        text = text_json,
    )
}

/// D9 发送:fired → Ok((mid,)) / NOT_ARMED → Err(NoCurrentContext 之外的宿主错误另计)。
pub fn send_text(chat_type: u32, peer_uid: &str, text: &str) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    let js = build_send_js(chat_type, peer_uid, text);
    // SAFETY: 阶梯自管全部检查;调用方保证 owner 线程。
    let out = unsafe { exec_script(&syms, isolate_addr(), &js) }?;
    Ok(out)
}
'''
s = s.rstrip() + '\n' + send
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('qq_v8 send ok')
