# HTTP contract

Base `/api/v1`. JSON uses snake_case. Success returns the documented object, errors `{error:{code,message}}` with an appropriate non-2xx status. API auth is `Authorization: Bearer <token>` when configured. The public content route uses only per-dictionary, expiring scoped capabilities, never API tokens. All paths below are relative to base.

## Models

`Dictionary`: `{id,name,revision,created:string,enabled,icon:string|null,entries:number,metadata:Metadata,resources:Resource[],trust_scripts:boolean,custom_css:string,font_family:string,typography_mode:'original'|'global'|'override',warnings:string[],index_status:{state:'missing'|'building'|'ready'|'error',indexed:number,total:number,error:string|null}}`. `created` is the RFC3339 import timestamp, not the dictionary's embedded creation date.

`Metadata`: `{title,description,format_version,encoding,encrypted:number,key_blocks:number,record_blocks:number,attributes:Record<string,string>,created:string|null,dictionary_version:string|null}`.

`Resource`: `{id,path,kind:'mdd'|'css'|'js'|'font'|'image'|'audio'|'file',size:number,enabled:boolean,order:number,key:string}`. `path` is a relative managed path, never an unrestricted filesystem read parameter. `key` is its virtual resource path and may be edited to fix associations.

`Group`: `{id,name,dictionary_ids:string[],sort:SortRule[]}`. `SortRule`: `{field:'name'|'entries'|'created',direction:'asc'|'desc'}`; empty rules means dictionary_ids is the manual order. Rule ties use manual order. Names compare lowercased Unicode strings; `created` sorts the import timestamp.

`Settings`: `{application:{default_group:string,history_enabled:boolean,history_limit:number},appearance:{theme:'system'|'light'|'dark',font_size:number,line_height:number,font_family:string,content_width:number,motion:boolean,reading_mode:boolean},spelling:{enabled:boolean,distance:number},security:{external_links:'ask'|'allow'|'block'},performance:{max_open_readers:number,cache_mb:number,query_workers:number}}`. `cache_mb` is the global reader-cache allocation pool, divided across reader slots; retained views/in-flight work/mapped pages are not included in this budget.

`HistoryItem`: `{id,word,group_id,at}`. `LogItem`: `{id,at,level,scope,message}`.

`Bootstrap`: `{dictionaries:Dictionary[],groups:Group[],settings:Settings,history:HistoryItem[],stats:{words:number,open_readers:number,cache_bytes:number,jobs:Job[]}}`. `Job`: `{id,kind,dictionary_id:string|null,state:'queued'|'running'|'done'|'error',progress:number,total:number,error:string|null}`.

## Reading

- GET `/health` -> `{ok:true,version}`.
- GET `/bootstrap` -> Bootstrap.
- GET `/suggest?q=&group=all&limit=20&mode=headword|fulltext` -> `{items:[{word,dictionary_ids:string[],match_kind:'exact'|'prefix'|'fuzzy'|'fulltext',preview:string|null}],truncated:boolean,elapsed_ms:number}`. Group filtering happens BEFORE limit. Disabled dictionaries never contribute.
- GET `/lookup?q=&dictionary=<id>` -> `{dictionary_id,word,articles:[{html:string,source_ids:number[],resolved_ids:number[],redirects:string[]}],elapsed_ms:number}`. Each html is a complete sandbox-ready document with controlled height/link/style bridge. API credentials never enter html. Reader CSS updates use parent postMessage.
- POST `/history` `{word,group_id}` -> HistoryItem. DELETE `/history` clears; DELETE `/history/<id>` removes one.

Bridge events from frame: `{type:'fastdict:height',height:number}`, `{type:'fastdict:navigate',word:string,anchor?:string}`, `{type:'fastdict:external',url:string}`, `{type:'fastdict:anchor',offset:number}`. Parent validates event.source, type and bounded values. Parent -> frame `{type:'fastdict:style',theme:'light'|'dark',font_size,line_height,font_family,reading_mode}` or `{type:'fastdict:anchor',anchor:string}`. Frame grants no management operations.

## Imports and dictionaries

- POST `/imports` multipart fields `files`, filenames carry relative directory paths -> ImportPreview.
- POST `/imports/path` `{path}` (only configured import roots) -> ImportPreview.
- `ImportPreview`: `{id,files:[{path,size,kind}],candidates:[{mdx,name,resources:string[],warnings:string[]}],warnings:string[]}`.
- POST `/imports/<id>/commit` `{dictionaries:[{mdx,name,resources:string[]}]}` -> `{dictionaries:Dictionary[]}`. User edits associations before commit; service owns immutable copies.
- PUT `/dictionaries/<id>` accepts `{name?,enabled?,icon?,trust_scripts?,custom_css?,font_family?,typography_mode?,resources?:Resource[]}` -> Dictionary.
- DELETE `/dictionaries/<id>` unregisters and removes derived records, without modifying live mapped files; physical revisions may remain until no readers reference them.
- POST `/dictionaries/<id>/resources` multipart -> Dictionary (append mounts into a new immutable revision).
- GET `/dictionaries/<id>/resources/<rid>/entries?prefix=&limit=100` -> `{items:[{path,size}],truncated}` (for MDD) or the one file.
- GET `/dictionaries/<id>/resources/<rid>/content?path=` -> authenticated resource bytes with safe MIME; UI previews through a blob, never executes HTML/JS.

## Configuration

- POST `/groups` `{name,dictionary_ids?,sort?}` -> Group. PUT `/groups/<id>` `{name?,dictionary_ids?,sort?}` -> Group. DELETE `/groups/<id>` removes group only.
- PUT `/settings` Settings -> Settings.
- GET `/logs?limit=200` -> `{items:LogItem[]}`. DELETE `/logs` clears.
- POST `/maintenance` `{target:'headwords'|'fulltext'|'cache',action:'clear'|'rebuild',dictionary_id?:string}` -> `{job_id:string|null}`. Real full-text index state is reflected by bootstrap. No maintenance action deletes dictionary originals.
- GET `/backup` -> downloadable versioned backup; optional `include_files=true` includes managed dictionary sources.
- POST `/restore` multipart field `file` -> `{restored:true}`. Validate archive paths, size, schema and referenced revisions. Restored dictionary scripts are disabled until explicitly authorized.

## Engine integration (server/src/engine.rs)

Independent synchronous module, called through service's bounded blocking worker pool. Public serializable structs can be re-used by service models.

- `ResourceMount { path:PathBuf, key:String }`, explicit ordered mounts (MDD and loose files).
- `Engine::open(mdx_path:&Path, root:&Path, mounts:&[ResourceMount], cache_bytes:usize) -> anyhow::Result<Engine>`; root and mapped files are managed immutable revisions.
- `Engine::metadata() -> EngineMetadata` matching Metadata above.
- `Engine::entries() -> &[wodix::Entry]`.
- `Engine::lookup(word:&str) -> anyhow::Result<Vec<EngineArticle>>`; EngineArticle `{html,source_ids,resolved_ids,redirects}` raw style-expanded HTML. Service renderer turns it into sandbox document.
- `Engine::record_text(id:usize) -> anyhow::Result<String>` for background full-text indexing only.
- `Engine::resource(path:&str) -> anyhow::Result<wodix::Bytes>` explicit mounts, normalized safe root paths, raw binary, redirect limits. Unrelated bad MDD does not block a located valid resource.
- `Engine::resource_entries(mount_path:&Path,prefix:&str,limit:usize) -> anyhow::Result<Vec<(String,u64)>>`.
- `Engine::cache_bytes() -> usize` current resident decompressed bytes, not RSS.
