# Acceptance

These requirements are verified against real files and rendered UI, not source-text or mock-forwarding tests.

## Engine

- [ ] MDX source-order duplicate senses and @@@LINK chains retain provenance; missing/cyclic redirects report errors.
- [ ] Explicit arbitrary MDD mounts, gaps and independent bad volumes; local CSS/JS/font/image/audio resources; namespaced resolution and conflict precedence.
- [ ] Exact metadata fields and raw header attributes, key/record block statistics; missing dates/versions remain null.

## Service

- [ ] Batch upload and permitted host-directory import preserve trees, auto-associate companions and report ambiguity.
- [ ] Dictionary/resource CRUD and inspection, independent group membership/order and multi-field sorts persist across restart.
- [ ] Joint suggestions filter group before limit, do not decompress records and discard stale responses in UI.
- [ ] Reader residency, decode concurrency, request sizes and caches have bounded configuration.
- [ ] Application/appearance/security/spelling/history settings; diagnostics; index/cache clear/rebuild; validated backup/restore.
- [ ] Real indexed full-text search, explicit index state and reading presentation mode.
- [ ] CLI query/manage operations use same service, JSON stdout, stderr errors, proper nonzero failures and scoped destructive commands.
- [ ] Remote connection uses the same API and reports unavailable/unauthorized state.

## UI

- [ ] Home has large search with group switch plus 查词/设置; focus history; IME-safe suggestions; bottom docking; keyboard navigation.
- [ ] Word top bar, collapsible icon/title dictionary sections, no article cards, same visual ground, single outer scroll.
- [ ] Internal entry/anchor/audio links and back restore reading context.
- [ ] Settings categories, dictionary detail two tabs, resource tree preview/binding, directory import and group multi-sort.
- [ ] Desktop/mobile, light/dark, reduced motion, focus and error/empty/loading states inspected in browser.

## Delivery

- [ ] Production build, actual CLI/service smoke and real browser flows completed.
- [ ] Documentation has exact setup/commands/security boundaries and exercised evidence; no unimplemented UI controls or mock data presented as working features.
