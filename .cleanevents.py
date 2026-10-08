import re

p = 'crates/caligo-bridge/src/daemon_client.rs'
s = open(p, encoding='utf-8').read()

# Remove the DEAD resident.take_events() block (hook forwards via events_rx now).
start = s.find('        for ev in resident.take_events() {')
assert start > 0
# find matching end: the block ends with "        }\n" before "// 2b)"? Find next top-level comment after it.
# The block: for ev in ... { ... } — find the closing "        }\n" by brace counting.
i = start
depth = 0
while True:
    ch = s[i]
    if ch == '{':
        depth += 1
    elif ch == '}':
        depth -= 1
        if depth == 0:
            end = i + 1
            break
    i += 1
# also swallow trailing newline
while s[end] == '\n':
    end += 1
removed = s[start:end]
assert 'wire_seq' not in removed, 'matched wrong block'
s = s[:start] + s[end:]

# Fix references: *acked_high / *next_local_seq in replay + window blocks
s = s.replace('.filter(|(s, _)| *s > acked_high)', '.filter(|(s, _)| *s > *acked_high)')
s = s.replace('replay.retain(|s| *s > acked_high);', 'replay.retain(|s| *s > *acked_high);')

# Remove now-dead duplicate "// 2b)" replay block (merge into one clean replay)
start2 = s.find('        // 2b) 事件上行(重放未确认 → 新事件;窗口满 → 记 Gap 丢最旧)。')
if start2 > 0:
    end2 = s.find('        // 3) 心跳', start2)
    assert end2 > start2
    replay_clean = '''        // 2b) 重放未确认(ACK 丢失场景;ACK 前移后自然收敛)。
        let replay: Vec<EventPayload> = unacked
            .iter()
            .filter(|(s, _)| *s > *acked_high)
            .map(|(_, e)| e.clone())
            .collect();
        for e in replay {
            if send_json(conn, &BridgeMsg::Event { event: e }).is_err() {
                return SessionEnd::Broken;
            }
            c.events_sent += 1;
        }
'''
    s = s[:start2] + replay_clean + s[end2:]

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('cleaned; removed dead block bytes:', len(removed))
