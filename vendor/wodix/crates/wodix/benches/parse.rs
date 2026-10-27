//! Offline by default. WODIX_FIXTURE overrides the input strictly;
//! WODIX_BENCH_ENTRIES selects the Rust corpus size (default 10000, max 100000).
//! The Rust corpus differs from tools/generate_benchmark.py's historical corpus.
//! Case IDs include the content fingerprint and entry count. All query and cache
//! preconditions are checked before timing; unsuitable external inputs fail.

#[path = "../tests/support/mod.rs"]
mod support;

use std::hint::black_box;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use support::Corpus;
use wodix::{Cache, Index, Limits, Lookup, Mdx, Read, Reader};

struct Source {
    bytes: Vec<u8>,
    identity: String,
}

impl Source {
    fn load() -> Self {
        let (bytes, label) = if let Some(path) = std::env::var_os("WODIX_FIXTURE") {
            let path = PathBuf::from(path);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("WODIX_FIXTURE {}: {error}", path.display()));
            eprintln!("External corpus: {}", path.display());
            (bytes, "external")
        } else {
            let count = match std::env::var("WODIX_BENCH_ENTRIES") {
                Ok(value) => value
                    .parse()
                    .expect("WODIX_BENCH_ENTRIES must be an integer"),
                Err(std::env::VarError::NotPresent) => Corpus::DEFAULT_ENTRIES,
                Err(error) => panic!("WODIX_BENCH_ENTRIES: {error}"),
            };
            let corpus = Corpus::benchmark(count).expect("invalid benchmark corpus size");
            (
                corpus.serialize().expect("generate offline corpus"),
                "rust-xorshift64-v1",
            )
        };
        let digest = blake3::hash(&bytes).to_hex();
        eprintln!("Corpus {label}: {} bytes, BLAKE3 {digest}", bytes.len());
        Self {
            identity: format!("{label}-{}", &digest[..16]),
            bytes,
        }
    }
}

struct Queries {
    hit: String,
    miss: String,
    prefix: String,
    fuzzy: String,
    prefix_count: usize,
    fuzzy_count: usize,
}

impl Queries {
    const LIMIT: usize = 20;

    fn prepare<R: Read, I: Lookup>(dictionary: &Mdx<R, I>) -> Self {
        let hit = dictionary
            .reader()
            .entries()
            .iter()
            .take(1024)
            .map(|entry| entry.headword.trim())
            .find(|word| {
                !word.is_empty()
                    && dictionary
                        .lookup(word)
                        .is_ok_and(|articles| !articles.is_empty())
            })
            .expect("fixture needs a resolvable article among its first 1024 entries")
            .to_owned();
        assert!(!dictionary.index().exact(&hit).is_empty());
        let mut miss = "mdict benchmark deliberately absent".to_owned();
        while !dictionary.index().exact(&miss).is_empty() {
            miss.push('~');
        }
        assert!(dictionary.lookup(&miss).unwrap().is_empty());

        let mut selected_prefix = None;
        for entry in dictionary.reader().entries().iter().take(1024) {
            let word = entry.headword.trim();
            for (end, _) in word.char_indices().rev() {
                if end == 0 {
                    continue;
                }
                let prefix = &word[..end];
                let count = dictionary.prefix(prefix, Self::LIMIT).unwrap().len();
                if count >= 2 {
                    selected_prefix = Some((prefix.to_owned(), count));
                    break;
                }
            }
            if selected_prefix.is_some() {
                break;
            }
        }
        let (prefix, prefix_count) = selected_prefix.expect(
            "fixture needs a nonempty prefix with at least two distinct candidates; no fake prefix case is registered",
        );
        let fuzzy = ['~', '!', '#', '界']
            .into_iter()
            .map(|suffix| format!("{hit}{suffix}"))
            .find(|word| dictionary.index().exact(word).is_empty())
            .expect("fixture needs an absent one-edit variant for the fuzzy case");
        let candidates = dictionary.fuzzy(&fuzzy, 1, Self::LIMIT).unwrap();
        assert!(
            candidates
                .iter()
                .any(|word| { dictionary.index().exact(word) == dictionary.index().exact(&hit) }),
            "the fuzzy case must recover the selected hit, not merely return unrelated candidates",
        );
        let fuzzy_count = candidates.len();
        eprintln!(
            "Queries: hit={hit:?}, miss={miss:?}, prefix={prefix:?} ({prefix_count} results, limit {}), fuzzy={fuzzy:?} (distance 1, {fuzzy_count} results, limit {})",
            Self::LIMIT,
            Self::LIMIT,
        );
        Self {
            hit,
            miss,
            prefix,
            fuzzy,
            prefix_count,
            fuzzy_count,
        }
    }
}

struct RecordCase<'a> {
    warm: Reader<&'a [u8]>,
    cold: Reader<&'a [u8]>,
    id: usize,
    bytes: u64,
}

