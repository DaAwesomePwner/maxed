use crate::AssetError;
use std::ops::Range;

pub fn checked_span(offset: u64, length: u64, file_length: u64) -> Result<Range<u64>, AssetError> {
    let end = offset
        .checked_add(length)
        .ok_or(AssetError::InvalidStructure)?;
    if end > file_length {
        return Err(AssetError::InvalidStructure);
    }
    Ok(offset..end)
}

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pub pos: usize,
    pub big: bool,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], big: bool) -> Self {
        Self { data, pos: 0, big }
    }
    pub fn take(&mut self, len: usize) -> Result<&'a [u8], AssetError> {
        let span = checked_span(self.pos as u64, len as u64, self.data.len() as u64)?;
        self.pos = span.end as usize;
        Ok(&self.data[span.start as usize..span.end as usize])
    }
    pub fn skip(&mut self, len: usize) -> Result<(), AssetError> {
        self.take(len).map(|_| ())
    }
    pub fn align(&mut self) -> Result<(), AssetError> {
        self.skip((4 - self.pos % 4) % 4)
    }
    pub fn u8(&mut self) -> Result<u8, AssetError> {
        Ok(self.take(1)?[0])
    }
    pub fn boolean(&mut self) -> Result<bool, AssetError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(AssetError::InvalidStructure),
        }
    }
    pub fn u16(&mut self) -> Result<u16, AssetError> {
        let b = self
            .take(2)?
            .try_into()
            .map_err(|_| AssetError::InvalidStructure)?;
        Ok(if self.big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }
    pub fn u32(&mut self) -> Result<u32, AssetError> {
        let b = self
            .take(4)?
            .try_into()
            .map_err(|_| AssetError::InvalidStructure)?;
        Ok(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
    pub fn i32(&mut self) -> Result<i32, AssetError> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64, AssetError> {
        let b = self
            .take(8)?
            .try_into()
            .map_err(|_| AssetError::InvalidStructure)?;
        Ok(if self.big {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        })
    }
    pub fn count(&mut self, max: usize) -> Result<usize, AssetError> {
        let n = self.u32()? as usize;
        if n > max {
            Err(AssetError::LimitExceeded)
        } else {
            Ok(n)
        }
    }
    pub fn string(&mut self) -> Result<String, AssetError> {
        let len = self.count(4096)?;
        let value = std::str::from_utf8(self.take(len)?)
            .map_err(|_| AssetError::InvalidStructure)?
            .to_owned();
        self.align()?;
        Ok(value)
    }
    pub fn cstring(&mut self) -> Result<String, AssetError> {
        let start = self.pos;
        for _ in 0..4096 {
            if self.u8()? == 0 {
                return std::str::from_utf8(&self.data[start..self.pos - 1])
                    .map(str::to_owned)
                    .map_err(|_| AssetError::InvalidStructure);
            }
        }
        Err(AssetError::LimitExceeded)
    }
}
