use crate::{
    engine::{Engine, ResourceMount},
    imports::{self, Candidate, ImportPreview},
    model::*,
    render::{self, RenderOptions},
    search::{self, Catalog},
    store::Store,
};
use anyhow::{Context, bail};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as AxPath, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct ServeConfig {
    pub listen: SocketAddr,
    pub data_dir: PathBuf,
    pub token: Option<String>,
    pub import_roots: Vec<PathBuf>,
    pub static_dir: PathBuf,
}
struct Capability {
    dictionary: String,
    revision: String,
    bindings: blake3::Hash,
    origin: String,
    expires: Instant,
    base: String,
}
struct App {
    config: ServeConfig,
    store: Store,
    catalog: RwLock<Arc<Catalog>>,
    catalog_publication: Mutex<()>,
    readers: Mutex<VecDeque<(String, Arc<Engine>)>>,
    reader_generation: AtomicU64,
    open_gates: Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>,
    workers: RwLock<Arc<Semaphore>>,
    background: Arc<Semaphore>,
    jobs: Mutex<Vec<Job>>,
    caps: Mutex<HashMap<String, Capability>>,
    mutation: tokio::sync::Mutex<()>,
}
type Shared = Arc<App>;
#[derive(Debug)]
pub struct ApiError(StatusCode, &'static str, String);
impl ApiError {
    fn bad(m: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request", m.into())
    }
    fn missing() -> Self {
        Self(
            StatusCode::NOT_FOUND,
            "not_found",
            "未找到请求的资源".into(),
        )
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let (status, code) = match e.downcast_ref::<std::io::Error>().map(std::io::Error::kind) {
            Some(std::io::ErrorKind::PermissionDenied) => (StatusCode::FORBIDDEN, "forbidden"),
            Some(std::io::ErrorKind::NotFound) => (StatusCode::NOT_FOUND, "not_found"),
            _ => (StatusCode::UNPROCESSABLE_ENTITY, "operation_failed"),
        };
        Self(status, code, e.to_string())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"error":{"code":self.1,"message":self.2}})),
        )
            .into_response()
    }
}
type Result<T> = std::result::Result<T, ApiError>;
fn valid_id(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}
fn bounded_word(s: &str) -> Result<()> {
    if s.trim().is_empty() || s.len() > 512 || s.chars().any(char::is_control) {
        return Err(ApiError::bad("查询必须为 1–512 字节且不含控制字符"));
    }
    Ok(())
}
fn number(v: &Value, key: &str, default: usize) -> usize {
    v[key].as_u64().unwrap_or(default as u64) as usize
}
impl App {
    fn dictionary(&self, id: &str) -> Result<Dictionary> {
        self.store
            .snapshot()
            .dictionaries
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(ApiError::missing)
    }
    fn root(&self, d: &Dictionary) -> PathBuf {
        self.config.data_dir.join("revisions").join(&d.revision)
    }
    fn cached_engine(&self, key: &str) -> Option<Arc<Engine>> {
        let mut readers = self.readers.lock().unwrap();
        let index = readers.iter().position(|(k, _)| k == key)?;
        let entry = readers.remove(index).unwrap();
        let engine = entry.1.clone();
        readers.push_back(entry);
        Some(engine)
    }
    fn engine(&self, d: &Dictionary) -> anyhow::Result<Arc<Engine>> {
        let key = format!("{}:{}:{}", d.id, d.revision, resource_fingerprint(d));
        if let Some(engine) = self.cached_engine(&key) {
            return Ok(engine);
        }
        let gate = {
            let mut gates = self.open_gates.lock().unwrap();
            gates.retain(|_, gate| gate.strong_count() > 0);
            if let Some(gate) = gates.get(&key).and_then(std::sync::Weak::upgrade) {
                gate
            } else {
                let gate = Arc::new(Mutex::new(()));
                gates.insert(key.clone(), Arc::downgrade(&gate));
                gate
            }
        };
        let _opening = gate.lock().unwrap();
        if let Some(engine) = self.cached_engine(&key) {
            return Ok(engine);
        }
        let generation = self.reader_generation.load(Ordering::Acquire);
        let snapshot = self.store.snapshot();
        let settings = &snapshot.settings;
        let max = number(&settings["performance"], "max_open_readers", 8);
        let cache = number(&settings["performance"], "cache_mb", 128) * 1024 * 1024 / max;
        let root = self.root(d);
        let mut resources = d.resources.iter().filter(|r| r.enabled).collect::<Vec<_>>();
        resources.sort_by_key(|r| r.order);
        let mounts = resources
            .into_iter()
            .map(|r| ResourceMount {
                path: root.join(&r.path),
                key: r.key.clone(),
            })
            .collect::<Vec<_>>();
        let engine = Arc::new(Engine::open(&root.join(&d.mdx), &root, &mounts, cache)?);
        let mut readers = self.readers.lock().unwrap();
        if self.reader_generation.load(Ordering::Acquire) != generation {
            return Ok(engine);
        }
        while readers.len() >= max {
            readers.pop_front();
        }
        readers.push_back((key, engine.clone()));
        Ok(engine)
    }
    fn refresh_catalog(&self) -> anyhow::Result<()> {
        let _publication = self.catalog_publication.lock().unwrap();
        let catalog = Catalog::new(self.store.words()?);
        *self.catalog.write().unwrap() = Arc::new(catalog);
        Ok(())
    }
    fn evict(&self, id: Option<&str>) {
        let mut readers = self.readers.lock().unwrap();
        self.reader_generation.fetch_add(1, Ordering::Release);
        readers.retain(|(key, _)| id.is_some_and(|id| !key.starts_with(&format!("{id}:"))));
    }
}

