//! 有界 protobuf wire 编解码(自研,零第三方依赖)。
//!
//! 合同(T18):
//! - varint/fixed32/fixed64/length-delimited 支持;group(wire 3/4)拒绝;
//! - 未知字段由调用方以 [`Reader::skip`] 跳过并保留计数(不静默丢弃字节
//!   而无痕迹);
//! - 截断/坏 tag/varint 畸形显式报错;输入总量与嵌套深度有界。

use super::{CodecError, MAX_DEPTH};

/// wire types。
pub const WT_VARINT: u32 = 0;
pub const WT_FIXED64: u32 = 1;
pub const WT_LEN: u32 = 2;
pub const WT_FIXED32: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub field: u32,
    pub wire: u32,
}

pub fn decode_tag(v: u32) -> Result<Tag, CodecError> {
    let field = v >> 3;
    let wire = v & 7;
    if field == 0 {
        return Err(CodecError::BadTag(v));
    }
    if wire == 3 || wire == 4 {
        return Err(CodecError::BadTag(v)); // group 已废弃:拒绝
    }
    if !matches!(wire, WT_VARINT | WT_FIXED64 | WT_LEN | WT_FIXED32) {
        return Err(CodecError::BadTag(v));
    }
    Ok(Tag { field, wire })
}

/// 只读游标。
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    depth: usize,
    /// 跳过的未知字段计数(T18 痕迹)。
    pub skipped_unknown: u64,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Result<Self, CodecError> {
        Ok(Self { buf, pos: 0, depth: 0, skipped_unknown: 0 })
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        if self.remaining() < n {
            return Err(CodecError::Truncated { need: n, have: self.remaining() });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn varint(&mut self) -> Result<u64, CodecError> {
        let mut v: u64 = 0;
        let mut shift = 0u32;
        loop {
            if shift >= 70 {
                return Err(CodecError::VarintTooLong);
            }
            let b = self.take(1)?[0];
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }

    pub fn tag(&mut self) -> Result<Tag, CodecError> {
        decode_tag(self.varint()? as u32)
    }

    pub fn bytes_value(&mut self) -> Result<&'a [u8], CodecError> {
        let len = self.varint()? as usize;
        self.take(len)
    }

    pub fn fixed32(&mut self) -> Result<u32, CodecError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn fixed64(&mut self) -> Result<u64, CodecError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }

    /// 跳过一个值(任意 wire type);未知字段计数 +1(T18 痕迹)。
    pub fn skip(&mut self, tag: Tag) -> Result<(), CodecError> {
        match tag.wire {
            WT_VARINT => {
                self.varint()?;
            }
            WT_FIXED64 => {
                self.fixed64()?;
            }
            WT_LEN => {
                self.bytes_value()?;
            }
            WT_FIXED32 => {
                self.fixed32()?;
            }
            _ => return Err(CodecError::BadTag(tag.wire)),
        }
        self.skipped_unknown += 1;
        Ok(())
    }

    /// 进入 length-delimited 子消息(深度有界)。
    pub fn enter(&mut self) -> Result<Reader<'a>, CodecError> {
        let bytes = self.bytes_value()?;
        if self.depth + 1 > MAX_DEPTH {
            return Err(CodecError::TooDeep);
        }
        Ok(Reader { buf: bytes, pos: 0, depth: self.depth + 1, skipped_unknown: 0 })
    }
}

/// 写游标(容量有界)。
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    fn push_varint(&mut self, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(b);
                return;
            }
            self.buf.push(b | 0x80);
        }
    }

    pub fn varint_field(&mut self, field: u32, v: u64) {
        self.push_varint(((field << 3) | WT_VARINT) as u64);
        self.push_varint(v);
    }

    pub fn bytes_field(&mut self, field: u32, data: &[u8]) {
        self.push_varint(((field << 3) | WT_LEN) as u64);
        self.push_varint(data.len() as u64);
        self.buf.extend_from_slice(data);
    }

    pub fn u32_field(&mut self, field: u32, v: u32) {
        self.varint_field(field, v as u64);
    }

    /// 嵌套消息:先占位再回填长度(单遍)。
    pub fn nested<F: FnOnce(&mut Writer)>(&mut self, field: u32, fill: F) {
        self.push_varint(((field << 3) | WT_LEN) as u64);
        let len_pos = self.buf.len();
        self.buf.push(0); // 假设嵌套 < 128 字节时先写 1 字节占位
        let start = self.buf.len();
        fill(self);
        let written = self.buf.len() - start;
        if written < 0x80 {
            self.buf[len_pos] = written as u8;
        } else {
            // 重排:移除 1 字节占位,插入多字节 varint。
            let body: Vec<u8> = self.buf.split_off(start);
            self.buf.truncate(len_pos);
            self.push_varint(written as u64);
            self.buf.extend_from_slice(&body);
        }
    }

    pub fn string_field(&mut self, field: u32, s: &str) {
        self.bytes_field(field, s.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T18 面:半包/截断必须报 Truncated,不是吞掉。
    #[test]
    fn truncated_len_delimited_rejected() {
        let mut w = Writer::new();
        w.bytes_field(1, b"0123456789");
        let full = w.finish();
        let r = Reader::new(&full[..7]).unwrap();
        let mut rd = r;
        let err = rd.tag().and_then(|t| match t.wire {
            WT_LEN => rd.bytes_value().map(|_| ()),
            _ => rd.skip(t),
        });
        assert!(matches!(err, Err(CodecError::Truncated { .. })));
    }

    /// 未知字段可跳过且留痕。
    #[test]
    fn unknown_field_skipped_and_counted() {
        let mut w = Writer::new();
        w.varint_field(99, 7); // 未知
        w.varint_field(1, 5); // 已知
        let buf = w.finish();
        let mut r = Reader::new(&buf).unwrap();
        let mut saw_known = false;
        while !r.at_end() {
            let t = r.tag().unwrap();
            if t.field == 1 && t.wire == WT_VARINT {
                assert_eq!(r.varint().unwrap(), 5);
                saw_known = true;
            } else {
                r.skip(t).unwrap();
            }
        }
        assert!(saw_known);
        assert_eq!(r.skipped_unknown, 1);
    }

    /// 嵌套 roundtrip(含 >128 字节需要多字节长度回填的分支)。
    #[test]
    fn nested_roundtrip_multibyte_len() {
        let mut inner = Writer::new();
        inner.string_field(1, "x".repeat(300).as_str());
        let inner_bytes = inner.finish();
        let mut outer = Writer::new();
        outer.bytes_field(2, &inner_bytes);
        let buf = outer.finish();
        let mut r = Reader::new(&buf).unwrap();
        let t = r.tag().unwrap();
        assert_eq!((t.field, t.wire), (2, WT_LEN));
        let mut sub = r.enter().unwrap();
        let t2 = sub.tag().unwrap();
        assert_eq!(t2.field, 1);
        assert_eq!(sub.bytes_value().unwrap().len(), 300);
    }

    /// group wire type 拒绝。
    #[test]
    fn group_wire_rejected() {
        assert!(matches!(decode_tag((1 << 3) | 3), Err(CodecError::BadTag(_))));
        assert!(matches!(decode_tag(0), Err(CodecError::BadTag(_))));
    }
}
