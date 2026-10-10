//! RealmShark AssetExtractor/UnityExtractor ID-list compatibility adapter.
//!
//! The checked-in Windows oracle uses Windows-1252 replacement encoding and LF,
//! including a final LF. Freeze that byte profile rather than depending on the
//! machine's locale/JDK or native newline. Java's current writer uses its default
//! charset and System.lineSeparator, so caches from other profiles can differ.
use crate::{unity::cancelled, AssetError, TextAsset};
use quick_xml::{events::Event, Reader};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::AtomicBool,
};

#[derive(Debug)]
pub struct ReferenceIdLists {
    pub object_ids: Vec<u8>,
    pub tile_ids: Vec<u8>,
}

const NON_XML: &[&str] = &[
    "manifest_xml",
    "COPYING",
    "Errors",
    "ExplainUnzip",
    "cloth_bazaar",
    "Cursors",
    "Dialogs",
    "Keyboard",
    "LICENSE",
    "LineBreaking Following Characters",
    "LineBreaking Leading Characters",
    "manifest_json",
    "spritesheetf",
    "iso_4217",
    "data",
    "manifest",
    "BillingMode",
];

/// Reproduce the reference Windows extraction profile, without changing Catalog
/// semantics. Duplicate IDs remain separate rows; IdToAsset's reader applies the
/// last row. Sources use NTFS upcase ordinal order for ASCII filenames after
/// UnityExtractor's collision suffixing; rows use Java's stable signed ID sort. Unsupported
/// filenames fail closed instead of guessing a filesystem's Unicode collation.
pub fn build_reference_id_lists(
    texts: &[TextAsset],
    cancel: &AtomicBool,
) -> Result<ReferenceIdLists, AssetError> {
    cancelled(cancel)?;
    if texts.len() > 100_000 {
        return Err(AssetError::LimitExceeded);
    }
    let mut total = 0usize;
    let mut names = BTreeSet::new();
    let mut folded = BTreeSet::new();
    let mut sources = Vec::new();
    for text in texts {
        cancelled(cancel)?;
        total = total
            .checked_add(text.bytes.len())
            .ok_or(AssetError::LimitExceeded)?;
        if total > 128 * 1024 * 1024 || text.bytes.len() > 64 * 1024 * 1024 {
            return Err(AssetError::LimitExceeded);
        }
        if NON_XML.contains(&text.name.as_str()) {
            continue;
        }
        if text.name.is_empty()
            || !text.name.is_ascii()
            || text.name.contains(['/', '\\', ':', '\0'])
            || text.name.len() > 240
        {
            return Err(AssetError::InvalidStructure);
        }
        let mut name = text.name.clone();
        let mut suffix = 1;
        while !names.insert(name.clone()) {
            suffix += 1;
            name = format!("{}{suffix}", text.name);
        }
        let name = format!("{name}.xml").to_ascii_uppercase();
        if !folded.insert(name.clone()) {
            return Err(AssetError::InvalidStructure);
        }
        sources.push((name, text));
    }
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    let mut objects = Vec::new();
    let mut tiles = Vec::new();
    let mut retained = 0usize;
    for (_, text) in sources {
        cancelled(cancel)?;
        let bytes = text
            .bytes
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&text.bytes);
        // Unity includes performance-test JSON among the TextAssets.
        if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
            return Err(AssetError::InvalidXml);
        }
        if !bytes
            .iter()
            .take(256)
            .copied()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|b| b == b'<')
        {
            continue;
        }
        records(bytes, cancel, &mut |node| {
            // DOM getElementsByTagName also includes nested definitions.
            for n in std::iter::once(&node).chain(node.descendants()) {
                cancelled(cancel)?;
                let target = match n.tag.as_str() {
                    "Object" => &mut objects,
                    "Ground" => &mut tiles,
                    _ => continue,
                };
                let id = n.attr("type").map(java_integer).transpose()?.unwrap_or(0);
                let row = if n.tag == "Object" {
                    object_row(n, id)?
                } else {
                    tile_row(n, id)?
                };
                retained = retained
                    .checked_add(row.len() + 64)
                    .ok_or(AssetError::LimitExceeded)?;
                if retained > 64 * 1024 * 1024 || target.len() >= 100_000 {
                    return Err(AssetError::LimitExceeded);
                }
                target.push((id, row));
            }
            Ok(())
        })?;
    }
    objects.sort_by_key(|r| r.0);
    tiles.sort_by_key(|r| r.0);
    Ok(ReferenceIdLists {
        object_ids: encode_rows(objects, cancel)?,
        tile_ids: encode_rows(tiles, cancel)?,
    })
}

