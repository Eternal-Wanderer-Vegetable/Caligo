//! 最小 PE/COFF 解析:只提取导出表所需字段(K1 静态调查用途)。
//!
//! 实现依据:Microsoft PE Format 文档(source-register S3)。
//! 只读文件字节,不执行任何 PE 内容。支持 PE32+(x64);PE32 也接受。

pub struct Export {
    pub ordinal: u32,
    pub name: String,
    pub function_rva: u32,
    /// 导出是转发器字符串(指向其他模块的 "module.symbol")时为 true。
    pub forwarded: bool,
}

pub struct ExportTable {
    pub machine: String,
    pub timestamp_utc: Option<String>,
    pub name_ordinal_base: u32,
    pub number_of_functions: u32,
    pub number_of_names: u32,
    pub dll_name: Option<String>,
    pub exports: Vec<Export>,
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn cstring_at(b: &[u8], off: usize) -> Option<String> {
    let start = b.get(off..)?;
    let end = start.iter().position(|c| *c == 0).unwrap_or(start.len());
    Some(String::from_utf8_lossy(&start[..end]).into_owned())
}

fn machine_name(m: u16) -> String {
    match m {
        0x8664 => "x64".into(),
        0x014C => "x86".into(),
        0xAA64 => "ARM64".into(),
        other => format!("0x{other:04X}"),
    }
}

struct Section {
    va: u32,
    vsize: u32,
    raw_ptr: u32,
    raw_size: u32,
}

fn rva_to_offset(sections: &[Section], rva: u32) -> Option<usize> {
    for s in sections {
        let span = s.vsize.max(s.raw_size);
        if rva >= s.va && rva < s.va + span {
            let delta = rva - s.va;
            if delta < s.raw_size {
                return Some((s.raw_ptr + delta) as usize);
            }
            return None; // 落在 BSS/未映射段,无文件偏移
        }
    }
    None
}

/// 解析 PE 导出表。任何结构性异常都以 Err(String) 返回,不做猜测性容错。
pub fn parse_export_table(file: &[u8]) -> Result<ExportTable, String> {
    if file.len() < 0x40 || &file[0..2] != b"MZ" {
        return Err("not an MZ image".into());
    }
    let e_lfanew = u32_at(file, 0x3C).ok_or("truncated DOS header")? as usize;
    if file.get(e_lfanew..e_lfanew + 4) != Some(&b"PE\0\0"[..]) {
        return Err("PE signature not found".into());
    }
    let coff = e_lfanew + 4;
    let machine = u16_at(file, coff).ok_or("truncated COFF header")?;
    let number_of_sections = u16_at(file, coff + 2).ok_or("truncated COFF header")? as usize;
    let size_of_optional = u16_at(file, coff + 16).ok_or("truncated COFF header")? as usize;
    let timestamp = u32_at(file, coff + 4).ok_or("truncated COFF header")?;

    let opt = coff + 20;
    let magic = u16_at(file, opt).ok_or("truncated optional header")?;
    let data_dir_offset = match magic {
        0x20B => opt + 112, // PE32+
        0x10B => opt + 96,  // PE32
        other => return Err(format!("unknown optional header magic 0x{other:04X}")),
    };
    let export_dir_rva = u32_at(file, data_dir_offset).ok_or("truncated data directories")?;
    let export_dir_size = u32_at(file, data_dir_offset + 4).ok_or("truncated data directories")?;

    let sec_base = opt + size_of_optional;
    let mut sections = Vec::with_capacity(number_of_sections);
    for i in 0..number_of_sections {
        let base = sec_base + i * 40;
        let vsize = u32_at(file, base + 8).unwrap_or(0);
        let va = u32_at(file, base + 12).unwrap_or(0);
        let raw_size = u32_at(file, base + 16).unwrap_or(0);
        let raw_ptr = u32_at(file, base + 20).unwrap_or(0);
        sections.push(Section {
            va,
            vsize,
            raw_ptr,
            raw_size,
        });
    }

    if export_dir_rva == 0 || export_dir_size == 0 {
        return Ok(ExportTable {
            machine: machine_name(machine),
            timestamp_utc: None,
            name_ordinal_base: 0,
            number_of_functions: 0,
            number_of_names: 0,
            dll_name: None,
            exports: Vec::new(),
        });
    }

    let dir_off = rva_to_offset(&sections, export_dir_rva)
        .ok_or("export directory RVA not mapped to file")?;
    let dll_name_rva = u32_at(file, dir_off + 12).ok_or("truncated export directory")?;
    let name_ordinal_base = u32_at(file, dir_off + 16).ok_or("truncated export directory")?;
    let number_of_functions = u32_at(file, dir_off + 20).ok_or("truncated export directory")?;
    let number_of_names = u32_at(file, dir_off + 24).ok_or("truncated export directory")?;
    let addr_functions = u32_at(file, dir_off + 28).ok_or("truncated export directory")?;
    let addr_names = u32_at(file, dir_off + 32).ok_or("truncated export directory")?;
    let addr_ordinals = u32_at(file, dir_off + 36).ok_or("truncated export directory")?;

    let fn_off = rva_to_offset(&sections, addr_functions).ok_or("AddressOfFunctions not mapped")?;
    let names_off = rva_to_offset(&sections, addr_names).ok_or("AddressOfNames not mapped")?;
    let ord_off =
        rva_to_offset(&sections, addr_ordinals).ok_or("AddressOfNameOrdinals not mapped")?;

    // 转发器判定:函数 RVA 落在导出目录自身范围内即为转发字符串。
    let mut exports = Vec::with_capacity(number_of_names as usize);
    for i in 0..number_of_names as usize {
        let name_rva = u32_at(file, names_off + i * 4).ok_or("names array truncated")?;
        let ordinal_index =
            u16_at(file, ord_off + i * 2).ok_or("ordinals array truncated")? as usize;
        let function_rva =
            u32_at(file, fn_off + ordinal_index * 4).ok_or("functions array truncated")?;
        let name_off = rva_to_offset(&sections, name_rva).ok_or("export name not mapped")?;
        let name = cstring_at(file, name_off).ok_or("export name unreadable")?;
        let forwarded =
            function_rva >= export_dir_rva && function_rva < export_dir_rva + export_dir_size;
        exports.push(Export {
            ordinal: name_ordinal_base + ordinal_index as u32,
            name,
            function_rva,
            forwarded,
        });
    }

    let dll_name = rva_to_offset(&sections, dll_name_rva).and_then(|o| cstring_at(file, o));

    // TimeDateStamp → UTC 可读格式(仅报告用)。
    let timestamp_utc = if timestamp == 0 {
        None
    } else {
        // 用标准库把 unix 秒转为 UTC 日期串(不引入 chrono)。
        let secs = timestamp as i64;
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
        // 民用历天数→Y-M-D(Howard Hinnant 算法)。
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        Some(format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}Z"))
    };

    Ok(ExportTable {
        machine: machine_name(machine),
        timestamp_utc,
        name_ordinal_base,
        number_of_functions,
        number_of_names,
        dll_name,
        exports,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_pe_bytes() {
        assert!(parse_export_table(b"not a pe file at all").is_err());
        assert!(parse_export_table(&[]).is_err());
        let mut mz = vec![0u8; 0x40];
        mz[0] = b'M';
        mz[1] = b'Z';
        // e_lfanew 指向空白,PE 签名缺失。
        assert!(parse_export_table(&mz).is_err());
    }
}