impl<'a> RecordCase<'a> {
    fn prepare(bytes: &'a [u8]) -> Self {
        let warm = Reader::parse(bytes, "record benchmark").unwrap();
        let mut selected = None;
        for id in 0..warm.entries().len().min(1024) {
            let record = warm.read(id).unwrap();
            if record.as_ref().is_empty() {
                continue;
            }
            let before = warm.cache_stats();
            let repeated = warm.read(id).unwrap();
            let after = warm.cache_stats();
            if after.misses == before.misses && after.hits == before.hits + 1 {
                assert_eq!(record.as_ref(), repeated.as_ref());
                selected = Some((id, record.as_ref().len() as u64));
                break;
            }
        }
        let (id, record_bytes) = selected.expect(
            "fixture needs a nonempty, cacheable single-block record among its first 1024 entries",
        );
        let mut limits = Limits::default();
        limits.cache = 0;
        let cold = Reader::with_limits(bytes, "record benchmark", limits).unwrap();
        let expected = cold.read(id).unwrap();
        let before = cold.cache_stats();
        assert_eq!(cold.read(id).unwrap().as_ref(), expected.as_ref());
        let after = cold.cache_stats();
        assert_eq!(after.hits, 0);
        assert_eq!(after.resident, 0);
        assert_eq!(after.misses, before.misses + 1);
        eprintln!(
            "Single-block record: id {id}, {record_bytes} decoded bytes; cold cache=0, warm cache={}",
            warm.limits().cache
        );
        Self {
            warm,
            cold,
            id,
            bytes: record_bytes,
        }
    }
}

/// Owns every file the benchmark creates; no user cache is read or deleted.
struct Storage {
    _directory: tempfile::TempDir,
    source: PathBuf,
    cache: PathBuf,
}

impl Storage {
    fn prepare(bytes: &[u8], dictionary: &Mdx<Reader<&[u8]>>, hit: &str) -> Self {
        let directory = tempfile::tempdir().expect("create isolated benchmark directory");
        let source = directory.path().join("source.mdx");
        let cache_path = source.with_extension("mdict.idx");
        let mut file = std::fs::File::create(&source).unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let cache = Cache::new(
            &cache_path,
            dictionary.reader().entries(),
            dictionary.reader().limits(),
        );
        cache
            .save(dictionary.index())
            .expect("synchronously publish benchmark cache");
        let loaded = cache.load().unwrap().expect("saved cache must be present");
        assert_eq!(loaded.exact(hit), dictionary.index().exact(hit));
        // SAFETY: This directory is exclusively owned here. The source is never
        // modified again, and all mapped readers are dropped before Storage.
        let opened = unsafe { Mdx::open(&source) }.unwrap();
        assert_eq!(opened.index().exact(hit), dictionary.index().exact(hit));
        Self {
            _directory: directory,
            source,
            cache: cache_path,
        }
    }
}

struct Bench;

impl Bench {
    const WARM_CACHE_BYTES: usize = 256 * 1024 * 1024;

    fn scan<R: Read>(reader: &R) -> u64 {
        let mut total = 0u64;
        for record in reader.records() {
            let (_, bytes) = record.unwrap();
            total += black_box(bytes.as_ref()).len() as u64;
        }
        total
    }