// AssetExtractor.addNode reads getFirstChild().getNodeValue(), not trimmed text
// content. Keep this small DOM adapter separate from the Catalog XML parser:
// comments, CDATA boundaries and nested markup must not change Catalog behavior.
#[derive(Default)]
struct Node {
    tag: String,
    attrs: BTreeMap<String, String>,
    text: Option<String>,
    first_seen: bool,
    text_open: bool,
    children: Vec<Node>,
}
impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
    fn first(&mut self, text: Option<&str>, plain: bool) {
        if !self.first_seen {
            self.text = text.map(str::to_owned);
        } else if self.text_open && plain {
            if let (Some(value), Some(part)) = (&mut self.text, text) {
                value.push_str(part);
            }
        }
        self.text_open = plain && text.is_some() && (!self.first_seen || self.text_open);
        self.first_seen = true;
    }
    // addNode starts with "" for no children. An Element first child instead
    // supplies a real Java null, even if later siblings contain text.
    fn scalar(&self) -> Option<&str> {
        if self.first_seen {
            self.text.as_deref()
        } else {
            Some("")
        }
    }
    fn descendants(&self) -> impl Iterator<Item = &Node> {
        let mut stack: Vec<_> = self.children.iter().rev().collect();
        std::iter::from_fn(move || {
            let n = stack.pop()?;
            stack.extend(n.children.iter().rev());
            Some(n)
        })
    }
}
fn records(
    bytes: &[u8],
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Node) -> Result<(), AssetError>,
) -> Result<(), AssetError> {
    let mut reader = Reader::from_reader(bytes);
    let mut stack: Vec<Node> = Vec::new();
    let (mut depth, mut roots, mut events, mut nodes, mut chars) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    loop {
        cancelled(cancel)?;
        events += 1;
        if events > 4_000_000 {
            return Err(AssetError::LimitExceeded);
        }
        match reader.read_event().map_err(|_| AssetError::InvalidXml)? {
            Event::Start(e) | Event::Empty(e) => {
                let empty =
                    bytes.get(reader.buffer_position().saturating_sub(2) as usize) == Some(&b'/');
                depth += 1;
                if depth == 1 {
                    roots += 1;
                }
                if depth > 64 {
                    return Err(AssetError::LimitExceeded);
                }
                if roots > 1 {
                    return Err(AssetError::InvalidXml);
                }
                let tag = e.name().as_ref().to_owned();
                if !stack.is_empty() || matches!(tag.as_str(), "Object" | "Ground") {
                    if let Some(parent) = stack.last_mut() {
                        parent.first(None, false);
                    } else {
                        nodes = 0;
                        chars = 0;
                    }
                    nodes += 1;
                    if nodes > 8192 {
                        return Err(AssetError::LimitExceeded);
                    }
                    let mut n = Node {
                        tag,
                        ..Node::default()
                    };
                    chars += n.tag.len();
                    for attr in e.attributes() {
                        let attr = attr.map_err(|_| AssetError::InvalidXml)?;
                        if n.attrs.len() >= 64 {
                            return Err(AssetError::LimitExceeded);
                        }
                        let key = attr.key.as_ref().to_owned();
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|_| AssetError::InvalidXml)?
                            .into_owned();
                        chars += key.len() + value.len();
                        n.attrs.insert(key, value);
                    }
                    stack.push(n);
                    if empty {
                        finish_node(&mut stack, emit)?;
                    }
                }
                if empty {
                    depth -= 1;
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or(AssetError::InvalidXml)?;
                if !stack.is_empty() {
                    finish_node(&mut stack, emit)?;
                }
            }
            Event::Text(e) => {
                let value = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                if let Some(n) = stack.last_mut() {
                    chars += value.len();
                    n.first(Some(&value), true);
                } else if depth == 0 && !value.trim().is_empty() {
                    return Err(AssetError::InvalidXml);
                }
            }
            Event::CData(e) => {
                if let Some(n) = stack.last_mut() {
                    let value = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                    chars += value.len();
                    n.first(Some(&value), false);
                }
            }
            Event::Comment(e) => {
                if let Some(n) = stack.last_mut() {
                    let value = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                    chars += value.len();
                    n.first(Some(&value), false);
                }
            }
            Event::PI(e) => {
                if let Some(n) = stack.last_mut() {
                    let value = e.content().trim_start();
                    chars += value.len();
                    n.first(Some(value), false);
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
                if let Some(n) = stack.last_mut() {
                    chars += value.len();
                    n.first(Some(&value), true);
                }
            }
            Event::DocType(_) => return Err(AssetError::InvalidXml),
            Event::Eof => break,
            _ => {}
        }
        if chars > 1024 * 1024 {
            return Err(AssetError::LimitExceeded);
        }
    }
    if depth != 0 || roots != 1 || !stack.is_empty() {
        return Err(AssetError::InvalidXml);
    }
    Ok(())
}
fn finish_node(
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

// Integer.decode accepts signed decimal, hexadecimal (0x / #) and octal.
fn java_integer(s: &str) -> Result<i32, AssetError> {
    let (negative, s) = if let Some(s) = s.strip_prefix('-') {
        (true, s)
    } else {
        (false, s.strip_prefix('+').unwrap_or(s))
    };
    let (radix, digits) = if let Some(s) = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .or_else(|| s.strip_prefix('#'))
    {
        (16, s)
    } else if s.starts_with('0') && s.len() > 1 {
        (8, &s[1..])
    } else {
        (10, s)
    };
    if digits.is_empty() || digits.starts_with(['-', '+']) {
        return Err(AssetError::InvalidCatalog);
    }
    let n = i64::from_str_radix(digits, radix).map_err(|_| AssetError::InvalidCatalog)?;
    i32::try_from(if negative { -n } else { n }).map_err(|_| AssetError::InvalidCatalog)
}

// Java assigns repeated scalar children in document order (last wins), without
// trimming. The bounded XML tree retains first-child values verbatim.
fn value<'a>(n: &'a Node, tag: &str) -> &'a str {
    n.children
        .iter()
        .rev()
        .find(|n| n.tag == tag)
        .map_or("", |n| n.scalar().unwrap_or("null"))
}

