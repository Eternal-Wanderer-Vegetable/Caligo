import ctypes

k32 = ctypes.windll.kernel32
pid = 26104
h = k32.OpenProcess(0x0400 | 0x0010, False, pid)
assert h, "OpenProcess failed"
buf = ctypes.c_uint64()
got = ctypes.c_size_t()
def rd(addr):
    ok = k32.ReadProcessMemory(h, ctypes.c_void_p(addr), ctypes.byref(buf), 8, ctypes.byref(got))
    return buf.value if ok and got.value == 8 else None

svc = 0x24b71a38000
f50 = rd(svc + 0x50)
f60 = rd(svc + 0x60)
print(f"svc+0x50 = {f50:#x}" if f50 is not None else "svc+0x50 unreadable")
print(f"svc+0x60 = {f60:#x}" if f60 is not None else "svc+0x60 unreadable")
if f50 and f60:
    print("=> SAME OBJECT" if f50 == f60 else "=> DIFFERENT OBJECTS")
    for label, t in (("0x50", f50), ("0x60", f60)):
        vptr = rd(t)
        if vptr:
            slot7 = rd(vptr + 0x38)
            s = f"  transport{label}: obj={t:#x} vptr={vptr:#x}"
            if slot7 is not None:
                s += f" slot7={slot7:#x}"
            print(s)