    fn run(criterion: &mut Criterion) {
        let source = Source::load();
        let bytes = source.bytes.as_slice();
        let dictionary = Mdx::parse(bytes, "benchmark").expect("parse and index benchmark input");
        let queries = Queries::prepare(&dictionary);
        let records = RecordCase::prepare(bytes);
        let storage = Storage::prepare(bytes, &dictionary, &queries.hit);
        let count = dictionary.reader().entries().len() as u64;
        let identity = format!("{}-n{count}", source.identity);

        let before = dictionary.reader().cache_stats();
        assert!(!dictionary.lookup(&queries.hit).unwrap().is_empty());
        assert_eq!(
            dictionary.reader().cache_stats().misses,
            before.misses,
            "lookup_hit_warm must not decode an uncached block",
        );
        let mut limits = Limits::default();
        limits.cache = Self::WARM_CACHE_BYTES;
        let sequential = Reader::with_limits(bytes, "sequential benchmark", limits).unwrap();
        let total_bytes = Self::scan(&sequential);
        assert!(
            total_bytes > 0 && total_bytes <= Self::WARM_CACHE_BYTES as u64,
            "warm sequential corpus must fit the explicit 256 MiB cache budget"
        );
        let before = sequential.cache_stats();
        assert_eq!(Self::scan(&sequential), total_bytes);
        assert_eq!(
            sequential.cache_stats().misses,
            before.misses,
            "warm sequential must retain every block, including any record-stream padding"
        );
        let fresh = Reader::parse(bytes, "first sequential setup check").unwrap();
        assert_eq!(fresh.cache_stats().misses, 0);
        assert_eq!(fresh.cache_stats().blocks, 0);
        assert_eq!(Self::scan(&fresh), total_bytes);
        assert!(fresh.cache_stats().misses > 0);
        eprintln!(
            "Corpus {identity}: {count} entries, {total_bytes} decoded record bytes; first sequential cache={}, warm sequential cache={}",
            fresh.limits().cache,
            sequential.limits().cache
        );
        drop(fresh);

        let mut parsing = criterion.benchmark_group(format!("parse/{identity}"));
        parsing.throughput(Throughput::Bytes(bytes.len() as u64));
        parsing.bench_function("reader_parse_borrowed", |bench| {
            bench.iter(|| Reader::parse(black_box(bytes), "benchmark").unwrap());
        });
        parsing.bench_function("parse_and_build_index", |bench| {
            bench.iter(|| Mdx::parse(black_box(bytes), "benchmark").unwrap());
        });
        parsing.bench_function("open_mmap_and_build_index", |bench| {
            bench.iter(|| {
                // SAFETY: Storage exclusively owns this immutable source file.
                let reader = unsafe { Reader::open(black_box(&storage.source)) }.unwrap();
                let dictionary: Mdx<_> = Mdx::new(reader).unwrap();
                black_box(dictionary)
            });
        });
        parsing.bench_function("open_mmap_and_validate_cached_index", |bench| {
            bench.iter(|| {
                // SAFETY: Storage outlives the returned mapping and never changes its source.
                unsafe { Mdx::open(black_box(&storage.source)) }.unwrap()
            });
        });
        parsing.finish();

        let reader = dictionary.reader();
        let cache = Cache::new(&storage.cache, reader.entries(), reader.limits());
        let mut indexing = criterion.benchmark_group(format!("index/{identity}"));
        indexing.throughput(Throughput::Elements(count));
        indexing.bench_function("build", |bench| {
            bench.iter(|| Index::build(black_box(reader.entries()), reader.limits()).unwrap());
        });
        indexing.bench_function("cache_validate_load_and_rebuild_fst", |bench| {
            bench.iter(|| cache.load().unwrap().expect("benchmark cache disappeared"));
        });
        indexing.bench_function("cache_save_atomic_and_sync", |bench| {
            bench.iter(|| cache.save(black_box(dictionary.index())).unwrap());
        });
        indexing.finish();

        let mut searching = criterion.benchmark_group(format!("query/{identity}"));
        searching.throughput(Throughput::Elements(1));
        searching.bench_function("exact_hit", |bench| {
            bench.iter(|| dictionary.index().exact(black_box(&queries.hit)));
        });
        searching.bench_function("exact_miss", |bench| {
            bench.iter(|| dictionary.index().exact(black_box(&queries.miss)));
        });
        searching.bench_function("lookup_hit_warm_blocks", |bench| {
            bench.iter(|| dictionary.lookup(black_box(&queries.hit)).unwrap());
        });
        searching.bench_function("lookup_miss", |bench| {
            bench.iter(|| dictionary.lookup(black_box(&queries.miss)).unwrap());
        });
        searching.bench_function(
            format!("prefix_limit20_results{}", queries.prefix_count),
            |bench| {
                bench.iter(|| {
                    dictionary
                        .prefix(black_box(&queries.prefix), Queries::LIMIT)
                        .unwrap()
                });
            },
        );
        searching.bench_function(
            format!("fuzzy_edit1_limit20_results{}", queries.fuzzy_count),
            |bench| {
                bench.iter(|| {
                    dictionary
                        .fuzzy(black_box(&queries.fuzzy), 1, Queries::LIMIT)
                        .unwrap()
                });
            },
        );
        searching.finish();

        let mut access = criterion.benchmark_group(format!("records/{identity}"));
        access.throughput(Throughput::Bytes(records.bytes));
        access.bench_function("single_block_cold_cache0", |bench| {
            bench.iter(|| records.cold.read(black_box(records.id)).unwrap());
        });
        access.bench_function("single_block_warm", |bench| {
            bench.iter(|| records.warm.read(black_box(records.id)).unwrap());
        });
        access.throughput(Throughput::Bytes(total_bytes));
        access.bench_function("sequential_first_pass_fresh_reader", |bench| {
            // Parsing is untimed. Each timed pass owns a fresh, never-read block
            // cache; iter_batched_ref also keeps reader destruction out of timing.
            bench.iter_batched_ref(
                || Reader::parse(bytes, "benchmark").unwrap(),
                |reader| Self::scan(reader),
                BatchSize::PerIteration,
            );
        });
        access.bench_function("sequential_warm_all_blocks", |bench| {
            bench.iter(|| Self::scan(&sequential));
        });
        access.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).measurement_time(Duration::from_secs(5));
    targets = Bench::run
}
criterion_main!(benches);
