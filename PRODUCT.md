# fastdict

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Vite, React, TypeScript, Tailwind CSS, shadcn/ui (Radix), Motion for React. Rust HTTP service based on wodix. The user's `vp` is interpreted as Vite, as proposed and accepted for implementation.

## Users

Chinese-speaking users consulting multiple locally owned MDX dictionaries from a desktop or a phone. Agents consuming the same dictionary service using a non-interactive CLI.

## Product Purpose

A configurable, quiet dictionary service, not an ebook library or a general-purpose reader. Dictionaries remain on the service host; a phone does not need copies. Local and remote connections share the UI and API.

## Product Principles

1. Search first: a large search input and persistent 查词 / 设置 workspace navigation.
2. One continuous reading surface: dictionary headings and text, not stacked cards.
3. Correct sources before animation: redirects, duplicate articles, resource volumes, provenance, and back navigation must work with real files.
4. Pay only for requested work: suggestions never decompress article bodies; collapsed dictionaries do not render content.
5. Mature accessible components for controls; no custom reinvention of dialogs, menus, tabs, or keyboard list navigation.

## Capabilities and Constraints

- Search focus moves the whole search field and caret together to the bottom dock, with history and suggestions above it. Selecting an entry returns the same input to the top and shows the article. Query groups use the same quiet dropdown menu as the search mode. Settings save automatically without a separate save action.
- Service status and reconnection live in one shared header entry. Settings categories retain loaded data during connection failures; connection fields live only under connection and security.
- The query group is selectable next to search. Dictionary sections independently expand/collapse, with group-defined ordering and icons.
- Internal entry links prefer the originating dictionary. Article redirect resolution is not a new navigation. Back restores query, group, expansion and scroll.
- Top-level settings: application, appearance, dictionaries, security, spelling, backup/restore, logs, indexes/cache.
- Dictionary settings have management and group management. Keep the dictionary list beside its details; configuration and information are tabs within the same panel.
- Bind multiple arbitrarily named MDDs, CSS, JS and fonts, with ordering, enabled state and resource inspection. Support manual icons and automatic discovery.
- Bulk import directories, preserve relative paths, identify companion assets, report ambiguity and missing volumes, and allow correcting bindings.
- Show metadata honestly: format version versus dictionary version, key/record blocks, entry count, encoding, creation date. Missing fields remain missing.
- Dictionaries can belong to multiple groups. Groups have independent manual order and stable multi-field ascending/descending rules.
- @@@LINK resolves with cycle/depth/error semantics. Preserve distinct articles and detailed source records; do not deduplicate across dictionaries.
- Arbitrary dictionary scripts are untrusted. Default safe mode disables them. Compatibility mode requires explicit per-dictionary authorization and an isolated renderer without application credentials or native privileges.
- Full-text search is an explicit alternate search mode backed by a derived index. Reading mode is a presentation mode for the same entries.
- CLI uses the same API, structured stdout, stderr diagnostics, scoped mutations, and non-interactive operation.
- Desktop packaging is a future host of this frontend/service, not a second engine. No native IPC is granted to dictionary content.

## Operating Context

User-owned dictionaries; no commercial dictionary is bundled or downloaded. Imported files become managed immutable revisions before memory mapping. Remote uploads and host-directory imports are explicit different sources, not hidden filesystem assumptions.

## Evidence on Hand

wodix 0.1.0 at upstream revision 2f5c0e30655c7c8690799b610fc820fccf4a2375. Research exercised basic MDX/MDD, aliases, Unicode, duplicate records, redirect cycles/missing targets, MDD numbering gaps, and whole-volume initialization failure. There is no evidence yet for commercial-corpus compatibility or service-level latency/RSS claims.

## Accessibility & Inclusion

Keyboard-operable controls, visible focus, IME-safe search, reduced-motion support, at least 44px primary touch targets, responsive mobile settings, no nested article scrollbars, and honest empty/loading/error/partial-failure states.
