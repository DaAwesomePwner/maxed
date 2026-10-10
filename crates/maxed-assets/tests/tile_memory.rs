use maxed_assets::{
    load_or_extract,
    sprite_delivery::{read_sprite, TileKey},
    AssetPaths, AssetProgress,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Instant,
};
#[allow(dead_code)]
mod common;

// This integration-test binary contains one test. Track live requested heap bytes,
// including png's internal allocations, without instrumenting production code.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn added(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            added(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(ptr, layout, size) };
        if !next.is_null() {
            if size >= layout.size() {
                added(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn measure<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let result = work();
    (
        result,
        PEAK.load(Ordering::Relaxed).saturating_sub(baseline),
    )
}

#[test]
fn extraction_and_serving_never_allocate_an_atlas_and_cached_extraction_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AssetPaths {
        resources: dir.path().join("resources.assets"),
        app_data: dir.path().join("app"),
    };
    // 32 MiB synthetic source, created before measuring the actual pipeline.
    std::fs::write(&paths.resources, common::fixture_sized(None, 4096, 2048)).unwrap();
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    let (cache, extraction_peak) =
        measure(|| load_or_extract(&paths, &cancel, &mut |_| {}).unwrap());
    let elapsed = start.elapsed();
    assert!(
        extraction_peak < 8 * 1024 * 1024,
        "extraction peak {extraction_peak}"
    );
    let sprite = &cache.catalog.sprites[0];
    assert_eq!(sprite.y, 37);
    let tile = TileKey::from_sprite(sprite).unwrap();
    let (bytes, serving_peak) = measure(|| read_sprite(&cache, tile).unwrap());
    assert!(bytes.len() <= 256 * 256 * 4 + 256 + 1024);
    assert!(serving_peak < 384 * 1024, "serving peak {serving_peak}");
    // Independent comparison with the same rectangle in the streamed atlas PNG.
    let mut tile_reader = png::Decoder::new(std::io::Cursor::new(&bytes))
        .read_info()
        .unwrap();
    let mut tile_pixels = vec![0; tile_reader.output_buffer_size().unwrap()];
    tile_reader.next_frame(&mut tile_pixels).unwrap();
    let mut atlas = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(cache.root.join("characters.png")).unwrap(),
    ))
    .read_info()
    .unwrap();
    for y in 0..sprite.y + sprite.height {
        let row = atlas.next_row().unwrap().unwrap();
        if y >= sprite.y {
            let at = ((y - sprite.y) * sprite.width * 4) as usize;
            let x = sprite.x as usize * 4;
            let len = sprite.width as usize * 4;
            assert_eq!(&tile_pixels[at..at + len], &row.data()[x..x + len]);
        }
    }
    drop(atlas);
    // Delivery has no dependency on a PNG atlas, nor on client files.
    std::fs::rename(
        cache.root.join("characters.png"),
        cache.root.join("not-served.png"),
    )
    .unwrap();
    assert_eq!(read_sprite(&cache, tile).unwrap(), bytes);
    std::fs::rename(
        cache.root.join("not-served.png"),
        cache.root.join("characters.png"),
    )
    .unwrap();
    let pack = cache.root.join("tiles.pack");
    let before = std::fs::read(&pack).unwrap();
    let modified = std::fs::metadata(&pack).unwrap().modified().unwrap();
    let mut stages = Vec::new();
    let warm = Instant::now();
    let second = load_or_extract(&paths, &cancel, &mut |phase| stages.push(phase)).unwrap();
    let warm_elapsed = warm.elapsed();
    assert!(!stages.contains(&AssetProgress::Sprites));
    assert_eq!(cache.root, second.root);
    assert_eq!(before, std::fs::read(&pack).unwrap());
    assert_eq!(
        modified,
        std::fs::metadata(pack).unwrap().modified().unwrap()
    );
    println!("32 MiB synthetic atlas: cold {elapsed:?}, warm {warm_elapsed:?}, extraction peak {extraction_peak} bytes, serving peak {serving_peak} bytes, response {} bytes", bytes.len());
}