fn texture(n: &Node) -> Result<Option<String>, AssetError> {
    let mut index = -1;
    let mut file = None;
    for n in &n.children {
        match n.tag.as_str() {
            "Index" => {
                let value = n.scalar().ok_or(AssetError::InvalidCatalog)?;
                index = if value.starts_with("0x") {
                    java_integer(value)?
                } else {
                    value.parse().map_err(|_| AssetError::InvalidCatalog)?
                }
            }
            "File" => file = n.scalar(),
            _ => {}
        }
    }
    Ok(file
        .filter(|_| index != -1)
        .map(|file| format!("{index},{file}")))
}

fn object_row(n: &Node, id: i32) -> Result<String, AssetError> {
    let mut projectiles = Vec::new();
    let mut attacks = Vec::new();
    let mut textures = Vec::new();
    for child in &n.children {
        match child.tag.as_str() {
            "Subattack" => {
                if let Some(id) = child.attr("projectileId") {
                    attacks.push(
                        id.parse::<usize>()
                            .map_err(|_| AssetError::InvalidCatalog)?,
                    );
                }
            }
            "Projectile" => {
                let (mut min, mut max, mut pierce) = (None, None, false);
                for p in &child.children {
                    match p.tag.as_str() {
                        "Damage" => {
                            min = p.scalar();
                            max = p.scalar();
                        }
                        "MinDamage" => min = p.scalar(),
                        "MaxDamage" => max = p.scalar(),
                        "ArmorPiercing" => pierce = true,
                        _ => {}
                    }
                }
                if let (Some(min), Some(max)) = (min, max) {
                    projectiles.push(format!(
                        "{},{},{}",
                        min.replace('\t', ""),
                        max.replace('\t', ""),
                        u8::from(pierce)
                    ));
                }
            }
            "Texture" | "AnimatedTexture" => {
                if let Some(t) = texture(child)? {
                    textures.push(t);
                }
            }
            _ => {}
        }
    }
    let mut projectile_text = String::new();
    if !projectiles.is_empty() {
        let selected = if attacks.is_empty() {
            projectiles.iter().collect::<Vec<_>>()
        } else {
            attacks
                .iter()
                .map(|&i| projectiles.get(i).ok_or(AssetError::InvalidCatalog))
                .collect::<Result<Vec<_>, _>>()?
        };
        projectile_text.push_str(value(n, "SlotType"));
        for p in selected {
            projectile_text.push(',');
            projectile_text.push_str(p);
        }
    }
    Ok(format!(
        "{id};{};{};{};{projectile_text};{};{};{}",
        value(n, "DisplayId"),
        value(n, "Class"),
        value(n, "Group"),
        textures.join(","),
        value(n, "Labels"),
        n.attr("id").unwrap_or("")
    ))
}

