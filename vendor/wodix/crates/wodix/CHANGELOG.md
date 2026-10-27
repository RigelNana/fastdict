# Changelog

## 0.1.0 - 2026-09-07

### Initial API

- Rename the project, package and Rust namespace to `wodix`; move the crate to `crates/wodix`. Update imports and Cargo commands without compatibility aliases.
- Rename benchmark environment variables to `WODIX_FIXTURE` and `WODIX_BENCH_ENTRIES`; historical reports and on-disk format identifiers remain unchanged.

- Supported APIs are exported from the crate root. Internal format modules and low-level decoder paths are no longer public; migrate imports rather than depending on compatibility aliases.
- `Cache`, `Entry`, `Encryption` and `StyleSheet` are named root exports for public signatures and advanced callers.
- `Limits` and `Error` are non-exhaustive. Construct Limits from Default then set fields; retain a fallback when matching Error.
- Debug output is a bounded summary, not a way to inspect complete dictionary payloads.

### Fixes

- Publish index caches through the standard library's atomic rename so Windows readers can keep the previous snapshot open while a writer replaces it. Retain synchronization and temporary-file cleanup on failure.
- Compare resource-error paths by file identity, accounting for Windows short paths and canonicalized temporary directories on macOS.
- Cover open-handle replacement and failed-publication cleanup with deterministic regressions; retain subprocess output when a cache writer fails.

### Documentation and package

- Add crate-level examples, executable fixture-backed rustdoc and a read-only compile-fail example.
- Document raw records, decoded text, resolved articles, source-local ordinals, component limits and file/cache side effects.
- Include README, MIT license text and source provenance notice in the crates.io package.
- Set the repository to `https://github.com/RigelNana/wodix` and rewrite the README documentation in English.
- Publish the initial `wodix` release as version 0.1.0, with registry installation instructions and docs.rs API documentation.

### Validation and benchmarks

- Remove the now-unreachable text-chain helper and redundant unused locator size fields exposed by the facade cutover; active lookup and record-reading algorithms are unchanged.
- Add deterministic offline fixture generation and verification commands, public consumer tests, and packaged fixture assets.
- Make default Criterion workloads self-contained, validate query preconditions, and distinguish cold/warm blocks and sequential passes.
- Retain historical corpus generation as a standalone standard-library Python tool rather than only as embedded report text.
- Add cross-platform, MSRV, strict documentation, release benchmark smoke and standalone package verification workflows.

### Scope

This change does not redesign parsing, cache policy, concurrency, on-disk index layout or the desktop application. Historical benchmark reports remain immutable observations for their recorded code and corpus.