fn resource_fingerprint(dictionary: &Dictionary) -> blake3::Hash {
    let mut hash = blake3::Hasher::new();
    hash.update(dictionary.mdx.as_bytes());
    for resource in &dictionary.resources {
        for value in [&resource.path, &resource.key, &resource.kind] {
            hash.update(&(value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
        hash.update(&[u8::from(resource.enabled)]);
        hash.update(&(resource.order as u64).to_le_bytes());
    }
    hash.finalize()
}
async fn blocking<T: Send + 'static>(
    state: Shared,
    f: impl FnOnce(Shared) -> anyhow::Result<T> + Send + 'static,
) -> Result<T> {
    let pool = state.workers.read().unwrap().clone();
    let permit = pool
        .acquire_owned()
        .await
        .map_err(|_| ApiError::bad("Service shutting down"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        f(state)
    })
    .await
    .map_err(|e| ApiError::bad(e.to_string()))?
    .map_err(Into::into)
}
fn origin_allowed(app: &App, origin: &str, host: &str) -> bool {
    origin == format!("http://{host}")
        || std::env::var("FASTDICT_PUBLIC_ORIGIN").ok().as_deref() == Some(origin)
        || std::env::var("FASTDICT_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .any(|o| !o.is_empty() && o == origin)
        || (app.config.listen.ip().is_loopback()
            && matches!(origin, "http://localhost:5173" | "http://127.0.0.1:5173"))
}
fn host_allowed(app: &App, host: &str) -> bool {
    let Ok(url) = url::Url::parse(&format!("http://{host}")) else {
        return false;
    };
    let Some(name) = url.host_str() else {
        return false;
    };
    if name == "localhost"
        || name
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| {
                ip.is_loopback()
                    || ip == app.config.listen.ip()
                    || (!app.config.listen.ip().is_loopback()
                        && ip != std::net::IpAddr::from([0, 0, 0, 0]))
            })
    {
        return true;
    }
    std::env::var("FASTDICT_PUBLIC_ORIGIN")
        .ok()
        .and_then(|s| url::Url::parse(&s).ok())
        .is_some_and(|u| u.host_str() == Some(name))
}
async fn guard(State(app): State<Shared>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !host_allowed(&app, &host) {
        return ApiError(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "Host not allowed".into(),
        )
        .into_response();
    }
    let asset = request.uri().path().starts_with("/api/v1/content/");
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let api = request.uri().path().starts_with("/api/v1/");
    if api && !asset {
        if let Some(origin) = &origin {
            if !origin_allowed(&app, origin, &host) {
                return ApiError(
                    StatusCode::FORBIDDEN,
                    "invalid_origin",
                    "Origin not allowed".into(),
                )
                .into_response();
            }
        }
        if request.method() == axum::http::Method::OPTIONS {
            let mut response = StatusCode::NO_CONTENT.into_response();
            cors(&mut response, origin.as_deref());
            return response;
        }
        if let Some(token) = &app.config.token {
            let expected = format!("Bearer {token}");
            if request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                != Some(expected.as_str())
            {
                return {
                    let mut response = ApiError(
                        StatusCode::UNAUTHORIZED,
                        "unauthorized",
                        "需要有效的 API 访问令牌".into(),
                    )
                    .into_response();
                    cors(&mut response, origin.as_deref());
                    response
                };
            }
        }
    }
    let request = if api
        && request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|mime| mime == "application/json" || mime.ends_with("+json"))
            }) {
        let (parts, body) = request.into_parts();
        match axum::body::to_bytes(body, 2 * 1024 * 1024).await {
            Ok(bytes) => Request::from_parts(parts, Body::from(bytes)),
            Err(_) => {
                return ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "request_limit",
                    "JSON request exceeds 2 MiB".into(),
                )
                .into_response();
            }
        }
    } else {
        request
    };
    if request.uri().to_string().len() > 8192 {
        return ApiError::bad("Request URI too long").into_response();
    }
    let mut response = next.run(request).await;
    if api
        && response.status().is_client_error()
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|mime| mime == "application/json" || mime.ends_with("+json"))
            })
    {
        let status = response.status();
        response = ApiError(
            status,
            "invalid_request",
            status
                .canonical_reason()
                .unwrap_or("Invalid request")
                .into(),
        )
        .into_response();
    }
    if api && !asset {
        cors(&mut response, origin.as_deref());
    }
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
fn cors(response: &mut Response, origin: Option<&str>) {
    if let Some(origin) = origin {
        if let Ok(value) = HeaderValue::from_str(origin) {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
            response
                .headers_mut()
                .insert(header::VARY, HeaderValue::from_static("Origin"));
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("Authorization, Content-Type"),
            );
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
            );
        }
    }
}
pub async fn serve(mut config: ServeConfig) -> anyhow::Result<()> {
    if !config.listen.ip().is_loopback() && config.token.as_ref().is_none_or(|t| t.len() < 16) {
        bail!("Non-loopback listen requires a token of at least 16 characters");
    }
    if config.token.as_ref().is_some_and(|t| t.is_empty()) {
        bail!("Token must not be empty");
    }
    std::fs::create_dir_all(&config.data_dir)?;
    config.data_dir = config.data_dir.canonicalize()?;
    std::fs::create_dir_all(config.data_dir.join("revisions"))?;
    std::fs::create_dir_all(config.data_dir.join("staging"))?;
    let store = Store::open(&config.data_dir)?;
    store.update(|s| {
        for d in &mut s.dictionaries {
            if d.index_status.state == "building" {
                d.index_status.state = "error".into();
                d.index_status.error = Some("索引任务被服务重启中断，请重建".into());
            }
        }
        Ok(())
    })?;
    let settings = store.load()?.settings;
    let catalog = Catalog::new(store.words()?);
    validate_snapshot(store.snapshot().as_ref())?;
    let static_dir = config.static_dir.clone();
    let address = config.listen;
    let app = Arc::new(App {
        config,
        store,
        catalog: RwLock::new(Arc::new(catalog)),
        catalog_publication: Mutex::new(()),
        readers: Mutex::new(VecDeque::new()),
        reader_generation: AtomicU64::new(0),
        open_gates: Mutex::new(HashMap::new()),
        workers: RwLock::new(Arc::new(Semaphore::new(number(
            &settings["performance"],
            "query_workers",
            4,
        )))),
        background: Arc::new(Semaphore::new(1)),
        jobs: Mutex::new(vec![]),
        caps: Mutex::new(HashMap::new()),
        mutation: tokio::sync::Mutex::new(()),
    });
    let routes = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"ok":true,"version":env!("CARGO_PKG_VERSION")})) }),
        )
        .route("/bootstrap", get(bootstrap))
        .route("/suggest", get(suggest))
        .route("/lookup", get(lookup))
        .route("/history", post(history_add).delete(history_clear))
        .route("/history/{id}", delete(history_remove))
        .route("/imports", post(import_upload))
        .route("/imports/path", post(import_path))
        .route("/imports/{id}/commit", post(import_commit))
        .route(
            "/dictionaries/{id}",
            put(dictionary_update).delete(dictionary_remove),
        )
        .route("/dictionaries/{id}/resources", post(resource_append))
        .route(
            "/dictionaries/{id}/resources/{rid}/entries",
            get(resource_entries),
        )
        .route(
            "/dictionaries/{id}/resources/{rid}/content",
            get(resource_preview),
        )
        .route("/groups", post(group_create))
        .route("/groups/{id}", put(group_update).delete(group_remove))
        .route("/settings", put(settings_update))
        .route("/logs", get(logs_get).delete(logs_clear))
        .route("/maintenance", post(maintenance))
        .route("/backup", get(backup))
        .route("/restore", post(restore))
        .route("/content/{id}/{revision}/{cap}/{*path}", get(content));
    let routes = routes.fallback(|| async { ApiError::missing() });
    let router = Router::new()
        .nest("/api/v1", routes)
        .fallback_service(
            tower_http::services::ServeDir::new(&static_dir).not_found_service(
                tower_http::services::ServeFile::new(static_dir.join("index.html")),
            ),
        )
        .layer(DefaultBodyLimit::max(imports::MAX_BYTES as usize))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app.clone());
    app.store.log("info", "service", "Service started");
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
async fn bootstrap(State(app): State<Shared>) -> Result<Json<Value>> {
    let s = app.store.snapshot();
    let (open_readers, cache_bytes) = {
        let readers = app.readers.lock().unwrap();
        (
            readers.len(),
            readers.iter().map(|(_, e)| e.cache_bytes()).sum::<usize>(),
        )
    };
    let words = app.catalog.read().unwrap().len();
    let jobs = app.jobs.lock().unwrap().clone();
    Ok(Json(
        json!({"dictionaries":s.dictionaries,"groups":s.groups,"settings":s.settings,"history":s.history,"stats":{"words":words,"open_readers":open_readers,"cache_bytes":cache_bytes,"jobs":jobs}}),
    ))
}
#[derive(Deserialize)]
struct SearchParams {
    q: String,
    #[serde(default = "all")]
    group: String,
    #[serde(default = "twenty")]
    limit: usize,
    #[serde(default)]
    mode: String,
}
fn all() -> String {
    "all".into()
}
fn twenty() -> usize {
    20
}
async fn suggest(State(app): State<Shared>, Query(p): Query<SearchParams>) -> Result<Json<Value>> {
    bounded_word(&p.q)?;
    if p.limit == 0 || p.limit > 100 {
        return Err(ApiError::bad("limit must be 1–100"));
    }
    if !matches!(p.mode.as_str(), "" | "headword" | "fulltext") {
        return Err(ApiError::bad("Invalid search mode"));
    }
    let start = Instant::now();
    let s = app.store.snapshot();
    let ids = search::ordered_ids(&s, &p.group).ok_or_else(ApiError::missing)?;
    let (items, truncated) = if p.mode == "fulltext" {
        let query = search::fts_query(&p.q);
        let db = app.store.path.clone();
        blocking(app.clone(),move |_|{
   let c=rusqlite::Connection::open(db)?;c.busy_timeout(Duration::from_secs(5))?;
   let mut items=Vec::new();if ids.is_empty(){return Ok((items,false));}
   let placeholders=(0..ids.len()).map(|_|"?").collect::<Vec<_>>().join(",");
   let sql=format!("SELECT word,group_concat(DISTINCT dictionary_id),substr(body,1,240) FROM fulltext WHERE fulltext MATCH ? AND dictionary_id IN ({placeholders}) GROUP BY word ORDER BY word LIMIT ?");
   let mut values=vec![rusqlite::types::Value::Text(query)];
   values.extend(ids.into_iter().map(rusqlite::types::Value::Text));
   values.push(rusqlite::types::Value::Integer((p.limit+1) as i64));
   let mut stmt=c.prepare(&sql)?;
   let rows=stmt.query_map(rusqlite::params_from_iter(values),|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?;
   for row in rows{let (word,dictionaries,preview)=row?;items.push(search::Suggestion{word,dictionary_ids:dictionaries.split(',').map(str::to_owned).collect(),match_kind:"fulltext".into(),preview:Some(preview)});}
   let truncated=items.len()>p.limit;items.truncate(p.limit);Ok((items,truncated))
  }).await?
    } else {
        let catalog = app.catalog.read().unwrap().clone();
        let distance = if s.settings["spelling"]["enabled"].as_bool().unwrap_or(true) {
            number(&s.settings["spelling"], "distance", 1)
        } else {
            0
        };
        blocking(app.clone(), move |_| {
            Ok(catalog.suggest(&p.q, &ids, p.limit, distance))
        })
        .await?
    };
    Ok(Json(
        json!({"items":items,"truncated":truncated,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
    ))
}
#[derive(Deserialize)]
struct LookupParams {
    q: String,
    dictionary: String,
}
fn public_origin(headers: &HeaderMap) -> Result<String> {
    if let Ok(origin) = std::env::var("FASTDICT_PUBLIC_ORIGIN") {
        let parsed =
            url::Url::parse(&origin).map_err(|_| ApiError::bad("Invalid public origin"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ApiError::bad("Invalid public origin"));
        }
        return Ok(origin.trim_end_matches('/').into());
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::bad("Missing Host"))?;
    Ok(format!("http://{host}"))
}
async fn lookup(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(p): Query<LookupParams>,
) -> Result<Json<Value>> {
    bounded_word(&p.q)?;
    let d = app.dictionary(&p.dictionary)?;
    if !d.enabled {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "dictionary_disabled",
            "词典已停用".into(),
        ));
    }
    let start = Instant::now();
    let origin = public_origin(&headers)?;
    let bindings = resource_fingerprint(&d);
    let base = {
        let mut caps = app.caps.lock().unwrap();
        let now = Instant::now();
        caps.retain(|_, capability| capability.expires > now);
        if let Some(capability) = caps.values().find(|capability| {
            capability.dictionary == d.id && capability.revision == d.revision &&
            capability.bindings == bindings && capability.origin == origin &&
            capability.expires > now + Duration::from_secs(300)
        }) {
            capability.base.clone()
        } else {
            if caps.len() >= 4096 {
                if let Some(oldest) = caps.iter().min_by_key(|(_, capability)| capability.expires).map(|(key, _)| key.clone()) {
                    caps.remove(&oldest);
                }
            }
            let token = crate::model::id();
            let base = format!("{origin}/api/v1/content/{}/{}/{token}/", d.id, d.revision);
            caps.insert(token, Capability {
                dictionary: d.id.clone(), revision: d.revision.clone(), bindings, origin,
                expires: now + Duration::from_secs(3600), base: base.clone(),
            });
            base
        }
    };
    let word = p.q.clone();
    let dictionary_id = d.id.clone();
    let articles = blocking(app, move |app| {
        let engine = app.engine(&d)?;
        let mut articles = engine.lookup(&p.q)?;
        let options = RenderOptions {
            asset_base: base,
            trust_scripts: d.trust_scripts,
            custom_css: d.custom_css,
            font_family: d.font_family,
            typography_mode: d.typography_mode,
            stylesheets: d
                .resources
                .iter()
                .filter(|r| r.enabled && r.kind == "css")
                .map(|r| r.key.clone())
                .collect(),
            scripts: d
                .resources
                .iter()
                .filter(|r| r.enabled && r.kind == "js")
                .map(|r| r.key.clone())
                .collect(),
            fonts: d
                .resources
                .iter()
                .filter(|r| r.enabled && r.kind == "font")
                .filter_map(|r| {
                    Path::new(&r.key)
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .map(|name| (name.to_owned(), r.key.clone()))
                })
                .collect(),
        };
        for article in &mut articles {
            article.html = render::render_document(&article.html, &options)?;
        }
        Ok(articles)
    })
    .await?;
    Ok(Json(
        json!({"dictionary_id":dictionary_id,"word":word,"articles":articles,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
    ))
}

#[derive(Deserialize)]
struct HistoryRequest {
    word: String,
    group_id: String,
}
async fn history_add(
    State(app): State<Shared>,
    Json(p): Json<HistoryRequest>,
) -> Result<Json<HistoryItem>> {
    bounded_word(&p.word)?;
    let item = HistoryItem {
        id: crate::model::id(),
        word: p.word,
        group_id: p.group_id,
        at: now(),
    };
    app.store.update(|s| {
        if item.group_id != "all" && !s.groups.iter().any(|g| g.id == item.group_id) {
            bail!("Unknown group");
        }
        if s.settings["application"]["history_enabled"]
            .as_bool()
            .unwrap_or(true)
        {
            s.history
                .retain(|h| h.word != item.word || h.group_id != item.group_id);
            s.history.insert(0, item.clone());
            s.history
                .truncate(number(&s.settings["application"], "history_limit", 200));
        }
        Ok(())
    })?;
    Ok(Json(item))
}
async fn history_clear(State(app): State<Shared>) -> Result<Json<Value>> {
    app.store.update(|s| {
        s.history.clear();
        Ok(())
    })?;
    Ok(Json(json!({"ok":true})))
}
async fn history_remove(
    State(app): State<Shared>,
    AxPath(id): AxPath<String>,
) -> Result<Json<Value>> {
    app.store.update(|s| {
        s.history.retain(|h| h.id != id);
        Ok(())
    })?;
    Ok(Json(json!({"ok":true})))
}
async fn logs_get(
    State(app): State<Shared>,
    Query(p): Query<HashMap<String, String>>,
) -> Result<Json<Value>> {
    let limit = p
        .get("limit")
        .map(|v| v.parse::<usize>())
        .transpose()
        .map_err(|_| ApiError::bad("Invalid limit"))?
        .unwrap_or(200);
    if limit > 2000 {
        return Err(ApiError::bad("limit must not exceed 2000"));
    }
    Ok(Json(json!({"items":app.store.logs(limit)?})))
}
async fn logs_clear(State(app): State<Shared>) -> Result<Json<Value>> {
    app.store.clear_logs()?;
    Ok(Json(json!({"ok":true})))
}
fn validate_group(g: &Group, s: &Snapshot) -> anyhow::Result<()> {
    if g.name.trim().is_empty() || g.name.len() > 200 {
        bail!("Group name must be 1–200 bytes");
    }
    let mut seen = HashSet::new();
    for id in &g.dictionary_ids {
        if !seen.insert(id) || !s.dictionaries.iter().any(|d| &d.id == id) {
            bail!("Invalid or duplicate dictionary reference");
        }
    }
    if g.sort.len() > 3
        || g.sort.iter().any(|r| {
            !matches!(r.field.as_str(), "name" | "entries" | "created")
                || !matches!(r.direction.as_str(), "asc" | "desc")
        })
    {
        bail!("Invalid sort rules");
    }
    Ok(())
}
#[derive(Deserialize)]
struct GroupPatch {
    name: Option<String>,
    dictionary_ids: Option<Vec<String>>,
    sort: Option<Vec<SortRule>>,
}
async fn group_create(State(app): State<Shared>, Json(p): Json<GroupPatch>) -> Result<Json<Group>> {
    let g = Group {
        id: crate::model::id(),
        name: p.name.ok_or_else(|| ApiError::bad("Missing name"))?,
        dictionary_ids: p.dictionary_ids.unwrap_or_default(),
        sort: p.sort.unwrap_or_default(),
    };
    app.store.update(|s| {
        validate_group(&g, s)?;
        {
            if s.groups.len() >= 1000 {
                bail!("Group limit is 1000");
            }
            s.groups.push(g.clone());
        };
        Ok(())
    })?;
    Ok(Json(g))
}
async fn group_update(
    State(app): State<Shared>,
    AxPath(id): AxPath<String>,
    Json(p): Json<GroupPatch>,
) -> Result<Json<Group>> {
    if !app.store.load()?.groups.iter().any(|g| g.id == id) {
        return Err(ApiError::missing());
    }
    let g = app.store.update(|s| {
        let index = s
            .groups
            .iter()
            .position(|g| g.id == id)
            .context("Unknown group")?;
        let mut g = s.groups[index].clone();
        if let Some(name) = p.name {
            g.name = name;
        }
        if let Some(ids) = p.dictionary_ids {
            g.dictionary_ids = ids;
        }
        if let Some(sort) = p.sort {
            g.sort = sort;
        }
        validate_group(&g, s)?;
        s.groups[index] = g.clone();
        Ok(g)
    })?;
    Ok(Json(g))
}
async fn group_remove(
    State(app): State<Shared>,
    AxPath(id): AxPath<String>,
) -> Result<Json<Value>> {
    if !app.store.load()?.groups.iter().any(|g| g.id == id) {
        return Err(ApiError::missing());
    }
    app.store.update(|s| {
        s.groups.retain(|g| g.id != id);
        if s.settings["application"]["default_group"] == id {
            s.settings["application"]["default_group"] = json!("all");
        }
        for h in &mut s.history {
            if h.group_id == id {
                h.group_id = "all".into();
            }
        }
        Ok(())
    })?;
    Ok(Json(json!({"ok":true})))
}
fn validate_settings(v: &Value, s: &Snapshot) -> anyhow::Result<()> {
    let defaults = default_settings();
    for (section, fields) in defaults.as_object().unwrap() {
        let actual = v
            .get(section)
            .and_then(Value::as_object)
            .context("Missing settings section")?;
        for (key, default) in fields.as_object().unwrap() {
            let value = actual.get(key).context("Missing settings field")?;
            if (default.is_boolean() && !value.is_boolean())
                || (default.is_string() && !value.is_string())
                || (default.is_number() && !value.is_number())
            {
                bail!("Invalid settings field type");
            }
        }
    }
    for (section, key, min, max) in [
        ("application", "history_limit", 0., 10000.),
        ("appearance", "font_size", 10., 48.),
        ("appearance", "line_height", 1., 3.),
        ("appearance", "content_width", 320., 2400.),
        ("spelling", "distance", 0., 2.),
        ("performance", "max_open_readers", 1., 64.),
        ("performance", "cache_mb", 1., 4096.),
        ("performance", "query_workers", 1., 32.),
    ] {
        let n = v[section][key].as_f64().context("Invalid number")?;
        if !n.is_finite() || n < min || n > max {
            bail!("Setting {section}.{key} is outside allowed range");
        }
        if key != "line_height" && n.fract() != 0. {
            bail!("Setting requires an integer");
        }
    }
    if !matches!(
        v["appearance"]["theme"].as_str(),
        Some("system" | "light" | "dark")
    ) || !matches!(
        v["security"]["external_links"].as_str(),
        Some("ask" | "allow" | "block")
    ) {
        bail!("Invalid settings enum");
    }
    if v["appearance"]["font_family"].as_str().unwrap_or("").len() > 500 {
        bail!("Font family too long");
    }
    let group = v["application"]["default_group"]
        .as_str()
        .context("Invalid default group")?;
    if group != "all" && !s.groups.iter().any(|g| g.id == group) {
        bail!("Default group does not exist");
    }
    Ok(())
}
async fn settings_update(State(app): State<Shared>, Json(v): Json<Value>) -> Result<Json<Value>> {
    let _mutation = app.mutation.lock().await;
    let old = app.store.load()?;
    validate_settings(&v, &old)?;
    let old_count = number(&old.settings["performance"], "query_workers", 4);
    let count = number(&v["performance"], "query_workers", 4);
    app.store.update(|s| {
        validate_settings(&v, s)?;
        s.settings = v.clone();
        s.history
            .truncate(number(&v["application"], "history_limit", 200));
        Ok(())
    })?;
    reconfigure_workers(&app, old_count, count).await?;
    app.evict(None);
    Ok(Json(v))
}
async fn receive_upload(mut multipart: Multipart, dest: &Path, field_name: &str) -> Result<()> {
    tokio::fs::create_dir_all(dest)
        .await
        .map_err(anyhow::Error::from)?;
    let mut total = 0u64;
    let mut count = 0usize;
    let mut names = HashSet::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
    {
        if field.name() != Some(field_name) {
            return Err(ApiError::bad("Unexpected multipart field"));
        }
        let name = field
            .file_name()
            .ok_or_else(|| ApiError::bad("Missing file name"))?
            .to_owned();
        let path = imports::safe_path(&name)?;
        if !names.insert(name) {
            return Err(ApiError::bad("Duplicate upload path"));
        }
        count += 1;
        if count > imports::MAX_FILES {
            return Err(ApiError(
                StatusCode::PAYLOAD_TOO_LARGE,
                "upload_limit",
                "Too many files".into(),
            ));
        }
        let path = dest.join(path);
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .map_err(anyhow::Error::from)?;
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .await
            .map_err(anyhow::Error::from)?;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| ApiError::bad(e.to_string()))?
        {
            total = total
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| ApiError::bad("Upload too large"))?;
            if total > imports::MAX_BYTES {
                return Err(ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "upload_limit",
                    "Upload exceeds 2 GiB".into(),
                ));
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(anyhow::Error::from)?;
        }
    }
    if count == 0 {
        return Err(ApiError::bad("No files uploaded"));
    }
    Ok(())
}
async fn import_upload(
    State(app): State<Shared>,
    multipart: Multipart,
) -> Result<Json<ImportPreview>> {
    let id = crate::model::id();
    let dest = app.config.data_dir.join("staging").join(&id);
    if let Err(error) = receive_upload(multipart, &dest, "files").await {
        let _ = tokio::fs::remove_dir_all(&dest).await;
        return Err(error);
    }
    let preview = blocking(app, move |_| imports::inspect(&dest, id)).await?;
    Ok(Json(preview))
}
#[derive(Deserialize)]
struct PathImport {
    path: String,
}
async fn import_path(
    State(app): State<Shared>,
    Json(p): Json<PathImport>,
) -> Result<Json<ImportPreview>> {
    if p.path.len() > 4096 {
        return Err(ApiError::bad("Path too long"));
    }
    let preview = blocking(app, move |app| {
        let id = crate::model::id();
        let dest = app.config.data_dir.join("staging").join(&id);
        if let Err(e) =
            imports::copy_authorized(Path::new(&p.path), &app.config.import_roots, &dest)
        {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(e);
        }
        imports::inspect(&dest, id)
    })
    .await?;
    Ok(Json(preview))
}
#[derive(Deserialize)]
struct CommitRequest {
    dictionaries: Vec<Candidate>,
}
async fn import_commit(
    State(app): State<Shared>,
    AxPath(import_id): AxPath<String>,
    Json(p): Json<CommitRequest>,
) -> Result<Json<Value>> {
    if !valid_id(&import_id) {
        return Err(ApiError::bad("Invalid import id"));
    }
    if p.dictionaries.is_empty() || p.dictionaries.len() > 1000 {
        return Err(ApiError::bad("Select 1–1000 dictionaries"));
    }
    let _mutation = app.mutation.lock().await;
    if !app
        .config
        .data_dir
        .join("staging")
        .join(&import_id)
        .is_dir()
    {
        return Err(ApiError::missing());
    }
    let dictionaries = blocking(app.clone(), move |app| {
        let stage = app.config.data_dir.join("staging").join(&import_id);
        let preview = imports::inspect(&stage, import_id)?;
        let files: HashMap<_, _> = preview.files.iter().map(|f| (f.path.as_str(), f)).collect();
        let revision = crate::model::id();
        let root = app.config.data_dir.join("revisions").join(&revision);
        let outcome = (|| -> anyhow::Result<Vec<Dictionary>> {
            imports::copy_tree(&stage, &root)?;
            let mut dictionaries = Vec::new();
            let mut catalogs = Vec::new();
            let mut seen = HashSet::new();
            for c in p.dictionaries {
                imports::safe_path(&c.mdx)?;
                if !seen.insert(c.mdx.clone())
                    || files.get(c.mdx.as_str()).is_none_or(|f| f.kind != "mdx")
                {
                    bail!("Invalid or duplicate MDX selection");
                }
                if c.name.trim().is_empty() || c.name.len() > 200 {
                    bail!("Invalid dictionary name");
                }
                let mut bindings = HashSet::new();
                let mut resources = Vec::new();
                for (order, path) in c.resources.iter().enumerate() {
                    let file = files
                        .get(path.as_str())
                        .context("Unknown resource binding")?;
                    if file.kind == "mdx" || !bindings.insert(path) {
                        bail!("Invalid or duplicate resource binding");
                    }
                    let key = if file.kind == "mdd" {
                        String::new()
                    } else {
                        let parent = Path::new(&c.mdx).parent().unwrap_or(Path::new(""));
                        Path::new(path)
                            .strip_prefix(parent)
                            .unwrap_or(Path::new(path))
                            .to_string_lossy()
                            .into_owned()
                    };
                    resources.push(Resource {
                        id: crate::model::id(),
                        path: path.clone(),
                        kind: file.kind.clone(),
                        size: file.size,
                        enabled: true,
                        order,
                        key,
                    });
                }
                let mounts = resources
                    .iter()
                    .map(|r| ResourceMount {
                        path: root.join(&r.path),
                        key: r.key.clone(),
                    })
                    .collect::<Vec<_>>();
                let engine = Engine::open(&root.join(&c.mdx), &root, &mounts, 1024 * 1024)?;
                let entries = engine.entries().len();
                let words = engine
                    .entries()
                    .iter()
                    .map(|e| e.headword.clone())
                    .collect::<Vec<_>>();
                let warnings = preview
                    .candidates
                    .iter()
                    .find(|v| v.mdx == c.mdx)
                    .map(|v| v.warnings.clone())
                    .unwrap_or_default();
                let icon = resources
                    .iter()
                    .find(|r| {
                        r.kind == "image"
                            && Path::new(&r.path).file_stem().is_some_and(|s| {
                                matches!(s.to_str(), Some("icon" | "favicon" | "logo"))
                            })
                    })
                    .map(|r| r.key.clone());
                let dictionary = Dictionary {
                    id: crate::model::id(),
                    name: c.name,
                    revision: revision.clone(),
                    enabled: true,
                    icon,
                    entries,
                    metadata: serde_json::to_value(engine.metadata())?,
                    resources,
                    trust_scripts: false,
                    custom_css: String::new(),
                    font_family: String::new(),
                    typography_mode: global_typography(),
                    warnings,
                    index_status: IndexStatus {
                        state: "missing".into(),
                        indexed: 0,
                        total: entries,
                        error: None,
                    },
                    mdx: c.mdx,
                    created: now(),
                };
                catalogs.push((dictionary.id.clone(), words));
                dictionaries.push(dictionary);
            }
            let _publication = app.catalog_publication.lock().unwrap();
            let mut words = app.store.words()?;
            for (id, entries) in &catalogs {
                words.extend(entries.iter().map(|word| (id.clone(), word.clone())));
            }
            let catalog = Arc::new(Catalog::new(words));
            app.store.commit_import(&dictionaries, &catalogs)?;
            *app.catalog.write().unwrap() = catalog;
            Ok(dictionaries)
        })();
        if outcome.is_err() {
            let _ = std::fs::remove_dir_all(&root);
        } else {
            let _ = std::fs::remove_dir_all(&stage);
            app.store
                .log("info", "import", "Dictionary import committed");
        }
        outcome
    })
    .await?;
    Ok(Json(json!({"dictionaries":dictionaries})))
}

fn present_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
#[derive(Deserialize)]
struct DictionaryPatch {
    name: Option<String>,
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "present_value")]
    icon: Option<Value>,
    trust_scripts: Option<bool>,
    custom_css: Option<String>,
    font_family: Option<String>,
    typography_mode: Option<String>,
    resources: Option<Vec<Resource>>,
}
fn validate_resources(resources: &[Resource], old: &Dictionary) -> anyhow::Result<()> {
    if resources.len() > imports::MAX_FILES {
        bail!("Too many resources");
    }
    let mut ids = HashSet::new();
    for r in resources {
        let original = old
            .resources
            .iter()
            .find(|v| v.id == r.id)
            .context("Unknown resource id")?;
        if !ids.insert(&r.id)
            || r.path != original.path
            || r.kind != original.kind
            || r.size != original.size
        {
            bail!("Resource source metadata cannot be modified");
        }
        if !(r.kind == "mdd" && (r.key.is_empty() || r.key == "/")) {
            imports::safe_path(r.key.trim_start_matches('/'))?;
        }
    }
    Ok(())
}
async fn dictionary_update(
    State(app): State<Shared>,
    AxPath(id): AxPath<String>,
    Json(p): Json<DictionaryPatch>,
) -> Result<Json<Dictionary>> {
    let _mutation = app.mutation.lock().await;
    let mut d = app.dictionary(&id)?;
    if let Some(name) = p.name {
        if name.trim().is_empty() || name.len() > 200 {
            return Err(ApiError::bad("Invalid name"));
        }
        d.name = name;
    }
    if let Some(enabled) = p.enabled {
        d.enabled = enabled;
    }
    if let Some(icon) = p.icon {
        d.icon = if icon.is_null() {
            None
        } else {
            let icon = icon
                .as_str()
                .ok_or_else(|| ApiError::bad("Icon must be a virtual resource path"))?;
            imports::safe_path(icon.trim_start_matches('/'))?;
            Some(icon.into())
        };
    }
    if let Some(trust) = p.trust_scripts {
        d.trust_scripts = trust;
    }
    if let Some(css) = p.custom_css {
        if css.len() > 256 * 1024 {
            return Err(ApiError::bad("CSS too large"));
        }
        d.custom_css = css;
    }
    if let Some(font) = p.font_family {
        if font.len() > 500 {
            return Err(ApiError::bad("Font family too long"));
        }
        d.font_family = font;
    }
    if let Some(mode) = p.typography_mode {
        if !matches!(mode.as_str(), "original" | "global" | "override") {
            return Err(ApiError::bad("Invalid typography mode"));
        }
        d.typography_mode = mode;
    }
    let d = blocking(app.clone(), move |app| {
        let rebound = p.resources.is_some();
        let mut new_root = None;
        if let Some(mut resources) = p.resources {
            validate_resources(&resources, &d)?;
            resources.sort_by_key(|r| r.order);
            for (order, resource) in resources.iter_mut().enumerate() {
                resource.order = order;
            }
            let revision = crate::model::id();
            let root = app.config.data_dir.join("revisions").join(&revision);
            if let Err(error) = imports::copy_tree(&app.root(&d), &root) {
                let _ = std::fs::remove_dir_all(&root);
                return Err(error);
            }
            d.revision = revision;
            d.resources = resources;
            new_root = Some(root);
            d.index_status = IndexStatus {
                state: "missing".into(),
                indexed: 0,
                total: d.entries,
                error: None,
            };
            cancel_jobs(&app, Some(&d.id), None)?;
        }
        if let Err(error) = app.store.update_dictionary(&d, rebound) {
            if let Some(root) = new_root {
                let _ = std::fs::remove_dir_all(root);
            }
            return Err(error);
        }
        app.evict(Some(&d.id));
        Ok(d)
    })
    .await?;
    Ok(Json(d))
}
async fn dictionary_remove(
    State(app): State<Shared>,
    AxPath(id): AxPath<String>,
) -> Result<Json<Value>> {
    let _mutation = app.mutation.lock().await;
    app.dictionary(&id)?;
    blocking(app.clone(), move |app| {
        cancel_jobs(&app, Some(&id), None)?;
        app.store.update(|s| {
            s.dictionaries.retain(|d| d.id != id);
            for g in &mut s.groups {
                g.dictionary_ids.retain(|v| v != &id);
            }
            Ok(())
        })?;
        app.store.clear_derived(Some(&id), true, true)?;
        app.refresh_catalog()?;
        app.evict(Some(&id));
        app.caps.lock().unwrap().retain(|_, c| c.dictionary != id);
        app.store.log(
            "info",
            "dictionary",
            "Dictionary unregistered; immutable source revisions retained",
        );
        Ok(())
    })
    .await?;
    Ok(Json(json!({"ok":true})))
}
async fn resource_append(
    State(app): State<Shared>,
    AxPath(dictionary_id): AxPath<String>,
    multipart: Multipart,
) -> Result<Json<Dictionary>> {
    app.dictionary(&dictionary_id)?;
    let stage = app.config.data_dir.join("staging").join(crate::model::id());
    if let Err(e) = receive_upload(multipart, &stage, "files").await {
        let _ = tokio::fs::remove_dir_all(&stage).await;
        return Err(e);
    }
    let _mutation = app.mutation.lock().await;
    let result = blocking(app.clone(), move |app| {
        let mut d = app
            .dictionary(&dictionary_id)
            .map_err(|e| anyhow::anyhow!(e.2))?;
        let revision = crate::model::id();
        let root = app.config.data_dir.join("revisions").join(&revision);
        let result = (|| -> anyhow::Result<Dictionary> {
            let files = imports::list(&stage)?;
            imports::copy_tree(&app.root(&d), &root)?;
            for file in files {
                if file.kind == "mdx" {
                    bail!("Cannot append MDX as a resource");
                }
                let target = root.join(imports::safe_path(&file.path)?);
                if target.exists() {
                    bail!("Resource path already exists; choose a different path");
                }
                std::fs::create_dir_all(target.parent().unwrap())?;
                std::fs::copy(stage.join(&file.path), target)?;
                let key = if file.kind == "mdd" {
                    String::new()
                } else {
                    let parent = Path::new(&d.mdx).parent().unwrap_or(Path::new(""));
                    Path::new(&file.path)
                        .strip_prefix(parent)
                        .unwrap_or(Path::new(&file.path))
                        .to_string_lossy()
                        .into_owned()
                };
                d.resources.push(Resource {
                    id: crate::model::id(),
                    path: file.path,
                    kind: file.kind,
                    size: file.size,
                    enabled: true,
                    order: d.resources.len(),
                    key,
                });
            }
            imports::list(&root)?;
            d.revision = revision;
            d.index_status = IndexStatus {
                state: "missing".into(),
                indexed: 0,
                total: d.entries,
                error: None,
            };
            cancel_jobs(&app, Some(&d.id), None)?;
            app.store.update_dictionary(&d, true)?;
            app.evict(Some(&d.id));
            Ok(d)
        })();
        let _ = std::fs::remove_dir_all(stage);
        if result.is_err() {
            let _ = std::fs::remove_dir_all(root);
        }
        result
    })
    .await?;
    Ok(Json(result))
}
#[derive(Deserialize)]
struct ResourceQuery {
    #[serde(default)]
    prefix: String,
    #[serde(default = "hundred")]
    limit: usize,
    #[serde(default)]
    path: String,
}
fn hundred() -> usize {
    100
}
async fn resource_entries(
    State(app): State<Shared>,
    AxPath((id, rid)): AxPath<(String, String)>,
    Query(p): Query<ResourceQuery>,
) -> Result<Json<Value>> {
    if p.limit == 0 || p.limit > 1000 || p.prefix.len() > 1024 {
        return Err(ApiError::bad("Invalid resource query"));
    }
    let d = app.dictionary(&id)?;
    let r = d
        .resources
        .iter()
        .find(|r| r.id == rid)
        .cloned()
        .ok_or_else(ApiError::missing)?;
    let limit = p.limit;
    let items = blocking(app, move |app| {
        if r.kind == "mdd" {
            let root = app.root(&d);
            let path = root.join(&r.path);
            let engine = Engine::open(
                &root.join(&d.mdx),
                &root,
                &[ResourceMount {
                    path: path.clone(),
                    key: String::new(),
                }],
                1024 * 1024,
            )?;
            engine.resource_entries(&path, &p.prefix, limit + 1)
        } else {
            Ok(if r.key.starts_with(&p.prefix) {
                vec![(r.key, r.size)]
            } else {
                vec![]
            })
        }
    })
    .await?;
    let truncated = items.len() > limit;
    Ok(Json(
        json!({"items":items.into_iter().take(limit).map(|(path,size)|json!({"path":path,"size":size})).collect::<Vec<_>>(),"truncated":truncated}),
    ))
}
fn bytes_response(bytes: axum::body::Bytes, path: &str, asset: bool) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mime = match mime.essence_str() {
        "text/html" | "application/xhtml+xml" => "application/octet-stream",
        v => v,
    };
    let mut response = Body::from(bytes).into_response();
    if let Ok(value) = HeaderValue::from_str(mime) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    if asset {
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    } else {
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment"),
        );
    }
    response
}
async fn resource_preview(
    State(app): State<Shared>,
    AxPath((id, rid)): AxPath<(String, String)>,
    Query(p): Query<ResourceQuery>,
) -> Result<Response> {
    let d = app.dictionary(&id)?;
    let r = d
        .resources
        .iter()
        .find(|r| r.id == rid)
        .cloned()
        .ok_or_else(ApiError::missing)?;
    let path = if r.kind == "mdd" {
        imports::safe_path(p.path.trim_start_matches('/'))?;
        p.path
    } else {
        r.path.clone()
    };
    let display = path.clone();
    let bytes = blocking(app, move |app| {
        if r.kind == "mdd" {
            let root = app.root(&d);
            let mount = ResourceMount {
                path: root.join(&r.path),
                key: String::new(),
            };
            let engine = Engine::open(&root.join(&d.mdx), &root, &[mount], 1024 * 1024)?;
            let bytes = engine.resource(&path)?;
            if bytes.as_ref().len() > 32 * 1024 * 1024 {
                bail!("Preview exceeds 32 MiB");
            }
            Ok(axum::body::Bytes::from_owner(bytes))
        } else {
            let path = app.root(&d).join(&r.path);
            if std::fs::metadata(&path)?.len() > 32 * 1024 * 1024 {
                bail!("Preview exceeds 32 MiB");
            }
            Ok(axum::body::Bytes::from(std::fs::read(path)?))
        }
    })
    .await?;
    Ok(bytes_response(bytes, &display, false))
}
async fn content(
    State(app): State<Shared>,
    AxPath((id, revision, cap, path)): AxPath<(String, String, String, String)>,
) -> Result<Response> {
    imports::safe_path(path.trim_start_matches('/'))?;
    let (base, bindings) = {
        let caps = app.caps.lock().unwrap();
        let c = caps
            .get(&cap)
            .filter(|c| c.dictionary == id && c.revision == revision && c.expires > Instant::now())
            .ok_or_else(|| {
                ApiError(
                    StatusCode::FORBIDDEN,
                    "invalid_capability",
                    "内容访问凭据无效或已过期".into(),
                )
            })?;
        (c.base.clone(), c.bindings)
    };
    let d = app.dictionary(&id)?;
    if d.revision != revision || resource_fingerprint(&d) != bindings {
        return Err(ApiError(
            StatusCode::GONE,
            "stale_revision",
            "词典资源已更新，请重新查词".into(),
        ));
    }
    let display = path.clone();
    let bytes = blocking(app, move |app| {
        let bytes = app.engine(&d)?.resource(&path)?;
        if bytes.as_ref().len() > 64 * 1024 * 1024 {
            bail!("Resource exceeds 64 MiB response limit");
        }
        if imports::kind(&path) == "css" {
            let css = std::str::from_utf8(bytes.as_ref()).context("CSS must be UTF-8")?;
            Ok(axum::body::Bytes::from(render::rewrite_css(css, &base, &path)))
        } else {
            Ok(axum::body::Bytes::from_owner(bytes))
        }
    })
    .await?;
    Ok(bytes_response(bytes, &display, true))
}