fn tile_children(
    n: &Node,
    textures: &mut Vec<String>,
    damage: &mut String,
) -> Result<(), AssetError> {
    let (mut min, mut max) = (None, None);
    for child in &n.children {
        match child.tag.as_str() {
            "Texture" => {
                if let Some(t) = texture(child)? {
                    textures.push(t);
                }
            }
            "RandomTexture" => tile_children(child, textures, damage)?,
            // Deliberate Java switch fallthrough: MinDamage also sets max.
            "MinDamage" if child.first_seen => {
                min = child.scalar();
                max = child.scalar();
            }
            "MaxDamage" if child.first_seen => max = child.scalar(),
            _ => {}
        }
    }
    if let (Some(min), Some(max)) = (min, max) {
        *damage = format!("{min},{max}");
    }
    Ok(())
}
fn tile_row(n: &Node, id: i32) -> Result<String, AssetError> {
    let mut textures = Vec::new();
    let mut damage = String::new();
    tile_children(n, &mut textures, &mut damage)?;
    Ok(format!(
        "{id};{};{damage};{}",
        textures.join(","),
        n.attr("id").unwrap_or("")
    ))
}

fn encode_rows(rows: Vec<(i32, String)>, cancel: &AtomicBool) -> Result<Vec<u8>, AssetError> {
    let mut bytes = Vec::new();
    for (_, row) in rows {
        cancelled(cancel)?;
        for c in row.chars() {
            bytes.push(windows_1252(c));
        }
        bytes.push(b'\n');
    }
    Ok(bytes)
}
fn windows_1252(c: char) -> u8 {
    const EXTRA: &str = "€\u{fffd}‚ƒ„…†‡ˆ‰Š‹Œ\u{fffd}Ž\u{fffd}\u{fffd}‘’“”•–—˜™š›œ\u{fffd}žŸ";
    if (c as u32) < 128 || (160..=255).contains(&(c as u32)) {
        c as u8
    } else if c != '\u{fffd}' {
        EXTRA
            .chars()
            .position(|x| x == c)
            .map_or(b'?', |i| 128 + i as u8)
    } else {
        b'?'
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(name: &str, xml: &str) -> TextAsset {
        TextAsset {
            name: name.into(),
            bytes: xml.as_bytes().to_vec(),
        }
    }
    fn build(texts: &[TextAsset]) -> ReferenceIdLists {
        build_reference_id_lists(texts, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn rows_preserve_reference_fields_subattack_positions_and_damage_quirks() {
        let lists = build(&[text(
            "equip",
            r#"<Objects><Object type="0x10" id="Skull"><DisplayId> old </DisplayId><DisplayId>New &amp; Name</DisplayId><Class>Equipment</Class><Group>g</Group><SlotType>4</SlotType><Labels>UT, BOSS</Labels><Texture><Index>0x10</Index><File>a</File></Texture><AnimatedTexture><Index>08</Index><File>b</File></AnimatedTexture><Projectile id="9"><Damage>30</Damage><ArmorPiercing/></Projectile><Projectile id="0"><MinDamage>4</MinDamage><MaxDamage>6</MaxDamage></Projectile><Subattack projectileId="1"/><Subattack projectileId="0"/><Subattack projectileId="1"/></Object><Ground type="010" id="lava"><RandomTexture><Texture><Index>1</Index><File>tiles</File></Texture><RandomTexture><Texture><Index>2</Index><File>tiles</File></Texture></RandomTexture></RandomTexture><MaxDamage>9</MaxDamage><MinDamage>3</MinDamage></Ground></Objects>"#,
        )]);
        assert_eq!(
            lists.object_ids,
            b"16;New & Name;Equipment;g;4,4,6,0,30,30,1,4,6,0;16,a,8,b;UT, BOSS;Skull\n"
        );
        assert_eq!(lists.tile_ids, b"8;1,tiles,2,tiles;3,3;lava\n");
    }
    #[test]
    fn source_filename_order_suffix_collisions_and_duplicate_ids_are_preserved() {
        let lists = build(&[
            text(
                "z",
                "<Objects><Object type='2' id='z'/><Object type='-1' id='negative'/></Objects>",
            ),
            text("b2", "<Objects><Object type='2' id='literal'/></Objects>"),
            text(
                "b",
                "<Objects><Object type='2' id='first'/><Object type='2' id='again'/></Objects>",
            ),
            text("b", "<Objects><Object type='2' id='suffix3'/></Objects>"),
            text(
                "COPYING",
                "<Objects><Object type='2' id='excluded'/></Objects>",
            ),
            text("json", "{}"),
            text("a", "<Objects><Object type='811318' id='wide'/></Objects>"),
        ]);
        assert_eq!(lists.object_ids, b"-1;;;;;;;negative\n2;;;;;;;first\n2;;;;;;;again\n2;;;;;;;literal\n2;;;;;;;suffix3\n2;;;;;;;z\n811318;;;;;;;wide\n");
    }
    #[test]
    fn encoding_whitespace_and_numeric_rules_match_windows_oracle_profile() {
        let lists = build(&[text("a", "<Objects><Object type='#ff' id='ñ’中😀'><DisplayId> x </DisplayId><Projectile><Damage>\t8\t</Damage></Projectile></Object></Objects>")]);
        assert_eq!(lists.object_ids, b"255; x ;;;,8,8,0;;;\xf1\x92??\n");
        assert_eq!(java_integer("-0x80000000").unwrap(), i32::MIN);
        assert!(java_integer("2147483648").is_err());
        assert!(java_integer(" 1").is_err());
        assert!(java_integer("08").is_err());
    }
    #[test]
    fn java_first_child_values_and_incomplete_projectiles_are_preserved() {
        let lists = build(&[text("a", "<Objects><Object type='1'><DisplayId>a&amp;b<![CDATA[ignored]]>also ignored</DisplayId><Class><!--comment-->ignored</Class><Labels><Nested/>ignored</Labels><Group><![CDATA[first]]>ignored</Group><Projectile><MinDamage>2</MinDamage></Projectile><Projectile><Damage>3</Damage></Projectile><Subattack projectileId='0'/></Object><Ground type='2'><MinDamage><![CDATA[]]></MinDamage></Ground></Objects>")]);
        assert_eq!(lists.object_ids, b"1;a&b;comment;first;,3,3,0;;null;\n");
        assert_eq!(lists.tile_ids, b"2;;,;\n");
    }
    #[test]
    fn ntfs_upcase_order_controls_duplicate_object_and_tile_rows() {
        let texts = ["_x", "equip_lo", "`x", "^x", "]x", "[x", "a", "equipEggs"].map(|name| {
            text(
                name,
                &format!(
                    "<Root><Object type='1' id='{name}'/><Ground type='2' id='{name}'/></Root>"
                ),
            )
        });
        let lists = build(&texts);
        assert_eq!(lists.object_ids, b"1;;;;;;;a\n1;;;;;;;equipEggs\n1;;;;;;;equip_lo\n1;;;;;;;[x\n1;;;;;;;]x\n1;;;;;;;^x\n1;;;;;;;_x\n1;;;;;;;`x\n");
        assert_eq!(
            lists.tile_ids,
            b"2;;;a\n2;;;equipEggs\n2;;;equip_lo\n2;;;[x\n2;;;]x\n2;;;^x\n2;;;_x\n2;;;`x\n"
        );
    }
    #[test]
    fn element_first_child_is_null_for_optional_damage_and_texture_values() {
        let lists = build(&[text(
            "a",
            r#"<Root>
          <Object type="1" id="Nulls"><DisplayId><X/>ignored</DisplayId><Class><X/></Class><Group><X/></Group><Labels><X/></Labels>
            <Projectile><Damage><X/>ignored</Damage></Projectile>
            <Projectile><MinDamage><X/></MinDamage><MaxDamage>9</MaxDamage></Projectile>
            <Projectile><Damage>8</Damage><MaxDamage><X/></MaxDamage></Projectile>
            <Texture><Index>1</Index><File>old</File><File><X/>ignored</File></Texture>
          </Object>
          <Object type="2" id="Empty"><Projectile><Damage/></Projectile><Texture><Index>2</Index><File/></Texture></Object>
          <Ground type="3" id="NullMin"><MinDamage><X/>ignored</MinDamage><MaxDamage>9</MaxDamage></Ground>
          <Ground type="4" id="NullMax"><MinDamage>3</MinDamage><MaxDamage><X/></MaxDamage><Texture><Index>4</Index><File><X/></File></Texture></Ground>
          <Ground type="5" id="Empty"><MinDamage/></Ground>
          <Object type="6" id="Later"><Projectile><Damage><X/></Damage><Damage>7</Damage></Projectile><Texture><Index>6</Index><File><X/></File><File>later</File></Texture></Object>
        </Root>"#,
        )]);
        assert_eq!(
            lists.object_ids,
            b"1;null;null;null;;;null;Nulls\n2;;;;,,,0;2,;;Empty\n6;;;;,7,7,0;6,later;;Later\n"
        );
        assert_eq!(lists.tile_ids, b"3;;;NullMin\n4;;;NullMax\n5;;;Empty\n");
    }
    #[test]
    fn malformed_xml_bad_references_and_cancellation_fail_closed() {
        for xml in ["<!DOCTYPE Objects [<!ENTITY x 'y'>]><Objects/>", "<Objects>", "<Objects><Object><Projectile><Damage>1</Damage></Projectile><Subattack projectileId='1'/></Object></Objects>"] {
            assert!(build_reference_id_lists(&[text("a", xml)], &AtomicBool::new(false)).is_err());
        }
        assert!(matches!(
            build_reference_id_lists(&[], &AtomicBool::new(true)),
            Err(AssetError::Cancelled)
        ));
        assert!(build_reference_id_lists(
            &[text("a", "<Objects/>"), text("A", "<Objects/>")],
            &AtomicBool::new(false)
        )
        .is_err());
    }
}
