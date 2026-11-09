use maxed_types::{ByteOrigin, Direction, Endpoint, FlowKey, RawFrame, TcpSegment};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("Truncated frame")]
    Truncated,
    #[error("Invalid frame header")]
    InvalidHeader,
    #[error("Fragmented IPv4 is unsupported")]
    Fragmented,
    #[error("Unsupported network link or IP version")]
    UnsupportedLink,
}

pub(crate) fn be_u16(bytes: &[u8], offset: usize) -> Result<u16, ParseError> {
    let end = offset.checked_add(2).ok_or(ParseError::InvalidHeader)?;
    let part = bytes.get(offset..end).ok_or(ParseError::Truncated)?;
    Ok(u16::from_be_bytes([part[0], part[1]]))
}

pub fn parse_frame(frame: &RawFrame) -> Result<Option<TcpSegment>, ParseError> {
    let bytes = &frame.bytes;
    if bytes.len() > frame.original_len as usize {
        return Err(ParseError::InvalidHeader);
    }
    let mut kind = be_u16(bytes, 12)?;
    let mut start = 14;
    for _ in 0..2 {
        if !matches!(kind, 0x8100 | 0x88a8) {
            break;
        }
        kind = be_u16(bytes, start + 2)?;
        start += 4;
    }
    if matches!(kind, 0x8100 | 0x88a8 | 0x86dd) {
        return Err(ParseError::UnsupportedLink);
    }
    if kind != 0x0800 {
        return Ok(None);
    }
    let ip = bytes.get(start..).ok_or(ParseError::Truncated)?;
    let base = ip.get(..20).ok_or(ParseError::Truncated)?;
    let ihl = usize::from(base[0] & 15) * 4;
    if base[0] >> 4 != 4 || ihl < 20 {
        return Err(ParseError::InvalidHeader);
    }
    ip.get(..ihl).ok_or(ParseError::Truncated)?;
    let total = usize::from(be_u16(ip, 2)?);
    if total < ihl {
        return Err(ParseError::InvalidHeader);
    }
    let ip = ip.get(..total).ok_or(ParseError::Truncated)?;
    if base[9] != 6 {
        return Ok(None);
    }
    if be_u16(ip, 6)? & 0x3fff != 0 {
        return Err(ParseError::Fragmented);
    }
    if total < ihl + 20 {
        return Err(ParseError::InvalidHeader);
    }
    let tcp = ip.get(ihl..).ok_or(ParseError::Truncated)?;
    let head = tcp.get(..20).ok_or(ParseError::Truncated)?;
    let tcp_len = usize::from(head[12] >> 4) * 4;
    if tcp_len < 20 || tcp_len > tcp.len() {
        return Err(ParseError::InvalidHeader);
    }
    let source = Endpoint {
        ip: [base[12], base[13], base[14], base[15]],
        port: be_u16(tcp, 0)?,
    };
    let dest = Endpoint {
        ip: [base[16], base[17], base[18], base[19]],
        port: be_u16(tcp, 2)?,
    };
    let (flow, dir) = match (source.port == 2050, dest.port == 2050) {
        (true, true) => return Err(ParseError::InvalidHeader),
        (false, false) => return Ok(None),
        (true, false) => (
            FlowKey {
                client: dest,
                server: source,
            },
            Direction::ServerToClient,
        ),
        (false, true) => (
            FlowKey {
                client: source,
                server: dest,
            },
            Direction::ClientToServer,
        ),
    };
    let payload = tcp.get(tcp_len..).ok_or(ParseError::Truncated)?;
    Ok(Some(TcpSegment {
        flow,
        dir,
        at: frame.at,
        seq: u32::from_be_bytes([head[4], head[5], head[6], head[7]]),
        syn: head[13] & 2 != 0,
        fin: head[13] & 1 != 0,
        rst: head[13] & 4 != 0,
        bytes: payload.to_vec(),
        origin: ByteOrigin {
            frame_index: frame.index,
            offset: start + ihl + tcp_len,
            len: payload.len(),
        },
    }))
}

impl ParseError {
    /// Recover attribution only from a complete IPv4 address/port prefix.
    /// Noninitial fragments have no TCP header; never interpret their payload as ports.
    pub fn affected_flow(self, frame: &RawFrame) -> Option<(FlowKey, Direction)> {
        let bytes = &frame.bytes;
        let mut kind = be_u16(bytes, 12).ok()?;
        let mut start = 14;
        for _ in 0..2 {
            if !matches!(kind, 0x8100 | 0x88a8) {
                break;
            }
            kind = be_u16(bytes, start + 2).ok()?;
            start += 4;
        }
        if kind != 0x0800 {
            return None;
        }
        let ip = bytes.get(start..)?;
        let base = ip.get(..20)?;
        let ihl = usize::from(base[0] & 15) * 4;
        let total = usize::from(be_u16(ip, 2).ok()?);
        if base[0] >> 4 != 4
            || ihl < 20
            || base[9] != 6
            || be_u16(ip, 6).ok()? & 0x1fff != 0
            || total < ihl + 4
        {
            return None;
        }
        let ports = ip.get(ihl..ihl + 4)?;
        let source = Endpoint {
            ip: base[12..16].try_into().ok()?,
            port: be_u16(ports, 0).ok()?,
        };
        let dest = Endpoint {
            ip: base[16..20].try_into().ok()?,
            port: be_u16(ports, 2).ok()?,
        };
        match (source.port == 2050, dest.port == 2050) {
            (true, false) => Some((
                FlowKey {
                    client: dest,
                    server: source,
                },
                Direction::ServerToClient,
            )),
            (false, true) => Some((
                FlowKey {
                    client: source,
                    server: dest,
                },
                Direction::ClientToServer,
            )),
            _ => None,
        }
    }
}