#[derive(Deserialize)]
struct MaintenanceRequest {
    target: String,
    action: String,
    dictionary_id: Option<String>,
}
fn job_active(app: &App, id: &str) -> bool {
    app.jobs
        .lock()
        .unwrap()
        .iter()
        .any(|j| j.id == id && j.state == "running")
}
fn job_progress(app: &App, id: &str, progress: usize) {
    if let Some(j) = app.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
        j.progress = progress;
    }
}
async fn maintenance(
    State(app): State<Shared>,
    Json(p): Json<MaintenanceRequest>,
) -> Result<Json<Value>> {
    let _mutation = app.mutation.lock().await;
    if !matches!(p.target.as_str(), "headwords" | "fulltext" | "cache")
        || !matches!(p.action.as_str(), "clear" | "rebuild")
    {
        return Err(ApiError::bad("Invalid maintenance operation"));
    }
    let dictionaries = app
        .store
        .load()?
        .dictionaries
        .into_iter()
        .filter(|d| p.dictionary_id.as_ref().is_none_or(|id| id == &d.id))
        .collect::<Vec<_>>();
    if p.dictionary_id.is_some() && dictionaries.is_empty() {
        return Err(ApiError::missing());
    }
    cancel_jobs(&app, p.dictionary_id.as_deref(), Some(&p.target))?;
    if p.action == "clear" {
        if p.target == "cache" {
            app.evict(p.dictionary_id.as_deref());
        } else {
            app.store.clear_derived(
                p.dictionary_id.as_deref(),
                p.target == "headwords",
                p.target == "fulltext",
            )?;
            if p.target == "headwords" {
                app.refresh_catalog()?;
            } else {
                app.store.update(|s| {
                    for d in &mut s.dictionaries {
                        if p.dictionary_id.as_ref().is_none_or(|id| id == &d.id) {
                            d.index_status = IndexStatus {
                                state: "missing".into(),
                                indexed: 0,
                                total: d.entries,
                                error: None,
                            };
                        }
                    }
                    Ok(())
                })?;
            }
        }
        return Ok(Json(json!({"job_id":null})));
    }
    let job_id = crate::model::id();
    let job = Job {
        id: job_id.clone(),
        kind: p.target.clone(),
        dictionary_id: p.dictionary_id.clone(),
        state: "queued".into(),
        progress: 0,
        total: if p.target == "fulltext" {
            dictionaries.iter().map(|d| d.entries).sum()
        } else {
            dictionaries.len()
        },
        error: None,
    };
    {
        let mut jobs = app.jobs.lock().unwrap();
        if jobs.iter().filter(|j| matches!(j.state.as_str(), "queued" | "running")).count() >= 64 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "job_limit",
                "Too many background jobs".into(),
            ));
        }
        while jobs.len() >= 256 {
            if let Some(index) = jobs.iter().position(|j| matches!(j.state.as_str(), "done" | "error")) {
                jobs.remove(index);
            } else {
                break;
            }
        }
        jobs.push(job);
    }
    if p.target == "fulltext" {
        app.store.update(|s| {
            for d in &mut s.dictionaries {
                if dictionaries.iter().any(|v| v.id == d.id) {
                    d.index_status = IndexStatus {
                        state: "building".into(),
                        indexed: 0,
                        total: d.entries,
                        error: None,
                    };
                }
            }
            Ok(())
        })?;
    }
    let returned = job_id.clone();
    let task_app = app.clone();
    tokio::spawn(async move {
        let app = task_app;
        let Ok(permit) = app.background.clone().acquire_owned().await else {
            return;
        };
        let app2 = app.clone();
        let id2 = job_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            {
                let mut jobs = app2.jobs.lock().unwrap();
                let Some(j) = jobs.iter_mut().find(|j| j.id == id2 && j.state == "queued") else {
                    return Ok(());
                };
                j.state = "running".into();
            }
            run_maintenance(&app2, &id2, &p.target, &dictionaries)
        })
        .await;
        let error = match result {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(e) => Some(e.to_string()),
        };
        let mut jobs = app.jobs.lock().unwrap();
        if let Some(j) = jobs
            .iter_mut()
            .find(|j| j.id == job_id && j.state == "running")
        {
            j.state = if error.is_some() { "error" } else { "done" }.into();
            j.error = error;
            if j.state == "done" {
                j.progress = j.total;
            }
        }
    });
    Ok(Json(json!({"job_id":returned})))
}
fn run_maintenance(
    app: &App,
    job: &str,
    target: &str,
    dictionaries: &[Dictionary],
) -> anyhow::Result<()> {
    let mut progress = 0;
    for d in dictionaries {
        if !job_active(app, job) {
            return Ok(());
        }
        if !app
            .store
            .load()?
            .dictionaries
            .iter()
            .any(|v| v.id == d.id && v.revision == d.revision)
        {
            continue;
        }
        let result = (|| -> anyhow::Result<()> {
            if target == "cache" {
                app.evict(Some(&d.id));
                app.engine(d)?;
                if !job_active(app, job) {
                    app.evict(Some(&d.id));
                    return Ok(());
                }
                progress += 1;
                job_progress(app, job, progress);
                return Ok(());
            }
            let root = app.root(d);
            let engine = Engine::open(&root.join(&d.mdx), &root, &[], 4 * 1024 * 1024)?;
            if target == "headwords" {
                let words = engine
                    .entries()
                    .iter()
                    .map(|e| e.headword.clone())
                    .collect::<Vec<_>>();
                let jobs = app.jobs.lock().unwrap();
                if !jobs.iter().any(|j| j.id == job && j.state == "running") {
                    return Ok(());
                }
                let _publication = app.catalog_publication.lock().unwrap();
                app.store.set_words(&d.id, &words)?;
                *app.catalog.write().unwrap() = Arc::new(Catalog::new(app.store.words()?));
                drop(jobs);
                progress += 1;
                job_progress(app, job, progress);
                return Ok(());
            }
            let mut db = rusqlite::Connection::open(&app.store.path)?;
            db.busy_timeout(Duration::from_secs(5))?;
            {
                let jobs = app.jobs.lock().unwrap();
                if !jobs.iter().any(|j| j.id == job && j.state == "running") {
                    return Ok(());
                }
                db.execute("DELETE FROM fulltext WHERE dictionary_id=?1", [&d.id])?;
            }
            let mut batch = Vec::with_capacity(100);
            let mut batch_bytes = 0usize;
            for (record, entry) in engine.entries().iter().enumerate() {
                if !job_active(app, job) {
                    return Ok(());
                }
                let text = render::plain_text(&engine.record_text(record)?);
                let body = search::tokenize(&text);
                batch_bytes += body.len();
                batch.push((record, entry.headword.clone(), body));
                if batch.len() == 100
                    || batch_bytes >= 1024 * 1024
                    || record + 1 == engine.entries().len()
                {
                    let mut jobs = app.jobs.lock().unwrap();
                    let Some(active) = jobs
                        .iter_mut()
                        .find(|j| j.id == job && j.state == "running")
                    else {
                        return Ok(());
                    };
                    let tx = db.transaction()?;
                    {
                        let mut insert=tx.prepare("INSERT INTO fulltext(dictionary_id,record_id,word,body) VALUES(?1,?2,?3,?4)")?;
                        for (record, word, body) in &batch {
                            insert.execute(rusqlite::params![d.id, *record as i64, word, body])?;
                        }
                    }
                    tx.commit()?;
                    progress += batch.len();
                    batch.clear();
                    batch_bytes = 0;
                    active.progress = progress;
                    app.store.update(|s| {
                        if let Some(live) = s.dictionaries.iter_mut().find(|v| {
                            v.id == d.id
                                && v.revision == d.revision
                                && v.index_status.state == "building"
                        }) {
                            live.index_status.indexed = record + 1;
                        }
                        Ok(())
                    })?;
                    drop(jobs);
                    std::thread::yield_now();
                }
            }
            let jobs = app.jobs.lock().unwrap();
            if jobs.iter().any(|j| j.id == job && j.state == "running") {
                app.store.update(|s| {
                    if let Some(live) = s
                        .dictionaries
                        .iter_mut()
                        .find(|v| v.id == d.id && v.revision == d.revision)
                    {
                        live.index_status = IndexStatus {
                            state: "ready".into(),
                            indexed: d.entries,
                            total: d.entries,
                            error: None,
                        };
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let jobs = app.jobs.lock().unwrap();
            if target == "fulltext" && jobs.iter().any(|j| j.id == job && j.state == "running") {
                app.store.update(|s| {
                    for live in &mut s.dictionaries {
                        if live.index_status.state == "building"
                            && dictionaries
                                .iter()
                                .any(|v| v.id == live.id && v.revision == live.revision)
                        {
                            live.index_status.state = "error".into();
                            live.index_status.error = Some(error.to_string());
                        }
                    }
                    Ok(())
                })?;
            }
            app.store
                .log("error", "maintenance", "Derived index maintenance failed");
            return Err(error);
        }
    }
    Ok(())
}
async fn backup(
    State(app): State<Shared>,
    Query(p): Query<HashMap<String, String>>,
) -> Result<Response> {
    let include = p.get("include_files").is_some_and(|v| v == "true");
    let file=blocking(app,move|app|{
  let snapshot=app.store.snapshot();
  let manifest=serde_json::to_vec_pretty(snapshot.as_ref())?;
  if manifest.len()>16*1024*1024{bail!("Backup manifest exceeds 16 MiB");}
  let mut file=tempfile::tempfile()?;
  if include{
   let mut archive=zip::ZipWriter::new(file);
   let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
   archive.start_file("manifest.json",options)?;archive.write_all(&manifest)?;
   let mut seen=HashSet::new();let mut total=manifest.len()as u64;let mut files=1usize;
   for d in &snapshot.dictionaries{
    if !seen.insert(d.revision.clone()){continue;}
    let root=app.root(d);
    for entry in imports::list(&root)?{
     total=total.checked_add(entry.size).context("Backup too large")?;files+=1;
     if total>imports::MAX_BYTES||files>imports::MAX_FILES{bail!("Source backup exceeds 2 GiB or 20000 files; use configuration-only backup or export dictionaries separately");}
     archive.start_file(format!("revisions/{}/{}",d.revision,entry.path),options)?;
     let mut source=std::fs::File::open(root.join(entry.path))?;
     std::io::copy(&mut source,&mut archive)?;
    }
   }
   file=archive.finish()?;
  }else{file.write_all(&manifest)?;}
  std::io::Seek::rewind(&mut file)?;Ok(file)
 }).await?;
    let stream = tokio_util::io::ReaderStream::new(tokio::fs::File::from_std(file));
    let mut response = Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(if include {
            "application/zip"
        } else {
            "application/json"
        }),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(if include {
            "attachment; filename=\"fastdict-backup.zip\""
        } else {
            "attachment; filename=\"fastdict-backup.json\""
        }),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
fn validate_snapshot(snapshot: &Snapshot) -> anyhow::Result<()> {
    if snapshot.version != 1 {
        bail!("Unsupported backup schema version");
    }
    if snapshot.dictionaries.len() > 1000
        || snapshot.groups.len() > 1000
        || snapshot.history.len() > 10000
    {
        bail!("Backup object limit exceeded");
    }
    let mut ids = HashSet::new();
    for d in &snapshot.dictionaries {
        if !valid_id(&d.id)
            || !valid_id(&d.revision)
            || !ids.insert(&d.id)
            || d.name.is_empty()
            || d.name.len() > 200
            || d.custom_css.len() > 256 * 1024
            || d.font_family.len() > 500
        {
            bail!("Invalid dictionary in backup");
        }
        imports::safe_path(&d.mdx)?;
        if imports::kind(&d.mdx) != "mdx" {
            bail!("Dictionary source must be MDX");
        }
        {
            if !matches!(
                d.typography_mode.as_str(),
                "original" | "global" | "override"
            ) {
                bail!("Invalid typography mode");
            }
            validate_resources(&d.resources, d)?;
        };
        for r in &d.resources {
            if !valid_id(&r.id) {
                bail!("Invalid resource id");
            }
            imports::safe_path(&r.path)?;
            if imports::kind(&r.path) != r.kind || r.kind == "mdx" {
                bail!("Invalid resource kind");
            }
        }
    }
    let mut groups = HashSet::new();
    for g in &snapshot.groups {
        if !valid_id(&g.id) || !groups.insert(&g.id) {
            bail!("Invalid group id");
        }
        validate_group(g, snapshot)?;
    }
    validate_settings(&snapshot.settings, snapshot)?;
    for h in &snapshot.history {
        if !valid_id(&h.id)
            || h.word.is_empty()
            || h.word.len() > 512
            || (h.group_id != "all" && !snapshot.groups.iter().any(|g| g.id == h.group_id))
        {
            bail!("Invalid history item");
        }
    }
    Ok(())
}
async fn restore(State(app): State<Shared>, multipart: Multipart) -> Result<Json<Value>> {
    let stage = app.config.data_dir.join("staging").join(crate::model::id());
    if let Err(e) = receive_upload(multipart, &stage, "file").await {
        let _ = tokio::fs::remove_dir_all(&stage).await;
        return Err(e);
    }
    let _mutation = app.mutation.lock().await;
    let old_count = number(
        &app.store.load()?.settings["performance"],
        "query_workers",
        4,
    );
    blocking(app.clone(), move |app| {
        let result = restore_files(&app, &stage);
        let _ = std::fs::remove_dir_all(&stage);
        result
    })
    .await?;
    let count = number(
        &app.store.load()?.settings["performance"],
        "query_workers",
        4,
    );
    reconfigure_workers(&app, old_count, count).await?;
    app.store.log(
        "info",
        "restore",
        "Backup restored; dictionary script authorization disabled",
    );
    Ok(Json(json!({"restored":true})))
}
fn restore_files(app: &App, stage: &Path) -> anyhow::Result<()> {
    let uploads = imports::list(stage)?;
    if uploads.len() != 1 {
        bail!("Restore requires exactly one file");
    }
    let source = stage.join(&uploads[0].path);
    let mut file = std::fs::File::open(&source)?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic)?;
    std::io::Seek::rewind(&mut file)?;
    let extracted = stage.join("unpacked");
    let archive = n == 4 && magic == [b'P', b'K', 3, 4];
    let mut snapshot: Snapshot;
    if archive {
        let mut zip = zip::ZipArchive::new(file)?;
        if zip.len() > imports::MAX_FILES {
            bail!("Backup has too many files");
        }
        let mut total = 0u64;
        let mut paths = HashSet::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name().trim_end_matches('/').to_owned();
            let relative = imports::safe_path(&name)?;
            if !paths.insert(name.clone()) {
                bail!("Duplicate archive path");
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                bail!("Archive symlinks are forbidden");
            }
            if name != "manifest.json" && name != "revisions" && !name.starts_with("revisions/") {
                bail!("Unknown backup member");
            }
            let size = entry.size();
            total = total.checked_add(size).context("Backup too large")?;
            if total > imports::MAX_BYTES {
                bail!("Expanded backup exceeds 2 GiB");
            }
            let dest = extracted.join(relative);
            if entry.is_dir() {
                std::fs::create_dir_all(dest)?;
                continue;
            }
            std::fs::create_dir_all(dest.parent().unwrap())?;
            let mut output = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(dest)?;
            let copied = std::io::copy(&mut entry.by_ref().take(size + 1), &mut output)?;
            if copied != size {
                bail!("Archive size mismatch");
            }
        }
        let manifest = extracted.join("manifest.json");
        if std::fs::metadata(&manifest)?.len() > 16 * 1024 * 1024 {
            bail!("Backup manifest too large");
        }
        snapshot = serde_json::from_reader(std::fs::File::open(manifest)?)?;
    } else {
        if uploads[0].size > 16 * 1024 * 1024 {
            bail!("JSON backup exceeds 16 MiB");
        }
        snapshot = serde_json::from_reader(file)?;
    }
    validate_snapshot(&snapshot)?;
    let mut new_roots = Vec::new();
    let result = (|| -> anyhow::Result<()> {
        let mut revisions = HashMap::new();
        if archive {
            for d in &snapshot.dictionaries {
                if revisions.contains_key(&d.revision) {
                    continue;
                }
                let source = extracted.join("revisions").join(&d.revision);
                if !source.is_dir() {
                    bail!("Backup is missing referenced revision");
                }
                let revision = crate::model::id();
                let dest = app.config.data_dir.join("revisions").join(&revision);
                new_roots.push(dest.clone());
                imports::copy_tree(&source, &dest)?;
                revisions.insert(d.revision.clone(), revision);
            }
        }
        let mut catalogs = Vec::new();
        for d in &mut snapshot.dictionaries {
            if let Some(revision) = revisions.get(&d.revision) {
                d.revision = revision.clone();
            }
            let root = app.root(d);
            if !root.is_dir() {
                bail!(
                    "Configuration references unavailable managed source files; restore an include_files backup"
                );
            }
            imports::list(&root)?;
            for r in &d.resources {
                if std::fs::metadata(root.join(&r.path))?.len() != r.size {
                    bail!("Resource size does not match manifest");
                }
            }
            d.resources.sort_by_key(|r| r.order);
            let mounts = d
                .resources
                .iter()
                .filter(|r| r.enabled)
                .map(|r| ResourceMount {
                    path: root.join(&r.path),
                    key: r.key.clone(),
                })
                .collect::<Vec<_>>();
            let engine = Engine::open(&root.join(&d.mdx), &root, &mounts, 1024 * 1024)?;
            d.entries = engine.entries().len();
            d.metadata = serde_json::to_value(engine.metadata())?;
            d.trust_scripts = false;
            d.index_status = IndexStatus {
                state: "missing".into(),
                indexed: 0,
                total: d.entries,
                error: None,
            };
            catalogs.push((
                d.id.clone(),
                engine
                    .entries()
                    .iter()
                    .map(|e| e.headword.clone())
                    .collect::<Vec<_>>(),
            ));
        }
        let catalog = Arc::new(Catalog::new(
            catalogs
                .iter()
                .flat_map(|(id, words)| words.iter().map(move |word| (id.clone(), word.clone())))
                .collect(),
        ));
        cancel_jobs(app, None, None)?;
        let _publication = app.catalog_publication.lock().unwrap();
        app.store.restore_snapshot(snapshot, &catalogs)?;
        *app.catalog.write().unwrap() = catalog;
        app.evict(None);
        app.caps.lock().unwrap().clear();
        Ok(())
    })();
    if result.is_err() {
        for root in new_roots {
            let _ = std::fs::remove_dir_all(root);
        }
    }
    result
}

fn cancel_jobs(app: &App, dictionary: Option<&str>, target: Option<&str>) -> anyhow::Result<()> {
    let mut jobs = app.jobs.lock().unwrap();
    let mut cancelled_fulltext = Vec::new();
    for j in jobs
        .iter_mut()
        .filter(|j| matches!(j.state.as_str(), "queued" | "running"))
    {
        if target.is_none_or(|t| j.kind == t)
            && dictionary.is_none_or(|id| j.dictionary_id.as_deref().is_none_or(|d| d == id))
        {
            j.state = "error".into();
            j.error = Some("Cancelled by a newer operation".into());
            if j.kind == "fulltext" {
                cancelled_fulltext.push(j.dictionary_id.clone());
            }
        }
    }
    if !cancelled_fulltext.is_empty() {
        app.store.update(|s| {
            for d in &mut s.dictionaries {
                if d.index_status.state == "building"
                    && cancelled_fulltext
                        .iter()
                        .any(|id| id.as_ref().is_none_or(|id| id == &d.id))
                {
                    d.index_status.state = "missing".into();
                    d.index_status.error = None;
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

async fn reconfigure_workers(app: &App, old_count: usize, count: usize) -> Result<()> {
    if old_count == count {
        return Ok(());
    }
    let previous = app.workers.read().unwrap().clone();
    let drained = previous
        .clone()
        .acquire_many_owned(old_count as u32)
        .await
        .map_err(|_| ApiError::bad("Worker pool is unavailable"))?;
    *app.workers.write().unwrap() = Arc::new(Semaphore::new(count));
    previous.close();
    drop(drained);
    Ok(())
}
