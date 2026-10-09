# -*- coding: utf-8 -*-
"""P5 对象图字段测绘:从捕获头出发 BFS 通知对象图,定位关键字段的
(头偏移 → 对象 → 偏移) 路径。只读 RPM。"""
import json, struct, ctypes, sys
from collections import deque

PID = 21732
FIXTURE = r'docs/execution/2026-10-08-native-contract-recovery/evidence/p6-g2/g2-run-21732-r4-p5.jsonl'

msgs = {}
for line in open(FIXTURE, encoding='utf-8'):
    d = json.loads(line)
    if d.get('kind') == 'msg_head':
        msgs.setdefault(d['i'], []).append((d['q0'], d['q1']))
heads = {i: [v for pair in pairs for v in pair] for i, pairs in msgs.items()}

k32 = ctypes.windll.kernel32
h = k32.OpenProcess(0x410, 0, PID)
assert h
BUF = ctypes.create_string_buffer(0x400)
GOT = ctypes.c_size_t()
def rpm(addr, n=0x300):
    n = min(n, 0x400)
    if k32.ReadProcessMemory(h, ctypes.c_void_p(addr), BUF, n, ctypes.byref(GOT)) and GOT.value == n:
        return BUF.raw[:n]
    return None

BOT = 1694717255
SELF = 3089665724
TS_LO, TS_HI = 0x6AC30000, 0x6AD20000   # 2026-10-09 前后 unix 秒
MARKS = [
    (b'P5TEST', 'P5TEST'),
    (str(BOT).encode(), 'BOT_uin_str'),
    (struct.pack('<I', BOT), 'BOT_uin_u32'),
    (struct.pack('<Q', BOT), 'BOT_uin_u64'),
    (str(SELF).encode(), 'SELF_uin_str'),
    (b'u_', 'uid_prefix'),
]

def find_all(b, m, limit=2):
    out, pos = [], b.find(m)
    while pos >= 0 and len(out) < limit:
        out.append(pos); pos = b.find(m, pos + 1)
    return out

def scan_obj(addr):
    return rpm(addr)

def bfs(root_addrs, max_objs=64, max_depth=4):
    seen = set()
    q = deque([(a, None, None, 0) for a in root_addrs])  # (addr, parent, via_off, depth)
    order = []
    while q and len(order) < max_objs:
        addr, parent, via, depth = q.popleft()
        if addr in seen or addr < 0x10000:
            continue
        seen.add(addr)
        data = rpm(addr)
        if data is None:
            continue
        order.append((addr, parent, via, depth, data))
        if depth >= max_depth:
            continue
        for w in range(0, len(data) - 8, 8):
            v = struct.unpack_from('<Q', data, w)[0]
            if 0x1000_0000_000 <= v < 0x7FF0_0000_0000 and v not in seen:
                q.append((v, addr, w, depth + 1))
    return order

def annotate(data):
    out = []
    for m, name in MARKS:
        for pos in find_all(data, m):
            out.append((name, pos))
    # timestamps: u32 in range at 8-aligned offsets
    for w in range(0, min(len(data), 0x100) - 4, 8):
        v = struct.unpack_from('<I', data, w)[0]
        if TS_LO <= v <= TS_HI:
            out.append(('ts32', w))
    return out

CARRIERS = {
    9:  [('head+0x00', None)],      # Type P: +0x00 = 对象指针
    27: [('head+0x30', 6), ('head+0x38', 7)],  # Type S: 池指针
}
for i, roots in CARRIERS.items():
    q = heads[i]
    ras = []
    for name, idx in roots:
        if idx is None:
            ras.append((q[0], name))
        else:
            ras.append((q[idx], name))
    print(f"\n{'='*72}\n消息 i={i} 对象图 BFS (roots={[(hex(a), n) for a, n in ras]})")
    order = bfs([a for a, _ in ras])
    for addr, parent, via, depth, data in order:
        ann = annotate(data)
        vptr = struct.unpack_from('<Q', data, 0)[0]
        tag = ''
        if ann:
            tag = '  ' + ' '.join(f"{n}@+{p:#x}" for n, p in ann[:6])
        print(f"  d{depth} {addr:#x} (via {'head' if parent is None else f'{parent:#x}+{via:#x}'}) vptr={vptr:#x}{tag}")
        if any(n == 'P5TEST' for n, _ in ann):
            # 打印 P5TEST 附近结构
            for n, p in ann:
                if n == 'P5TEST':
                    lo = max(0, p - 16)
                    print(f"      ctx[{lo:#x}:+{p+len('P5TEST')+16:#x}]: {data[lo:p+24].hex(' ')}")
