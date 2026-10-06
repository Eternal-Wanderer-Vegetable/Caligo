//! 离线 MSVC RTTI 扫描(K2-02 WU1):定位 C++ 类的 vtable RVA。
//!
//! 依据(PE/COFF 规范 + MSVC 实现约定,source-register S3/S12):
//! - **TypeDescriptor(TD)**:`type_info` 对象 = vfptr(8B) + spare(8B) + 名字串;
//!   名字串形如 `.?AVEnvironment@node@@`(V=class/U=struct/D=enum...),NUL 结尾。
//!   故 `td_rva = name_rva - 0x10`。
//! - **CompleteObjectLocator(COL,x64 布局)**:
//!   `+0x00 signature`(x64 映像相对格式 = 1)、`+0x04 member_offset`(vftable 在
//!   完整对象内的偏移;主 vtable 为 0)、`+0x08 cdOffset`、`+0x0C pTypeDescriptor(RVA)`、
//!   `+0x10 pClassDescriptor(RVA)`、`+0x14 pSelf(RVA)`。
//! - **vftable**:函数指针数组,`[-1]` 槽存放指向 COL 的**完整 VA**(重定位后为
//!   加载基址+COL RVA;文件态即 file_ImageBase + col_rva)。故 vtable_rva = 槽位 + 8。
//!
//! 安全边界:只读文件字节;全部匹配都做结构校验(签名位 + pSelf 自指),不猜。

use crate::pe::parse_layout;

/// 一个引用了某 TD 的完整对象定位器。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ColHit {
    pub col_rva: u32,
    /// vftable 在完整对象内的偏移(0 = 主 vtable/完整对象定位器)。
    pub member_offset: u32,
    pub cd_offset: u32,
    /// 引用本 COL 的 vtable(即 COL 的自旋向上引用者)。
    pub vtables: Vec<VtableHit>,
}

/// 一个 vtable:COL 指针槽([-1])的 RVA 与首函数指针。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VtableHit {
    pub vtable_rva: u32,
    /// vtable[0] 的函数 RVA(不在任何段内时为 None)。
    pub first_fn_rva: Option<u32>,
}

/// 一个类型描述符及其引用链。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdHit {
    pub td_rva: u32,
    pub name: String,
    pub cols: Vec<ColHit>,
}

/// 在 `hay` 中找出 `needle` 的全部出现位置(首字节过滤的朴素搜索;足够用于
/// 百 MB 级映像 × 少量针,release 下秒级)。
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || hay.len() < needle.len() {
        return out;
    }
    let first = needle[0];
    let mut from = 0;
    while from <= hay.len() - needle.len() {
        let Some(rel) = hay[from..].iter().position(|&b| b == first) else {
            break;
        };
        let i = from + rel;
        if i > hay.len() - needle.len() {
            break;
        }
        if &hay[i + 1..i + needle.len()] == &needle[1..] {
            out.push(i);
        }
        from = i + 1;
    }
    out
}

