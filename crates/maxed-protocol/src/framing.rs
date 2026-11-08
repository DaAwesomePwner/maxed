use crate::ProtocolError;

pub const MAX_FRAME: usize = 200_000;

pub fn framed_len(header: &[u8]) -> Result<usize, ProtocolError> {
    let h = header.get(..4).ok_or(ProtocolError::Truncated)?;
    let len = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as usize;
    if !(5..=MAX_FRAME).contains(&len) {
        return Err(ProtocolError::Framing);
    }
    Ok(len)
}

#[derive(Default)]
pub struct Framer {
    buffer: Vec<u8>,
}

/// Completed frames precede any error from the remainder of the same chunk.
pub struct FramerOutput {
    pub frames: Vec<Vec<u8>>,
    pub error: Option<ProtocolError>,
}

impl Framer {
    pub fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }
    /// Incrementally buffer at most one bounded frame, even for huge input chunks.
    pub fn push(&mut self, mut bytes: &[u8]) -> FramerOutput {
        let mut frames = Vec::new();
        while !bytes.is_empty() {
            // The common case has a complete header in this chunk: allocate
            // the final bounded size before copying even the header.
            if self.buffer.is_empty() && bytes.len() >= 4 {
                if let Ok(len) = framed_len(bytes) {
                    self.buffer.reserve_exact(len);
                }
            }
            let target = if self.buffer.len() < 4 {
                4
            } else {
                match framed_len(&self.buffer) {
                    Ok(len) => len,
                    Err(e) => {
                        self.reset();
                        return FramerOutput {
                            frames,
                            error: Some(e),
                        };
                    }
                }
            };
            let count = (target - self.buffer.len()).min(bytes.len());
            self.buffer.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            if self.buffer.len() >= 4 {
                let len = match framed_len(&self.buffer) {
                    Ok(len) => len,
                    Err(e) => {
                        self.reset();
                        return FramerOutput {
                            frames,
                            error: Some(e),
                        };
                    }
                };
                self.buffer.reserve_exact(len - self.buffer.len());
                if self.buffer.len() == len {
                    frames.push(std::mem::take(&mut self.buffer));
                }
            }
        }
        FramerOutput {
            frames,
            error: None,
        }
    }
}
