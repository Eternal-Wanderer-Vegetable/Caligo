//! CLG1 帧编解码(K1 定义,K4-D7b 下沉至 model;bridge 与 core 共用)。
//!
//! 帧格式:`[u32 magic][u32 len(LE)][payload]`,最大帧 1 MiB(拟定初值)。
//! 解码器接受任意分块的字节流:**增量有界**(K4-D5 前置修复)——组装缓冲
//! 硬上限 `8 + MAX_FRAME_SIZE`,整块 payload 走快路径不经缓冲;超长帧只读
//! 头即拒绝。

/// 帧头魔数,"CLG1"。
pub const FRAME_MAGIC: u32 = 0x314C_4743;
/// 最大帧(payload)字节数。拟定初值:1 MiB。
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

/// 帧编解码错误。半包不是错误(解码器等待更多字节);以下为确定性错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// 魔数不匹配:流错位或非本协议数据。致命,调用方应复位解码器。
    MagicMismatch(u32),
    /// 声明的 payload 长度超过 [`MAX_FRAME_SIZE`]。致命。
    FrameTooLarge(usize),
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::MagicMismatch(got) => write!(f, "frame magic mismatch: {got:#010x}"),
            FrameError::FrameTooLarge(got) => {
                write!(f, "frame too large: {got} > {MAX_FRAME_SIZE}")
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// 把一个 payload 编码为完整帧。
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_FRAME_SIZE {
        return Err(FrameError::FrameTooLarge(payload.len()));
    }
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// 流式帧解码器:容忍半包,产出完整 payload。
///
/// **不把任意大小的输入整块复制进缓存** —— 内部缓存严格有界
/// (`8 + MAX_FRAME_SIZE` 字节);超长声明只读头即拒绝,不缓冲其内容。
#[derive(Debug, Default)]
pub struct FrameDecoder {
    /// 当前帧的组装缓冲(header + 已收到的 payload 前缀)。
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前缓存字节数(测试/观测用;恒 ≤ 8 + [`MAX_FRAME_SIZE`])。
    pub fn pending_bytes(&self) -> usize {
        self.buf.len()
    }

    /// 送入一段字节;完整帧的 payload 追加到 `out`。
    ///
    /// 返回 `Err` 时解码器状态不可信,调用方应丢弃重建(流已错位)。
    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<Vec<u8>>) -> Result<(), FrameError> {
        let mut ci = 0usize;
        // 1) 补齐 header(若上一轮停在半包头)。
        if self.buf.len() < 8 {
            let take = (8 - self.buf.len()).min(chunk.len() - ci);
            self.buf.extend_from_slice(&chunk[ci..ci + take]);
            ci += take;
            if self.buf.len() < 8 {
                return Ok(());
            }
        }
        loop {
            // 不变量:buf 非空时,buf[0..8] 是完整 header。
            let (magic, len) = parse_header(&self.buf);
            if magic != FRAME_MAGIC {
                return Err(FrameError::MagicMismatch(magic));
            }
            if len > MAX_FRAME_SIZE {
                return Err(FrameError::FrameTooLarge(len));
            }
            let want_total = 8 + len;
            let mut payload_whole_in_chunk = false;
            if self.buf.len() < want_total {
                let need = want_total - self.buf.len();
                let avail = chunk.len() - ci;
                if self.buf.len() == 8 && avail >= need {
                    // 快路径:payload 完全位于 chunk,不经过组装缓冲。
                    payload_whole_in_chunk = true;
                } else {
                    let take = need.min(avail);
                    if take == 0 {
                        return Ok(());
                    }
                    self.buf.extend_from_slice(&chunk[ci..ci + take]);
                    ci += take;
                    if self.buf.len() < want_total {
                        return Ok(());
                    }
                }
            }
            if payload_whole_in_chunk {
                out.push(chunk[ci..ci + len].to_vec());
                ci += len;
            } else {
                let payload = self.buf.split_off(8);
                out.push(payload);
            }
            // 继续下一帧:重建组装缓冲,先补 header(不足 8 字节则等待)。
            self.buf.clear();
            let take = 8.min(chunk.len() - ci);
            self.buf.extend_from_slice(&chunk[ci..ci + take]);
            ci += take;
            if self.buf.len() < 8 {
                return Ok(());
            }
        }
    }
}

fn parse_header(buf: &[u8]) -> (u32, usize) {
    (
        u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
        u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_half_frame() {
        let payload = b"hello model framing";
        let frame = encode_frame(payload).unwrap();
        let mut d = FrameDecoder::new();
        let mut out = Vec::new();
        d.push(&frame[..5], &mut out).unwrap();
        assert!(out.is_empty());
        d.push(&frame[5..], &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], payload);
    }

    #[test]
    fn bounded_under_large_chunk() {
        let mut stream = Vec::new();
        for i in 0u32..50 {
            stream.extend_from_slice(&encode_frame(format!("f{i}").as_bytes()).unwrap());
        }
        let mut d = FrameDecoder::new();
        let mut out = Vec::new();
        d.push(&stream, &mut out).unwrap();
        assert_eq!(out.len(), 50);
        assert_eq!(d.pending_bytes(), 0);
    }
}
