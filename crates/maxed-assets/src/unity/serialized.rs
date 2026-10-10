use super::{cancelled, header::Header, object::Object};
use crate::{checked_span, reader::Reader, AssetError};
use std::sync::atomic::AtomicBool;

pub(crate) struct Serialized {
    pub unity_version: String,
    pub objects: Vec<Object>,
    pub type_tree: bool,
}

impl Serialized {
    pub fn parse(
        bytes: &[u8],
        h: &Header,
        file_length: u64,
        cancel: &AtomicBool,
    ) -> Result<Self, AssetError> {
        let mut r = Reader::new(bytes, h.big);
        let unity_version = r.cstring()?;
        r.skip(4)?; // target platform
        let type_tree = r.boolean()?;
        let count = r.count(1_000_000)?;
        let mut classes = Vec::new();
        for _ in 0..count {
            cancelled(cancel)?;
            classes.push(read_type(&mut r, h.version, type_tree, false)?);
        }
        let count = r.count(1_000_000)?;
        let mut objects = Vec::new();
        for _ in 0..count {
            cancelled(cancel)?;
            r.align()?;
            let path_id = r.u64()? as i64; // local to this file
            let relative = if h.version >= 22 {
                r.u64()?
            } else {
                u64::from(r.u32()?)
            };
            let offset = h
                .data_offset
                .checked_add(relative)
                .ok_or(AssetError::InvalidStructure)?;
            let size = r.u32()?;
            let type_index = r.u32()? as usize;
            let class = *classes
                .get(type_index)
                .ok_or(AssetError::InvalidStructure)?;
            checked_span(offset, u64::from(size), file_length)?;
            objects.push(Object {
                path_id,
                offset,
                size,
                class,
            });
        }
        let scripts = r.count(1_000_000)?;
        for _ in 0..scripts {
            r.skip(4)?;
            r.align()?;
            r.skip(8)?;
        }
        let externals = r.count(1_000_000)?;
        for _ in 0..externals {
            r.cstring()?;
            r.skip(20)?;
            r.cstring()?;
        }
        if h.version >= 20 {
            let refs = r.count(1_000_000)?;
            for _ in 0..refs {
                cancelled(cancel)?;
                read_type(&mut r, h.version, type_tree, true)?;
            }
        }
        r.cstring()?;
        if r.pos != bytes.len() {
            return Err(AssetError::InvalidStructure);
        }
        Ok(Self {
            unity_version,
            objects,
            type_tree,
        })
    }
}

fn read_type(
    r: &mut Reader<'_>,
    version: u32,
    tree: bool,
    reference: bool,
) -> Result<i32, AssetError> {
    let class = r.i32()?;
    r.boolean()?;
    let script_index = r.u16()? as i16;
    if class == 114 || (reference && script_index >= 0) {
        r.skip(16)?;
    }
    r.skip(16)?;
    if tree {
        let nodes = r.count(1_000_000)?;
        let strings = r.count(16 * 1024 * 1024)?;
        r.skip(
            nodes
                .checked_mul(if version >= 19 { 32 } else { 24 })
                .ok_or(AssetError::LimitExceeded)?,
        )?;
        r.skip(strings)?;
        if version >= 21 {
            if reference {
                r.cstring()?;
                r.cstring()?;
                r.cstring()?;
            } else {
                let dependencies = r.count(1_000_000)?;
                r.skip(dependencies * 4)?;
            }
        }
    }
    Ok(class)
}
