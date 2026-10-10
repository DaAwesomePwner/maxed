use crate::AssetError;
use quick_xml::{events::Event, Reader};
use std::collections::BTreeMap;

#[derive(Default, Debug)]
pub(crate) struct Node {
    pub tag: String,
    pub attrs: BTreeMap<String, String>,
    pub text: String,
    pub children: Vec<Node>,
}
impl Node {
    /// Preorder traversal, matching DOM getElementsByTagName. The parser bounds
    /// each record to 8192 nodes and depth 64, including this iterator's stack.
    pub fn descendants(&self) -> impl Iterator<Item = &Node> {
        let mut stack: Vec<_> = self.children.iter().rev().collect();
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter().rev());
            Some(node)
        })
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|n| n.tag == name)
    }
    pub fn value(&self, name: &str) -> Option<&str> {
        self.child(name).map(|n| n.text.trim())
    }
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
}

/// Streaming outer document; only one bounded definition tree is retained.
pub(crate) fn records(
    xml: &[u8],
    emit: &mut dyn FnMut(Node) -> Result<(), AssetError>,
) -> Result<(), AssetError> {
    if xml.len() > 64 * 1024 * 1024 {
        return Err(AssetError::LimitExceeded);
    }
    let mut r = Reader::from_reader(xml);
    let mut stack: Vec<Node> = Vec::new();
    let mut depth = 0usize;
    let mut nodes = 0usize;
    let mut chars = 0usize;
    let mut roots = 0;
    let mut events = 0usize;
    loop {
        events += 1;
        if events > 4_000_000 {
            return Err(AssetError::LimitExceeded);
        }
        match r.read_event().map_err(|_| AssetError::InvalidXml)? {
            Event::Start(e) | Event::Empty(e) => {
                let empty = xml.get(r.buffer_position().saturating_sub(2) as usize) == Some(&b'/');
                depth += 1;
                if depth == 1 {
                    roots += 1;
                    if roots > 1 {
                        return Err(AssetError::InvalidXml);
                    }
                }
                if depth > 64 {
                    return Err(AssetError::LimitExceeded);
                }
                let tag = e.name().as_ref().to_owned();
                if !stack.is_empty()
                    || [
                        "Object",
                        "Ground",
                        "Enchantment",
                        "Crucible",
                        "DungeonModTemplate",
                    ]
                    .contains(&tag.as_str())
                {
                    if stack.is_empty() {
                        nodes = 0;
                        chars = 0;
                    }
                    nodes += 1;
                    if nodes > 8192 {
                        return Err(AssetError::LimitExceeded);
                    }
                    let mut node = Node {
                        tag,
                        ..Node::default()
                    };
                    chars += node.tag.len();
                    for a in e.attributes() {
                        let a = a.map_err(|_| AssetError::InvalidXml)?;
                        if node.attrs.len() >= 64 {
                            return Err(AssetError::LimitExceeded);
                        }
                        let key = a.key.as_ref().to_owned();
                        let value = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|_| AssetError::InvalidXml)?
                            .into_owned();
                        chars += key.len() + value.len();
                        if chars > 1024 * 1024 {
                            return Err(AssetError::LimitExceeded);
                        }
                        node.attrs.insert(key, value);
                    }
                    stack.push(node);
                    if empty {
                        finish(&mut stack, emit)?;
                    }
                }
                if empty {
                    depth -= 1;
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(AssetError::InvalidXml);
                }
                depth -= 1;
                if !stack.is_empty() {
                    finish(&mut stack, emit)?;
                }
            }
            Event::Text(e) => {
                let value = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                if let Some(node) = stack.last_mut() {
                    chars += value.len();
                    if chars > 1024 * 1024 {
                        return Err(AssetError::LimitExceeded);
                    }
                    node.text.push_str(&value);
                } else if depth == 0 && !value.trim().is_empty() {
                    return Err(AssetError::InvalidXml);
                }
            }
            Event::CData(e) => {
                if let Some(node) = stack.last_mut() {
                    let value = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                    chars += value.len();
                    if chars > 1024 * 1024 {
                        return Err(AssetError::LimitExceeded);
                    }
                    node.text.push_str(&value);
                }
            }
            Event::GeneralRef(e) => {
                let value =
                    if let Some(c) = e.resolve_char_ref().map_err(|_| AssetError::InvalidXml)? {
                        c.to_string()
                    } else {
                        match e.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            _ => return Err(AssetError::InvalidXml),
                        }
                        .to_owned()
                    };
                if let Some(node) = stack.last_mut() {
                    chars += value.len();
                    if chars > 1024 * 1024 {
                        return Err(AssetError::LimitExceeded);
                    }
                    node.text.push_str(&value);
                }
            }
            Event::DocType(_) => return Err(AssetError::InvalidXml),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || !stack.is_empty() || roots != 1 {
        return Err(AssetError::InvalidXml);
    }
    Ok(())
}
fn finish(
    stack: &mut Vec<Node>,
    emit: &mut dyn FnMut(Node) -> Result<(), AssetError>,
) -> Result<(), AssetError> {
    let node = stack.pop().ok_or(AssetError::InvalidXml)?;
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        emit(node)?;
    }
    Ok(())
}
pub(crate) fn integer(s: &str) -> Result<i64, AssetError> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map_err(|_| AssetError::InvalidCatalog)
    } else {
        s.parse().map_err(|_| AssetError::InvalidCatalog)
    }
}
pub(crate) fn num<T: TryFrom<i64>>(s: &str) -> Result<T, AssetError> {
    T::try_from(integer(s)?).map_err(|_| AssetError::InvalidCatalog)
}
