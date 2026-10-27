# Wodix

A read-only Rust library for MDX dictionaries and MDD resource archives. Wodix exposes immutable record views, normalized headword queries, MDX redirect and stylesheet resolution, and associated resource loading.

[crates.io](https://crates.io/crates/wodix) · [API documentation](https://docs.rs/wodix) · [Source repository](https://github.com/RigelNana/wodix)

## Format support

MDX/MDD 1.x and 2.x layouts are supported, including uncompressed, LZO, and zlib blocks, supported key-info encryption, and UTF-8, UTF-16LE, GBK/GB18030, and Big5 text. Version 3 and unsupported encoding or encryption modes return typed errors rather than being interpreted as supported formats.

Construction parses keys and directories; record payloads are decoded lazily and may report errors when first accessed. The library does not render HTML, execute dictionary scripts, or interpret arbitrary resource payloads as text.

## Installation

Requires **Rust 1.87 or later** and a native C/C++ toolchain for dependencies. Add Wodix to your application's `Cargo.toml`:

```toml
[dependencies]
wodix = "0.1.0"
```

Public types and extension traits are exported from `wodix`; internal format modules are not part of the API.

## Look up articles

```rust,no_run
use wodix::{Mdx, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("dictionary.mdx")?;
    let dictionary = Mdx::parse(bytes, "dictionary")?;

    println!("{} entries", dictionary.reader().entries().len());
    for article in dictionary.lookup("apple")? {
        println!("{article}");
    }
    for headword in dictionary.fuzzy("aple", 1, 10)? {
        println!("{headword}");
    }
    Ok(())
}
```

`Mdx::parse` accepts owned or borrowed bytes and uses the second argument as a fallback title. It performs no filesystem I/O or resource discovery. Reading into a `Vec<u8>` avoids memory-mapped file-lifetime requirements, but loads the entire input into memory.

The built-in index applies NFC, lowercase, and trimming, not full Unicode case folding or stemming. `prefix(word, limit)` and `fuzzy(word, distance, limit)` return distinct original headwords in normalized lexical order. Fuzzy distance counts Unicode scalar values, not bytes or grapheme clusters, and results are not ranked by distance.

## Raw records, decoded text, and resources

| Operation | Contract |
| --- | --- |
| `Read::read(id)` | Raw decompressed record as immutable `Bytes`. |
| `Read::records()` | Source-order iteration, preserving duplicate headwords and advancing after errors. |
| `Read::text(id)` | Strict metadata-based decoding, with trailing NUL and whitespace removed. |
| `Mdx::lookup(word)` | Redirect resolution, decoded-article deduplication, then MDX stylesheet expansion. |
| `Mdd::lookup(path)` | First matching raw resource in one archive; no text decoding or redirect resolution. |
| `Resources::load(path)` | Local-file precedence, associated MDD volumes, and MDD resource redirects. |

Configure a reader and inspect its records, then load an independent MDD resource:

```rust,no_run
use wodix::{Limits, Mdd, Read, Reader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut limits = Limits::default();
    limits.cache = 8 * 1024 * 1024;
    let reader = Reader::with_limits(
        std::fs::read("dictionary.mdx")?,
        "dictionary",
        limits,
    )?;
    for record in reader.records() {
        let (entry, bytes) = record?;
        println!("{}: {} raw bytes", entry.headword, bytes.as_ref().len());
    }
    if !reader.entries().is_empty() {
        println!("{}", reader.text(0)?);
    }

    let resources = Mdd::parse(std::fs::read("dictionary.mdd")?, "resources")?;
    let image = resources.lookup("images/example.png")?;
    println!("{} resource bytes", image.as_ref().len());
    Ok(())
}
```

MDD header encoding applies to keys, not binary payloads. Resource lookup normalizes separators and key case. `Mdd::lookup` does not access the filesystem. For an MDX opened from a file, `dictionary.resources().load(path)` first checks local files under the dictionary directory, then lazily opens companion MDD volumes. Parent traversal and symlinks escaping the directory are rejected. A byte-parsed MDX has an empty associated resource collection.

A missing article returns an empty vector; a missing resource returns `Error::KeyNotFound`. Broken redirect targets, cycles, traversal limits, and corrupt data are distinct errors. High-level article and headword queries do not return partial vectors on failure.

Entry ordinals identify records only within their source. Entry references borrow that source, while returned `Bytes` can outlive the reader and its cache. Retaining a small view may retain an entire shared decompressed block; copy it explicitly when independent storage is needed.

## Safety and budgets

`Limits` and `Error` are non-exhaustive. Configure limits by modifying `Limits::default()`, and include a fallback arm when matching errors. `Mdx::with_limits` applies custom budgets directly; `Mdd::new` can wrap a `Reader::with_limits` reader.

Default byte budgets are 16 MiB for the header; 64 MiB each for the key index, compressed block, decompressed block, record, and per-reader block cache; and 256 MiB for component-local metadata and working storage. Defaults also bound entries to 10,000,000, directory blocks to 1,000,000, redirect depth to 32 edges, and fuzzy work to 4,000,000 dynamic-programming cells. Zero is a real budget. These limits are not an aggregate RSS cap: source bytes, outstanding record views, concurrent decoding, indexes, mapped pages, and allocator overhead consume additional memory.

Memory-mapped `Reader`, `Mdx`, and `Mdd` constructors (`open` and `open_with`) are **unsafe**. Backing files must remain unmodified and untruncated for their mapped lifetimes. `Mdx` resource discovery additionally requires stable directory paths and immutable companion volumes, including before lazy opening. Advisory locks do not establish these guarantees. Use owned-byte parsing when they cannot be ensured.

`Mdx::open` and `Mdd::open` may write neighboring index caches and persistent lock files. Loading associated resources can open additional volume caches. Read-only source access does not imply zero disk writes. `Reader` alone neither writes an index cache nor discovers resources. Article HTML and resource bytes remain untrusted and need application-level handling before display or execution.

## Index caching and extensions

`Cache` allows an explicit persistent index-cache location. Cache identity includes original headwords, source order, record offsets, and normalization/schema rules rather than relying on coarse modification times. Schema 3 validates canonical keys and postings against the source before rebuilding the FST; it does not traverse untrusted serialized FST nodes. Writers coordinate with file locks and publish through same-directory temporary writes, synchronization, and atomic replacement. Validation and reconstruction have a cost; cache loading is not guaranteed to beat a fresh index build.

The persistent headword cache is separate from the per-reader decompressed-block cache. The latter's residency statistics exclude views retained by callers after eviction.

`Read` and `Lookup` are the supported static extension boundaries. Custom readers must keep their entries, ordinals, and records consistent. `Mdx<R, I>::new` builds its index from the supplied reader and preserves that pairing. `Lookup::visit` returns `Result<()>`; callbacks may receive some groups before an error, so collected results are complete only after successful return.

## Tests and benchmarks

From this crate directory, including an extracted standalone package:

```bash
cargo run --example corpus --locked -- check-fixtures
cargo test --all-targets --locked
cargo test --doc --locked
cargo bench --bench parse -- --test
```

The committed small fixtures and default Criterion corpus are generated locally without downloading dictionaries. Initial Cargo dependency resolution may need network access. `WODIX_BENCH_ENTRIES` selects the generated corpus size; `WODIX_FIXTURE` selects an external input and fails explicitly if it cannot satisfy the benchmark scenarios. Criterion's `--test` mode checks execution paths; omit it for statistical measurement.

See [tests/README.md](tests/README.md) for fixture maintenance, all 17 benchmark cases, cold/warm definitions, measurement units, and package verification. Crate-level rustdoc includes executable fixture-based examples and compile-fail examples for the read-only API.

## License

See [LICENSE](LICENSE) for the MIT license and [NOTICE](NOTICE) for attribution and provenance.
