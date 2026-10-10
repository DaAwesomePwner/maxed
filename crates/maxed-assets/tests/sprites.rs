use maxed_assets::{parse_sprites, SheetInfo};
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
fn fixture(animated: bool) -> Vec<u8> {
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
        word(&mut b, g[0] + 8, 1f32.to_bits());
        word(&mut b, g[0] + 12, 1f32.to_bits());
        word(&mut b, g[7], 2);
    } else {
        // An absent optional vector is represented by an absent vtable field.
        let v = root - u32::from_le_bytes(b[root..root + 4].try_into().unwrap()) as usize;
        b[v + 6..v + 8].copy_from_slice(&0u16.to_le_bytes());
    }
    b
}
fn sheets() -> Vec<SheetInfo> {
    vec![SheetInfo {
        name: "characters".into(),
        width: 2,
        height: 2,
    }]
}
#[test]
fn static_rectangles_and_source_ordered_animation_override() {
    let a = parse_sprites(&fixture(false), &sheets()).unwrap();
    assert_eq!(a[0].key, "demo:7");
    assert_eq!(a[0].sheet, "characters.png");
    assert_eq!(a[0].width, 2);
    let b = parse_sprites(&fixture(true), &sheets()).unwrap();
    assert_eq!(b.len(), 1);
    assert_eq!((b[0].x, b[0].width), (1, 1));
}
#[test]
fn truncation_offsets_and_sheet_bounds_are_checked() {
    let bytes = fixture(false);
    for end in 1..bytes.len() {
        assert!(
            parse_sprites(&bytes[..end], &sheets()).is_err(),
            "length {end}"
        );
    }
    let mut bytes = bytes;
    word(&mut bytes, 0, u32::MAX);
    assert!(parse_sprites(&bytes, &sheets()).is_err());
    assert!(parse_sprites(&fixture(false), &[]).is_err());
    let mut info = sheets();
    info[0].width = 1;
    assert!(parse_sprites(&fixture(false), &info).is_err());
}
