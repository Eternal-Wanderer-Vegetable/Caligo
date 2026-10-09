import struct

path = "C:/Users/Vegetable/AppData/Roaming/QQ/Crashpad/reports/af77b408-7df7-4d14-ae2a-21177906344b.dmp"
data = open(path, "rb").read()
nstreams, dir_rva = struct.unpack_from("<II", data, 8)
streams = {}
for i in range(nstreams):
    st, sz, rva = struct.unpack_from("<III", data, dir_rva + i*12)
    streams.setdefault(st, []).append((rva, sz))
rva6, _ = streams[6][0]
code, flg, rec, addr = struct.unpack_from("<IIQQ", data, rva6+8)
ctx_size, ctx_rva = struct.unpack_from("<II", data, rva6+0xA0)
rsp, = struct.unpack_from("<Q", data, ctx_rva + 0x98)
rip, = struct.unpack_from("<Q", data, ctx_rva + 0xF8)
rva5, _ = streams[5][0]
nm, = struct.unpack_from("<I", data, rva5)
ranges = []
cum = rva5 + 4
for i in range(nm):
    sa, sz, mrva = struct.unpack_from("<QII", data, cum)
    ranges.append((sa, sz, mrva))
    cum += 16
def mem_read(va, size):
    for sa, sz, mrva in ranges:
        if sa <= va < sa + sz:
            o = mrva + (va - sa)
            return data[o:o+min(size, sa+sz-va)]
    return None
# dump ALL non-zero qwords rsp..rsp+0x600 with annotation for special ranges
print(f"rip={rip:#x} rsp={rsp:#x}")
TRAMP_LO, TRAMP_HI = 0x34123080000, 0x34123081000
for i in range(0, 0x600, 8):
    b = mem_read(rsp + i, 8)
    if not b or len(b) < 8: break
    v, = struct.unpack_from("<Q", b)
    if v == 0: continue
    tag = ""
    if TRAMP_LO <= v < TRAMP_HI: tag = "  <<<< TRAMPOLINE PAGE"
    print(f"[rsp+{i:#04x}] {v:#012x}{tag}")
