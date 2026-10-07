# worker: map OwnedEvent v2 -> full EventPayload (session kind, direction, source, msgTime)
p = 'crates/caligo-bridge/src/daemon_client.rs'
s = open(p, encoding='utf-8').read()
old = '''        for ev in resident.take_events() {
            let payload = EventPayload {
                event_seq: 0, // core 持久化时分配;本地 seq 仅用于 ACK 关联
                session_generation: cfg.session_generation,
                session: serde_json::json!({}),
                direction: "incoming".into(),
                sender: ev.sender.clone(),
                native_id: ev.native_id.clone(),
                text: String::new(),
                platform_time: None,
                observed_at_unix_ms: 0,
                source: "recv".into(),
            };
            let _ = ev.text_len;
            let seq = next_local_seq;'''
new = '''        for ev in resident.take_events() {
            // 方向判据:senderUin == 本账号 → SelfSent(不伪装 incoming);
            // 会话种类:chatType 1=private 2=group(opaque 透传)。
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
                    caligo_bridge::resident::EventSourceKind::Recv => "recv",
                    caligo_bridge::resident::EventSourceKind::Update => "update",
                }
                .into(),
            };
            let seq = next_local_seq;'''
assert old in s, 'worker event mapping not found'
s = s.replace(old, new)
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('worker mapping ok')
