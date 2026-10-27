# Wodix

A read-only Rust library for parsing and querying MDict dictionary files: MDX articles and MDD resources. Wodix provides source-order record access, normalized headword search, article redirect resolution, and associated resource loading.

[crates.io](https://crates.io/crates/wodix) · [API documentation](https://docs.rs/wodix) · [Source repository](https://github.com/RigelNana/wodix) · [Crate guide](crates/wodix/README.md) · [Maintainer guide](crates/wodix/tests/README.md) · [Changelog](crates/wodix/CHANGELOG.md)

## Supported formats

- MDX/MDD version 1.x and 2.x layouts.
- Uncompressed, LZO, and zlib blocks; supported key-info encryption.
- UTF-8, UTF-16LE, GBK/GB18030, and Big5 text encoding.
- Exact, prefix, and Unicode-aware fuzzy headword queries.
- MDX redirects and stylesheet expansion; raw MDD resources and lazy associated-volume loading.

Version 3, unsupported encodings, and unsupported encryption modes return typed errors. Record blocks are decoded on demand, so successful construction does not guarantee that every later record read will succeed.

Wodix does not render HTML or execute dictionary scripts. Article markup and binary resources remain untrusted application data.

## Installation

Requires **Rust 1.87 or later** and a native C/C++ toolchain for dependencies.

Add Wodix to your application's `Cargo.toml`:

```toml
[dependencies]
wodix = "0.1.0"
```

All supported types and extension traits are imported from the crate root; internal format modules are not public API.

## Quickstart

This complete program reads a dictionary into owned memory and prints resolved articles and prefix matches:

```rust,no_run
use wodix::{Mdx, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("dictionary.mdx")?;
    let dictionary = Mdx::parse(bytes, "dictionary")?;

    println!("{} entries", dictionary.reader().entries().len());
    for article in dictionary.lookup("apple")? {
        println!("{article}");
    }
    for headword in dictionary.prefix("app", 10)? {
        println!("{headword}");
    }
    Ok(())
}
```

The second argument supplies a fallback title, not an encoding. `Mdx::parse` accepts owned or borrowed bytes, performs no filesystem I/O itself, and does not discover companion resources. `std::fs::read` loads the whole file: choose this approach only when the source fits your application's memory budget.

## Records and resources

Raw records, decoded text, and resolved articles have different contracts:

| API | Result |
| --- | --- |
| `Read::read(id)` | Immutable `Bytes` containing the raw decompressed record. |
| `Read::records()` | Source-order `(entry, bytes)` results, including duplicate headwords. |
| `Read::text(id)` | Strictly decoded text with trailing NUL and whitespace removed. |
| `Mdx::lookup(word)` | Resolved articles, with duplicate decoded text removed and MDX stylesheet markers expanded. |
| `Mdd::lookup(path)` | The first matching raw binary resource in one MDD, without redirect resolution. |
| `Resources::load(path)` | Associated local files first, then discovered MDD volumes with resource redirects resolved. |

Read records and an independent resource archive without memory mapping:

```rust,no_run
use wodix::{Mdd, Read, Reader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = Reader::parse(std::fs::read("dictionary.mdx")?, "dictionary")?;
    for record in reader.records() {
        let (entry, bytes) = record?;
        println!("{}: {} raw bytes", entry.headword, bytes.as_ref().len());
    }

    let resources = Mdd::parse(std::fs::read("dictionary.mdd")?, "resources")?;
    let image = resources.lookup("images/example.png")?;
    println!("{} resource bytes", image.as_ref().len());
    Ok(())
}
```

MDD metadata encoding describes resource keys, not arbitrary payloads. For a file-opened MDX, `dictionary.resources().load(path)` searches its directory and companion MDD volumes. Local files take precedence; parent traversal and symlinks escaping the dictionary directory are rejected. A byte-parsed MDX has an empty associated resource collection.

A missing MDX headword returns an empty article vector; resource lookup returns `Error::KeyNotFound`. Broken redirects, cycles, exhausted budgets, and malformed records are errors, not ordinary misses. Entry ordinals belong to one source and are not persistent identifiers. A record iterator advances after an error if the caller continues iterating.

`Bytes` may outlive its reader or cache, while entry references borrow their source. A small retained record can keep a larger shared decompressed block alive; explicitly copy a small slice when independent storage is preferable.

## Search semantics

The built-in index normalizes keys using NFC, lowercase, and trimming. This is not full Unicode case folding or linguistic stemming. `prefix` and `fuzzy` return `Result<Vec<&str>>`, limiting distinct original headwords rather than record count. Fuzzy distance is Levenshtein distance over normalized Unicode scalar values, not UTF-8 bytes or grapheme clusters; results follow normalized lexical order, not relevance ranking.

`Read` and `Lookup` are the supported static extension traits. A custom source must keep entries, ordinals, and record reads consistent. `Mdx::new` builds an index from that exact source rather than accepting an independently mutable reader/index pair.

## Limits, file safety, and caches

`Limits` defines component-local budgets for parsing, decompression, records, metadata, cache residency, and query work. Start with `Limits::default()` and change the required fields; zero is a real budget, not an unlimited setting. Both `Limits` and `Error` are non-exhaustive.

Defaults include a 16 MiB header limit; 64 MiB each for key indexes, compressed blocks, decompressed blocks, records, and per-reader cache residency; a 256 MiB metadata budget; 32 redirect edges; and 4,000,000 fuzzy dynamic-programming cells. These are **not a process RSS cap**. Source storage, retained `Bytes`, in-flight decoding, index storage, mapped pages, and allocator overhead also consume memory.

Memory-mapped `open` and `open_with` constructors are explicitly **unsafe**. The caller must ensure that mapped MDX/MDD files remain unmodified and untruncated for their required lifetimes. For associated resources, this includes volumes not yet opened lazily, and directory paths must remain stable. Advisory locks alone do not provide these guarantees. Prefer owned-byte parsing when immutability cannot be ensured.

File-opened `Mdx` and `Mdd` objects may create derived index caches and persistent lock files; resource loading can open further caches. Read-only dictionary access therefore does not mean zero disk writes. `Cache` provides explicit cache-location control. Its schema validates source-bound canonical keys and postings before rebuilding the FST; cache loading has real validation and reconstruction cost. The persistent headword index and the per-reader decompressed-block cache are separate mechanisms.

## Development

Run from the repository root:

```bash
cargo run -p wodix --example corpus --locked -- check-fixtures
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo bench -p wodix --bench parse -- --test
python tools/check_package.py
```

Fixtures and default Criterion inputs are generated locally without downloading dictionaries. Fetching Cargo dependencies may still require network access. Criterion's `--test` mode exercises benchmark paths; omit it for statistical measurements. The package checker requires Python 3.12 or later and verifies a local `.crate` archive in isolation without publishing it.

The [maintainer guide](crates/wodix/tests/README.md) documents fixture generation, all 17 benchmark cases, cold/warm definitions, private-corpus handling, linting, MSRV checks, and package verification. Historical measurements in `benchmarks/` describe their recorded inputs and environments, not a performance guarantee for the current library.

## License

See [LICENSE](LICENSE) for the MIT license and [NOTICE](crates/wodix/NOTICE) for attribution and provenance.
