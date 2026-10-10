fn u32le(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn align(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}
fn string(out: &mut Vec<u8>, s: &str) {
    u32le(out, s.len() as u32);
    out.extend(s.as_bytes());
    align(out);
}

pub fn fixture() -> Vec<u8> {
    fixture_with_stream(None)
}
pub fn fixture_with_stream(companion: Option<&str>) -> Vec<u8> {
    fixture_sized(companion, 2, 2)
}
pub fn fixture_sized(companion: Option<&str>, width: u32, height: u32) -> Vec<u8> {
    let mut text = Vec::new();
    string(&mut text, "equip");
    u32le(&mut text, 10);
    text.extend(b"<Objects/>");
    let mut texture = Vec::new();
    string(&mut texture, "characters");
    texture.extend([0; 4]);
    for n in [width, height, width * height * 4, 0, 4, 1, 0, 0, 0, 0, 1, 2] {
        u32le(&mut texture, n);
    }
    texture.extend([0; 32]);
    u32le(&mut texture, 0); // platform blob
    if let Some(path) = companion {
        u32le(&mut texture, 0);
        texture.extend(0u64.to_le_bytes());
        u32le(&mut texture, width * height * 4);
        string(&mut texture, path);
    } else {
        u32le(&mut texture, width * height * 4);
        let mut random = 0x9e3779b9u32;
        for i in 0..width * height {
            if width == 2 && height == 2 {
                texture.extend(match i % 4 {
                    0 => [255, 0, 0, 255],
                    1 => [0, 255, 0, 255],
                    2 => [0, 0, 255, 255],
                    _ => [255, 255, 255, 255],
                });
            } else {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                texture.extend(random.to_le_bytes());
            }
        }
        texture.extend([0; 16]); // stream offset, size, empty path
    }
    let mut mapping = Vec::new();
    string(&mut mapping, "spritesheetf");
    let sprite_bytes = sprite_mapping(true, if width > 256 && height >= 293 { 256 } else { 1 }); // large rectangle starts at (1, 37)
    u32le(&mut mapping, sprite_bytes.len() as u32);
    mapping.extend(sprite_bytes);
    let mut metadata = b"6000.0.58f2\0".to_vec();
    u32le(&mut metadata, 19);
    metadata.push(0);
    u32le(&mut metadata, 2);
    for class in [49, 28] {
        u32le(&mut metadata, class);
        metadata.extend([0, 255, 255]);
        metadata.extend([0; 16]);
    }
    u32le(&mut metadata, 3);
    for (i, offset, size, type_index) in [
        (0u64, 0u64, text.len(), 0),
        (1, text.len() as u64, texture.len(), 1),
        (2, (text.len() + texture.len()) as u64, mapping.len(), 0),
    ] {
        align(&mut metadata);
        metadata.extend((i + 1).to_le_bytes());
        metadata.extend(offset.to_le_bytes());
        u32le(&mut metadata, size as u32);
        u32le(&mut metadata, type_index);
    }
    metadata.extend([0; 13]); // scripts, externals, refs, user string
    let offset = 48 + metadata.len();
    let len = offset + text.len() + texture.len() + mapping.len();
    let mut out = vec![0; 48];
    out[8..12].copy_from_slice(&22u32.to_be_bytes());
    out[20..24].copy_from_slice(&(metadata.len() as u32).to_be_bytes());
    out[24..32].copy_from_slice(&(len as u64).to_be_bytes());
    out[32..40].copy_from_slice(&(offset as u64).to_be_bytes());
    out.extend(metadata);
    out.extend(text);
    out.extend(texture);
    out.extend(mapping);
    out
}

fn word(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
}
fn table(b: &mut Vec<u8>, widths: &[usize]) -> (usize, Vec<usize>) {
    let v = b.len();
    b.resize(v + 4 + widths.len() * 2, 0);
    while !b.len().is_multiple_of(8) {
        b.push(0);
    }
    let t = b.len();
    let mut fields = vec![];
    let mut offset = 4;
    for (i, w) in widths.iter().enumerate() {
        if *w == 0 {
            fields.push(0);
            continue;
        }
        b[v + 4 + i * 2..v + 6 + i * 2].copy_from_slice(&(offset as u16).to_le_bytes());
        fields.push(t + offset);
        offset += w;
    }
    b[v..v + 2].copy_from_slice(&((4 + widths.len() * 2) as u16).to_le_bytes());
    b[v + 2..v + 4].copy_from_slice(&(offset as u16).to_le_bytes());
    b.resize(t + offset, 0);
    word(b, t, (t - v) as u32);
    (t, fields)
}
fn link(b: &mut [u8], from: usize, to: usize) {
    word(b, from, (to - from) as u32);
}
fn text(b: &mut Vec<u8>, s: &str) -> usize {
    let p = b.len();
    b.extend((s.len() as u32).to_le_bytes());
    b.extend(s.as_bytes());
    b.push(0);
    p
}
fn sprite_mapping(animated: bool, side: u32) -> Vec<u8> {
    let mut b = vec![0; 4];
    let (root, fields) = table(&mut b, &[4, 4]);
    link(&mut b, 0, root);
    let v = b.len();
    b.extend([1, 0, 0, 0, 0, 0, 0, 0]);
    link(&mut b, fields[0], v);
    let (sheet, s) = table(&mut b, &[4, 0, 4]);
    link(&mut b, v + 4, sheet);
    let name = text(&mut b, "demo");
    link(&mut b, s[0], name);
    let v = b.len();
    b.extend([1, 0, 0, 0, 0, 0, 0, 0]);
    link(&mut b, s[2], v);
    let (sprite, f) = table(&mut b, &[16, 0, 0, 4, 0, 0, 0, 8]);
    link(&mut b, v + 4, sprite);
    word(&mut b, f[0] + 8, 2f32.to_bits());
    word(&mut b, f[0] + 12, 2f32.to_bits());
    word(&mut b, f[3], 7);
    word(&mut b, f[7], 2);
    if animated {
        let v = b.len();
        b.extend([1, 0, 0, 0, 0, 0, 0, 0]);
        link(&mut b, fields[1], v);
        let (a, f) = table(&mut b, &[4, 4, 0, 0, 0, 4]);
        link(&mut b, v + 4, a);
        let name = text(&mut b, "demo");
        link(&mut b, f[0], name);
        word(&mut b, f[1], 7);
        let (s, g) = table(&mut b, &[16, 0, 0, 4, 0, 0, 0, 8]);
        link(&mut b, f[5], s);
        word(&mut b, g[0], 1f32.to_bits());
        word(
            &mut b,
            g[0] + 4,
            (if side == 256 { 37f32 } else { 0f32 }).to_bits(),
        );
        word(&mut b, g[0] + 8, (side as f32).to_bits());
        word(&mut b, g[0] + 12, (side as f32).to_bits());
        word(&mut b, g[7], 2);
    } else {
        // An absent optional vector is represented by an absent vtable field.
        let v = root - u32::from_le_bytes(b[root..root + 4].try_into().unwrap()) as usize;
        b[v + 6..v + 8].copy_from_slice(&0u16.to_le_bytes());
    }
    b
}
