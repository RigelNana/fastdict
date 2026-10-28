# Architecture

## Decisions

- One Rust binary exposes HTTP service and CLI subcommands. Browser and CLI use `/api/v1`; no duplicate query engine.
- Vite React app is served by the service in production, proxied during development. Remote endpoint and access token are device-local connection settings, never passed into article frames.
- wodix is vendored at upstream 2f5c0e30655c7c8690799b610fc820fccf4a2375, preserving MIT/NOTICE. Changes are limited to detailed article results, complete metadata/block information, explicit resource mounting and bounded resource behavior.
- SQLite stores configuration, imported revisions, resource bindings, groups, history, diagnostics and derived full-text data. Raw MDX/MDD remain files. Derived search indexes are disposable and separate from source files.
- Import copies files into a revision directory owned by the service. A mapped revision is never modified or truncated. Directory import is restricted to explicitly configured roots. Browser upload preserves relative paths and rejects traversal/symlinks.
- A shared immutable sorted word catalog maps NFC/lowercase/trim-normalized headwords to dictionary postings. Binary prefix lookup does not read payloads; group membership is applied before suggestion limits. The source engine retains record IDs. The service uses this sorted catalog rather than adding a second FST dependency; fuzzy fallback has explicit candidate/work limits and reports truncation.
- A bounded worker pool executes synchronous wodix work outside async request threads. A committed Arc configuration snapshot avoids SQLite JSON parsing on the query hot path. Per-revision single-flight coalesces cold opens. The total reader-cache budget is divided across bounded reader slots; retained Bytes, evicted but active readers, in-flight work and mapped pages mean this is not a strict RSS cap.
- Per-dictionary lookup requests permit independent errors and rendering without waiting for all dictionaries. Results include source and redirect provenance.
- Explicit MDD mounts and resource manifests avoid relying on consecutive filename discovery. Resource reads must not initialize unrelated volumes.
- Safe HTML is rewritten to scoped asset/entry URLs. Each expanded dictionary renders inside its own sandboxed opaque-origin iframe with a controlled bridge, restrictive CSP, transparent background and measured height. Default mode strips dictionary scripts/event handlers; compatibility mode is explicit. The bridge is not an authority for management actions.
- Local asset capability URLs are scoped to dictionary/revision and expiration. They never convey an API/admin credential. CSP blocks fetch/XHR/WebSocket/workers/remote subresources/forms; the iframe blocks popups/top navigation/native APIs. Compatibility scripts cannot be portably prevented from navigating their own iframe externally across browsers, so compatibility is an explicit trust decision, not a guarantee of arbitrary-script network confinement.
- Full-text uses a separate SQLite FTS5 derived index, Latin tokenization and CJK character tokenization/phrase search. It runs in an independent serial background pool with bounded write batches and cancellation checks between records. Availability is visible; short Chinese queries use the index, not an unbounded body scan.
- Backup restores configuration/bindings/groups/history with schema and path validation. Dictionary binaries are retained by managed revision; importing foreign backups must not activate arbitrary host paths or script permissions.
- Upload/archive limits: 2 GiB aggregate sources per operation, 20,000 files, 2 MiB JSON requests, 16 MiB backup manifest, 64 MiB served assets, 32 MiB authenticated previews. Uploads stream to staging; ZIP backup spools to a temporary file and streams; restore bounds expanded bytes and validates every member.
- Browser dictionary/resource uploads above 16 MiB use authenticated 8 MiB chunks. A disk-backed manifest records declared paths/sizes; writes enforce fixed block boundaries, verify duplicate deliveries, and repair partial blocks after interruption. Only complete files enter staging. Publication replies are persisted and retries do not add resources twice; chunked resource revisions retain indexes. At most eight unfinished upload sessions are admitted, with abandoned receiving data reclaimed after 24 hours on the next session creation. The client retries transient chunk/commit failures twice and reports aggregate progress. Small multipart imports and the CLI remain compatible.
- Non-loopback binds require a token of at least 16 characters. FASTDICT_PUBLIC_ORIGIN controls the canonical reverse-proxy origin/Host; FASTDICT_ALLOWED_ORIGINS allows exact admin origins, never wildcard credentialed access. Managed data is not modified in place; host import sources should be quiescent during copying.

## UI state

Home -> focused search/history -> typed suggestions -> entry. Search input stays mounted across states. Entry navigation snapshots contain query, group, preferred dictionary, expanded IDs and scroll position. Settings use dedicated routes and return to the prior reading state.

## Verification

Real authored MDX/MDD smoke corpus, not mocked HTTP responses. Cover duplicate senses, aliases/cycles, Unicode, arbitrary/gapped volumes, local resources, CSS/JS/font isolation, group filtering/order, persistence/restart, upload/path traversal, CLI parity, backup/restore, full-text index rebuild and mobile/desktop interaction. Measure warm/cold separately; do not reuse upstream historical benchmark numbers as product promises.

## Desktop boundary

Tauri/Electron are future packaging targets. The implemented HTTP boundary supports either host. Packaging is not claimed until an actual application package is built and exercised.
