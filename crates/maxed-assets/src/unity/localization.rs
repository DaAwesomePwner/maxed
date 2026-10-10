use super::*;
use crate::reader::Reader;
use std::collections::BTreeMap;

const LIMIT: usize = 16 * 1024 * 1024;

pub(super) fn read(
    file: &mut File,
    object: &Object,
    big: bool,
    cancel: &AtomicBool,
    english: &mut BTreeMap<String, String>,
    retained: &mut usize,
    budget: usize,
) -> Result<(), AssetError> {
    cancelled(cancel)?;
    let prefix = read_span(file, object.offset, (object.size as usize).min(8192))?;
    let mut r = Reader::new(&prefix, big);
    // GameObject PPtr, enabled byte with padding, script PPtr.
    if r.skip(28).is_err() {
        return Ok(());
    }
    match r.string() {
        Ok(name) if name == "I2Languages" => {}
        _ => return Ok(()),
    }
    if object.size as usize > LIMIT
        || retained
            .checked_add(object.size as usize)
            .is_none_or(|n| n > budget)
    {
        return Err(AssetError::LimitExceeded);
    }
    let bytes = read_span(file, object.offset, object.size as usize)?;
    match parse(&bytes, big, cancel) {
        Ok((values, parsed_retained)) => {
            *retained = retained
                .checked_add(parsed_retained)
                .filter(|n| *n <= budget)
                .ok_or(AssetError::LimitExceeded)?;
            english.extend(values);
        }
        Err(error @ (AssetError::LimitExceeded | AssetError::Cancelled)) => return Err(error),
        Err(_) => tracing::warn!("unsupported localization source; using catalog IDs"),
    }
    Ok(())
}

fn parse(
    bytes: &[u8],
    big: bool,
    cancel: &AtomicBool,
) -> Result<(BTreeMap<String, String>, usize), AssetError> {
    cancelled(cancel)?;
    let mut r = Reader::new(bytes, big);
    r.skip(28)?;
    if r.string().map_err(layout_error)? != "I2Languages" {
        return Err(AssetError::InvalidStructure);
    }
    r.skip(12)?; // Source settings preceding mTerms.
    let count = r.count(65536).map_err(layout_error)?;
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        cancelled(cancel)?;
        terms.push(r.pos);
        r.string().map_err(layout_error)?;
        r.i32()?;
        for _ in 0..r.count(64).map_err(layout_error)? {
            translation(&mut r)?;
        }
        let flags = r.count(64).map_err(layout_error)?;
        r.skip(flags)?;
        r.align()?;
        // Current sources have a zero trailing word per term. Reject changed
        // layouts rather than guessing the representation of this field.
        if r.u32()? != 0 {
            return Err(AssetError::InvalidStructure);
        }
    }
    r.skip(12)?; // Source settings preceding mLanguages.
    let mut english = None;
    for index in 0..r.count(64).map_err(layout_error)? {
        cancelled(cancel)?;
        let name = r.string().map_err(layout_error)?;
        let code = r.string().map_err(layout_error)?;
        r.i32()?; // Language flags.
        if code == "en" || name == "English" {
            english = Some(index);
        }
    }
    let Some(english) = english else {
        return Ok((BTreeMap::new(), 0));
    };
    let mut out = BTreeMap::new();
    let mut retained = terms.capacity() * std::mem::size_of::<usize>();
    for pos in terms {
        cancelled(cancel)?;
        r.pos = pos;
        let key = r.string().map_err(layout_error)?;
        let kind = r.i32()?;
        for index in 0..r.count(64).map_err(layout_error)? {
            let value = translation(&mut r)?;
            if kind == 0 && index == english && !value.is_empty() {
                retained += key.len() + value.len() + 128;
                if retained > LIMIT {
                    return Err(AssetError::LimitExceeded);
                }
                out.insert(key.clone(), value.to_owned());
            }
        }
    }
    Ok((out, retained))
}

// Plausibility caps describe this layout, not the bounded source's resource use.
fn layout_error(error: AssetError) -> AssetError {
    match error {
        AssetError::LimitExceeded => AssetError::InvalidStructure,
        other => other,
    }
}

fn translation<'a>(r: &mut Reader<'a>) -> Result<&'a str, AssetError> {
    let len = r.count(64 * 1024).map_err(layout_error)?;
    let value = std::str::from_utf8(r.take(len)?).map_err(|_| AssetError::InvalidStructure)?;
    r.align()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(out: &mut Vec<u8>, value: &str) {
        out.extend((value.len() as u32).to_le_bytes());
        out.extend(value.as_bytes());
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }

    fn source() -> Vec<u8> {
        let mut bytes = vec![0; 28];
        string(&mut bytes, "I2Languages");
        bytes.extend([0; 12]);
        bytes.extend(2u32.to_le_bytes());
        for key in ["one", "two"] {
            string(&mut bytes, key);
            bytes.extend(0u32.to_le_bytes());
            bytes.extend(1u32.to_le_bytes());
            string(&mut bytes, "value");
            bytes.extend(1u32.to_le_bytes());
            bytes.extend([0; 8]); // Aligned flags and trailing word.
        }
        bytes.extend([0; 12]);
        bytes.extend(1u32.to_le_bytes());
        string(&mut bytes, "en");
        string(&mut bytes, "en");
        bytes.extend(0u32.to_le_bytes());
        bytes
    }

    #[test]
    fn source_budget_charges_parsed_data_and_checks_input_and_output() {
        let bytes = source();
        let cancel = AtomicBool::new(false);
        let (_, charge) = parse(&bytes, false, &cancel).unwrap();
        assert!(charge > bytes.len() && charge < 1024);
        let mut file = tempfile::tempfile().unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
        let object = Object {
            path_id: 1,
            offset: 0,
            size: bytes.len() as u32,
            class: 114,
        };
        for available in [bytes.len() - 1, charge - 1, charge] {
            let mut retained = 1000;
            let mut english = BTreeMap::new();
            let result = read(
                &mut file,
                &object,
                false,
                &cancel,
                &mut english,
                &mut retained,
                1000 + available,
            );
            if available < charge {
                assert!(matches!(result, Err(AssetError::LimitExceeded)));
                assert!(english.is_empty());
                assert_eq!(retained, 1000);
            } else {
                result.unwrap();
                assert_eq!(english.len(), 2);
                assert_eq!(retained, 1000 + charge);
            }
        }
    }

    #[test]
    fn cancellation_preserves_localization_and_budget() {
        let bytes = source();
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            parse(&bytes, false, &cancel),
            Err(AssetError::Cancelled)
        ));
        let mut file = tempfile::tempfile().unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
        let object = Object {
            path_id: 1,
            offset: 0,
            size: bytes.len() as u32,
            class: 114,
        };
        let mut english = BTreeMap::from([("existing".into(), "Existing".into())]);
        let before = english.clone();
        let mut retained = 1000;
        assert!(matches!(
            read(
                &mut file,
                &object,
                false,
                &cancel,
                &mut english,
                &mut retained,
                LIMIT,
            ),
            Err(AssetError::Cancelled)
        ));
        assert_eq!(english, before);
        assert_eq!(retained, 1000);
    }
}