/// 把文件偏移映射为 RVA(给定布局;不在任何段原始数据内返回 None)。
fn off_to_rva(layout: &crate::pe::PeLayout, off: usize) -> Option<u32> {
    for s in &layout.sections {
        let start = s.raw_ptr as usize;
        if off >= start && off < start + s.raw_size as usize {
            return Some(s.va + (off - start) as u32);
        }
    }
    None
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    b.get(off..off + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

/// 判断文件偏移处的字节是否构成一个 TD 名字串(`.?A?` 开头、可打印、NUL 结尾)。
/// 返回名字串内容(不含 NUL)。
fn td_name_at(file: &[u8], off: usize) -> Option<String> {
    const PREFIX: &[u8; 3] = b".?A";
    if file.get(off..off + 3) != Some(&PREFIX[..]) {
        return None;
    }
    let mut end = off;
    while end < file.len() && file[end] != 0 {
        let c = file[end];
        if !(0x20..0x7F).contains(&c) {
            return None;
        }
        end += 1;
        if end - off > 512 {
            return None;
        }
    }
    if end == off || end >= file.len() {
        return None;
    }
    Some(String::from_utf8_lossy(&file[off..end]).into_owned())
}

/// 全链扫描:名字含 `contains`(区分大小写子串)的 TD → 引用它们的 COL →
/// 引用 COL 的 vtable。返回按 TD RVA 排序的命中列表。
pub fn scan(file: &[u8], contains: &str) -> Result<Vec<TdHit>, String> {
    let layout = parse_layout(file)?;
    if layout.machine != 0x8664 {
        return Err(format!(
            "x64 MSVC RTTI 布局仅适用于 AMD64 映像(machine=0x{:04X})",
            layout.machine
        ));
    }
    let image_base = layout.image_base;

    // 阶段 1:收集全部段原始数据中的 TD 名字。
    let mut tds: Vec<(u32, String)> = Vec::new();
    for s in &layout.sections {
        if s.raw_size == 0 {
            continue;
        }
        let start = s.raw_ptr as usize;
        let end = start + s.raw_size as usize;
        let data = file.get(start..end).ok_or("section raw range out of file")?;
        // 名字串都由 ".?A" 引导;先定位前缀再验证完整串。
        for hit in find_all(data, b".?A") {
            let off = start + hit;
            if off < 0x10 {
                continue;
            }
            let Some(name) = td_name_at(file, off) else {
                continue;
            };
            if !name.contains(contains) {
                continue;
            }
            // TD 基址 = 名字 - 0x10;必须落在同一段内(名字在 TD 结构内部)。
            let td_off = off - 0x10;
            if td_off < start {
                continue;
            }
            let Some(td_rva) = off_to_rva(&layout, td_off) else {
                continue;
            };
            tds.push((td_rva, name));
        }
    }
    tds.sort();
    tds.dedup();

    // 阶段 2:对每个 TD 找 COL(x64 签名位 + pSelf 自指双重校验)。
    let mut out = Vec::with_capacity(tds.len());
    for (td_rva, name) in tds {
        let needle = td_rva.to_le_bytes();
        let mut cols: Vec<ColHit> = Vec::new();
        for s in &layout.sections {
            if s.raw_size < 0x18 {
                continue;
            }
            let start = s.raw_ptr as usize;
            let end = start + s.raw_size as usize;
            let Some(data) = file.get(start..end) else {
                continue;
            };
            for hit in find_all(data, &needle) {
                // 命中点须是 COL 的 pTypeDescriptor 字段(即 col_off = hit - 0xC)。
                if hit < 0xC {
                    continue;
                }
                let col_off = start + hit - 0xC;
                let Some(col_rva) = off_to_rva(&layout, col_off) else {
                    continue;
                };
                let sig = u32_at(file, col_off).unwrap_or(0);
                let member_offset = u32_at(file, col_off + 4).unwrap_or(0);
                let cd_offset = u32_at(file, col_off + 8).unwrap_or(0);
                let p_self = u32_at(file, col_off + 0x14).unwrap_or(0);
                if sig != 1 || p_self != col_rva {
                    continue; // 巧合数据,不是 COL。
                }
                // 阶段 3:找引用本 COL 的 vtable([-1] 槽存完整 VA)。
                let col_va = image_base + col_rva as u64;
                let col_needle = col_va.to_le_bytes();
                let mut vtables = Vec::new();
                for vs in &layout.sections {
                    if vs.raw_size < 8 {
                        continue;
                    }
                    let vstart = vs.raw_ptr as usize;
                    let vend = vstart + vs.raw_size as usize;
                    let Some(vdata) = file.get(vstart..vend) else {
                        continue;
                    };
                    for vhit in find_all(vdata, &col_needle) {
                        let slot_off = vstart + vhit;
                        let Some(slot_rva) = off_to_rva(&layout, slot_off) else {
                            continue;
                        };
                        let vtable_rva = slot_rva + 8;
                        let first_fn_rva = u64_at(file, slot_off + 8)
                            .filter(|va| *va >= image_base)
                            .map(|va| (va - image_base) as u32);
                        vtables.push(VtableHit {
                            vtable_rva,
                            first_fn_rva,
                        });
                    }
                }
                vtables.sort();
                vtables.dedup();
                cols.push(ColHit {
                    col_rva,
                    member_offset,
                    cd_offset,
                    vtables,
                });
            }
        }
        cols.sort();
        cols.dedup();
        out.push(TdHit {
            td_rva,
            name,
            cols,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最小 PE32+ 映像:头 + 单个 .rdata 段,内含 TD、COL、vtable。
    /// 布局(文件偏移 = RVA,FileAlignment = SectionAlignment = 0x1000 的简化
    /// 由 raw_ptr==va 实现):
    ///   0x1000: TD(vfptr, spare, name)
    ///   0x1040: COL(sig=1, off=0, cd=0, td_rva, chd_rva, self)
    ///   0x1080: [-1]=COL VA, fn0, fn1  → vtable_rva = 0x1088
    fn synth_image() -> Vec<u8> {
        let mut img = vec![0u8; 0x2000];
        img[0] = b'M';
        img[1] = b'Z';
        img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes()); // e_lfanew
        img[0x40..0x44].copy_from_slice(b"PE\0\0");
        img[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes()); // machine
        img[0x46..0x48].copy_from_slice(&1u16.to_le_bytes()); // number_of_sections
        // SizeOfOptionalHeader @ coff+16 = 0x54(coff=0x44)。
        img[0x54..0x56].copy_from_slice(&0xF0u16.to_le_bytes());
        let opt = 0x58; // coff+20
        img[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes()); // PE32+
        img[opt + 24..opt + 32].copy_from_slice(&0x1800_0000_0000u64.to_le_bytes()); // ImageBase
        let sec = opt + 0xF0;
        img[sec..sec + 8].copy_from_slice(b".rdata\0\0");
        img[sec + 8..sec + 12].copy_from_slice(&0x1000u32.to_le_bytes()); // vsize
        img[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // va
        img[sec + 16..sec + 20].copy_from_slice(&0x1000u32.to_le_bytes()); // raw_size
        img[sec + 20..sec + 24].copy_from_slice(&0x1000u32.to_le_bytes()); // raw_ptr

        let td_rva: u32 = 0x1000;
        let col_rva: u32 = 0x1040;
        let image_base: u64 = 0x1800_0000_0000;

        // TD:name 在 td+0x10。
        img[td_rva as usize + 0x10..td_rva as usize + 0x10 + 22]
            .copy_from_slice(b".?AVEnvironment@node@@");
        // COL。
        img[col_rva as usize..col_rva as usize + 4].copy_from_slice(&1u32.to_le_bytes());
        img[col_rva as usize + 4..col_rva as usize + 8].copy_from_slice(&0u32.to_le_bytes());
        img[col_rva as usize + 8..col_rva as usize + 12].copy_from_slice(&0u32.to_le_bytes());
        img[col_rva as usize + 0x0C..col_rva as usize + 0x10]
            .copy_from_slice(&td_rva.to_le_bytes());
        img[col_rva as usize + 0x10..col_rva as usize + 0x14]
            .copy_from_slice(&0x1200u32.to_le_bytes());
        img[col_rva as usize + 0x14..col_rva as usize + 0x18]
            .copy_from_slice(&col_rva.to_le_bytes());
        // vtable:[-1] = COL VA。
        let slot: usize = 0x1080;
        img[slot..slot + 8].copy_from_slice(&(image_base + col_rva as u64).to_le_bytes());
        img[slot + 8..slot + 16].copy_from_slice(&(image_base + 0x5000u64).to_le_bytes());
        img
    }

    #[test]
    fn finds_td_col_vtable_chain() {
        let img = synth_image();
        let hits = scan(&img, "Environment@node").expect("scan ok");
        assert_eq!(hits.len(), 1);
        let td = &hits[0];
        assert_eq!(td.td_rva, 0x1000);
        assert_eq!(td.name, ".?AVEnvironment@node@@");
        assert_eq!(td.cols.len(), 1);
        let col = &td.cols[0];
        assert_eq!(col.col_rva, 0x1040);
        assert_eq!(col.member_offset, 0);
        assert_eq!(col.vtables.len(), 1);
        let vt = &col.vtables[0];
        assert_eq!(vt.vtable_rva, 0x1088);
        assert_eq!(vt.first_fn_rva, Some(0x5000));
    }

    #[test]
    fn rejects_col_without_self_reference() {
        let mut img = synth_image();
        // 破坏 pSelf → 巧合 DWORD 不应被判为 COL。
        img[0x1040 + 0x14..0x1040 + 0x18].copy_from_slice(&0xDEADu32.to_le_bytes());
        let hits = scan(&img, "Environment@node").expect("scan ok");
        assert!(hits[0].cols.is_empty());
    }

    #[test]
    fn contains_filter_selects_names() {
        let img = synth_image();
        assert!(scan(&img, "Nonexistent@@").expect("scan ok").is_empty());
        // 大小写敏感:小写不匹配。
        assert!(scan(&img, "environment@node").expect("scan ok").is_empty());
    }

    #[test]
    fn find_all_finds_overlapping_and_all_positions() {
        assert_eq!(find_all(b"aaaa", b"aa"), vec![0, 1, 2]);
        assert_eq!(find_all(b"xyz", b"xyz"), vec![0]);
        assert_eq!(find_all(b"xy", b"xyz"), Vec::<usize>::new());
        assert_eq!(find_all(&[], b"a"), Vec::<usize>::new());
    }
}
