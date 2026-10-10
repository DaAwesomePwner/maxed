//! Standalone attribution probe. This never starts Maxed or opens user history/settings.
use maxed_types::Catalog;
use std::{
    fs::File,
    io::{BufReader, Write},
    time::Duration,
};

fn phase(name: &str) {
    println!("phase={name}");
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(4));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("Temporary catalog copy required")?,
    );
    if !path
        .canonicalize()?
        .starts_with(std::env::temp_dir().canonicalize()?)
    {
        return Err("Only a temporary game-catalog copy is allowed".into());
    }
    phase("baseline");
    let mut catalog: Catalog = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    macro_rules! counts {
        ($($field:ident),+) => { $(println!("family={} len={} capacity={} element_bytes={}",
            stringify!($field), catalog.$field.len(), catalog.$field.capacity(),
            std::mem::size_of_val(catalog.$field.first().ok_or("Empty family")?));)+ };
    }
    counts!(items, objects, classes, enchants, dungeons, bags, sprites, tiles);
    phase("catalog_retained");
    catalog.sprites.shrink_to_fit();
    catalog.items.shrink_to_fit();
    catalog.objects.shrink_to_fit();
    catalog.classes.shrink_to_fit();
    catalog.enchants.shrink_to_fit();
    catalog.dungeons.shrink_to_fit();
    catalog.bags.shrink_to_fit();
    catalog.tiles.shrink_to_fit();
    catalog.crucible_bonuses.shrink_to_fit();
    phase("top_level_vectors_compacted");
    std::hint::black_box(&catalog);
    drop(catalog);
    phase("catalog_dropped");
    Ok(())
}
