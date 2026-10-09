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
    name = data[off:off+8].rstrip(b"\0").decode()
    vsize, vaddr, rawsize, rawoff = struct.unpack_from("<IIII", data, off+8)
    if name == ".pdata":
        pdata_rva, pdata_off, pdata_size = vaddr, rawoff, rawsize
entries = pdata_size // 12
def runtime(addr):
    lo, hi = 0, entries
    while lo < hi:
        mid = (lo+hi)//2
        begin = struct.unpack_from("<I", data, pdata_off+mid*12)[0]
        if begin <= addr: lo = mid+1
        else: hi = mid
    if lo:
        b, e, u = struct.unpack_from("<III", data, pdata_off+(lo-1)*12)
        if b <= addr < e: return (hex(b), hex(e), e-b)
    return None
for a in (0x3e6bbaf, 0x703c4f, 0x703cc1, 0x703d12, 0x703d94, 0x703e1f, 0x7339b3, 0x737474, 0x73c6ba, 0xb7d269, 0xb7cf7b, 0x31f818e, 0x293b855, 0x217e670, 0xd31c72, 0xd334f4):
    print(f"{a:#x}: {runtime(a)}")
