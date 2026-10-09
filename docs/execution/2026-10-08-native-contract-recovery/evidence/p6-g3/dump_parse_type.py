import struct
path = "E:/stella/_reference/qq-native-r2-20261008/inputs/wrapper.node"
data = open(path, "rb").read()
pe_off = struct.unpack_from("<I", data, 0x3C)[0]
coff = pe_off + 4
nsec = struct.unpack_from("<H", data, coff+2)[0]
opt_size = struct.unpack_from("<H", data, coff+16)[0]
sec_off = coff + 20 + opt_size
sections = []
for i in range(nsec):
    off = sec_off + i*40
    vsize, vaddr, rawsize, rawoff = struct.unpack_from("<IIII", data, off+8)
    sections.append((vaddr, rawoff, rawsize))
def rva2off(r):
    for vaddr, rawoff, rawsize in sections:
        if vaddr <= r < vaddr + rawsize:
            return rawoff + (r - vaddr)
    return None
ti_rva = 0x4916F40
o = rva2off(ti_rva)
attr, unwind, fwd, cta = struct.unpack_from("<IIII", data, o)
co = rva2off(cta)
nct, ctlist = struct.unpack_from("<II", data, co)
print(f"cta@{cta:#x} nct={nct} list={ctlist:#x}")
for i in range(nct):
    raw, = struct.unpack_from("<I", data, rva2off(ctlist + 4*i))
    print(f"  entry[{i}] raw={raw:#x}")
    for cand in (raw, raw & ~1):
        cto = rva2off(cand)
        if cto is None:
            cto = rva2off(cand - 0x18000000)
        if cto is None: 
            continue
        props, td_rva = struct.unpack_from("<II", data, cto)
        tdo = rva2off(td_rva)
        if tdo is None:
            tdo = rva2off(td_rva - 0x18000000)
        if tdo is None: continue
        nameb = data[tdo+16:tdo+96].split(b"\0")[0]
        print(f"    type: {nameb.decode('latin1')}")
        break
