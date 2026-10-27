//! Time parsing, index construction and cache loading separately.
//! The cache is temporary; this command does not remove the user's cache.

use std::path::Path;
use std::time::Instant;

use wodix::{Cache, Index, Lookup, Read, Reader};

struct Bench;

impl Bench {
    fn run(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = std::fs::read(path)?;
        let started = Instant::now();
        let reader = Reader::parse(bytes, "dictionary")?;
        println!(
            "Parse: {:?}; {} entries",
            started.elapsed(),
            reader.entries().len()
        );
        let started = Instant::now();
        let index = Index::build(reader.entries(), reader.limits())?;
        println!("Build FST: {:?}", started.elapsed());

        let directory = tempfile::tempdir()?;
        let cache = Cache::new(
            directory.path().join("dictionary.idx"),
            reader.entries(),
            reader.limits(),
        );
        cache.save(&index)?;
        let started = Instant::now();
        let loaded = cache.load()?.ok_or("cache could not be loaded")?;
        println!("Validate cache and rebuild FST: {:?}", started.elapsed());
        for entry in reader.entries() {
            if loaded.exact(&entry.headword) != index.exact(&entry.headword) {
                return Err(format!("cache differs for {:?}", entry.headword).into());
            }
        }
        println!("Cache matches every headword and duplicate posting.");
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Usage: bench_cache <dictionary.mdx>")?;
    Bench::run(Path::new(&path))?;
    Ok(())
}
