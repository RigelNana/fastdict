use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use reqwest::blocking::{Client, Response, multipart};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "fastdict",
    version,
    about = "fastdict dictionary service and shared HTTP API client",
    after_help = "Client credentials: set FASTDICT_TOKEN (recommended) or use --token-file. Endpoint may include /api/v1. JSON is emitted on stdout; failures on stderr. No command reads credentials from dictionary content."
)]
struct Cli {
    #[arg(
        long,
        env = "FASTDICT_ENDPOINT",
        default_value = "http://127.0.0.1:8765",
        global = true
    )]
    endpoint: String,
    #[arg(
        long,
        env = "FASTDICT_TOKEN_FILE",
        global = true,
        help = "File containing the bearer token; prefer this or FASTDICT_TOKEN over visible argv credentials"
    )]
    token_file: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = 120,
        global = true,
        help = "HTTP request timeout in seconds"
    )]
    timeout: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run HTTP service (credentials read from FASTDICT_TOKEN or --token-file).
    Serve {
        #[arg(long, default_value = "127.0.0.1:8765")]
        listen: SocketAddr,
        /// Data directory (defaults to .fastdict, or an existing .mydict installation).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        import_root: Vec<PathBuf>,
        #[arg(long, default_value = "./dist")]
        static_dir: PathBuf,
    },
    /// Query enabled dictionary suggestions without loading article bodies.
    Suggest {
        query: String,
        #[arg(long, default_value = "all")]
        group: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, value_enum, default_value = "headword")]
        mode: SearchMode,
    },
    /// Look up one dictionary or all exact matches in a group (default: all).
    Lookup {
        query: String,
        #[arg(long, conflicts_with = "group")]
        dictionary: Option<String>,
        #[arg(long, conflicts_with = "dictionary")]
        group: Option<String>,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    #[command(subcommand)]
    Dictionaries(DictionaryCommand),
    #[command(subcommand)]
    Groups(GroupCommand),
    #[command(subcommand)]
    Settings(SettingsCommand),
    #[command(subcommand)]
    History(HistoryCommand),
    #[command(subcommand)]
    Logs(LogsCommand),
    #[command(subcommand)]
    Index(IndexCommand),
    /// Save a versioned backup. --include-files is needed to move sources to another server.
    Backup {
        file: PathBuf,
        #[arg(long)]
        include_files: bool,
    },
    /// Restore a backup, replacing service configuration. Requires --yes.
    Restore {
        file: PathBuf,
        #[arg(long, required = true)]
        yes: bool,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum SearchMode {
    Headword,
    Fulltext,
}
#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
    Html,
}
#[derive(Clone, Copy, ValueEnum)]
enum IndexTarget {
    Headwords,
    Fulltext,
    Cache,
}
impl IndexTarget {
    fn name(self) -> &'static str {
        match self {
            Self::Headwords => "headwords",
            Self::Fulltext => "fulltext",
            Self::Cache => "cache",
        }
    }
}

#[derive(Subcommand)]
enum DictionaryCommand {
    List,
    Info {
        id: String,
    },
    /// Upload files/directories recursively; --dry-run stages a preview without registering dictionaries.
    Import {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Stage an allowed server-host directory; --dry-run prints its preview only.
    ImportPath {
        path: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Commit a prior preview using JSON {dictionaries:[{mdx,name,resources}]} from --file.
    Commit {
        preview_id: String,
        #[arg(long)]
        file: PathBuf,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    /// Apply dictionary configuration from a JSON object (name/icon/styles/trust/resources).
    Update {
        id: String,
        file: PathBuf,
    },
    /// Unregister only the named dictionary; source revisions may remain while readers use them.
    Remove {
        id: String,
        #[arg(long, required = true)]
        yes: bool,
    },
    #[command(subcommand)]
    Resources(ResourceCommand),
}
#[derive(Subcommand)]
enum ResourceCommand {
    List {
        dictionary: String,
    },
    Add {
        dictionary: String,
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Browse a resource/MDD's virtual entries.
    Entries {
        dictionary: String,
        resource: String,
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Write authenticated resource bytes to a file (or '-' for stdout).
    Get {
        dictionary: String,
        resource: String,
        #[arg(long, default_value = "")]
        path: String,
        #[arg(long)]
        output: PathBuf,
    },
    Enable {
        dictionary: String,
        resource: String,
    },
    Disable {
        dictionary: String,
        resource: String,
    },
    Remove {
        dictionary: String,
        resource: String,
        #[arg(long, required = true)]
        yes: bool,
    },
    /// Edit the virtual key used to resolve this resource.
    Key {
        dictionary: String,
        resource: String,
        key: String,
    },
    /// Set the complete resource precedence order by resource IDs.
    Order {
        dictionary: String,
        #[arg(required = true)]
        ids: Vec<String>,
    },
}
#[derive(Subcommand)]
enum GroupCommand {
    List,
    Create {
        name: String,
        #[arg(long = "dictionary")]
        dictionaries: Vec<String>,
    },
    /// Apply JSON {name?,dictionary_ids?,sort?}; '-' reads stdin.
    Update {
        id: String,
        file: PathBuf,
    },
    /// Replace membership/manual order. Use --empty to explicitly remove all members.
    Order {
        id: String,
        ids: Vec<String>,
        #[arg(long)]
        empty: bool,
    },
    Remove {
        id: String,
        #[arg(long, required = true)]
        yes: bool,
    },
}
#[derive(Subcommand)]
enum SettingsCommand {
    Get,
    /// Replace the complete settings object with JSON from file or '-'.
    Set {
        file: PathBuf,
    },
}
#[derive(Subcommand)]
enum HistoryCommand {
    List,
    Add {
        word: String,
        #[arg(long, default_value = "all")]
        group: String,
    },
    Remove {
        id: String,
        #[arg(long, required = true)]
        yes: bool,
    },
    Clear {
        #[arg(long, required = true)]
        yes: bool,
    },
}
#[derive(Subcommand)]
enum LogsCommand {
    List {
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Clear {
        #[arg(long, required = true)]
        yes: bool,
    },
}
#[derive(Args)]
struct Maintenance {
    #[arg(value_enum)]
    target: IndexTarget,
    #[arg(long, conflicts_with = "all", required_unless_present = "all")]
    dictionary: Option<String>,
    #[arg(
        long,
        conflicts_with = "dictionary",
        help = "Explicitly target every dictionary"
    )]
    all: bool,
    #[arg(long, help = "Wait for the returned background job to finish")]
    wait: bool,
    #[arg(long, default_value_t = 3600)]
    wait_timeout: u64,
}
#[derive(Subcommand)]
enum IndexCommand {
    Status {
        #[arg(long)]
        dictionary: Option<String>,
    },
    Clear {
        #[command(flatten)]
        scope: Maintenance,
        #[arg(long, required = true)]
        yes: bool,
    },
    Rebuild {
        #[command(flatten)]
        scope: Maintenance,
    },
}

struct Api {
    client: Client,
    base: String,
    token: Option<String>,
}
impl Api {
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder {
        let request = self
            .client
            .request(method, format!("{}{}", self.base, path));
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
    fn json(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let request = self.request(method, path);
        let response = match body {
            Some(body) => request.json(body),
            None => request,
        }
        .send()?;
        parse_json(response)
    }
    fn bootstrap(&self) -> Result<Value> {
        self.json(reqwest::Method::GET, "/bootstrap", None)
    }
    fn dictionary(&self, id: &str) -> Result<Value> {
        self.bootstrap()?["dictionaries"]
            .as_array()
            .context("Missing dictionaries")?
            .iter()
            .find(|d| d["id"].as_str() == Some(id))
            .cloned()
            .with_context(|| format!("Unknown dictionary: {id}"))
    }
    fn upload(&self, path: &str, paths: &[PathBuf]) -> Result<Value> {
        parse_json(
            self.request(reqwest::Method::POST, path)
                .multipart(upload_form(paths)?)
                .send()?,
        )
    }
    fn import(&self, preview: Value, dry_run: bool) -> Result<Value> {
        if dry_run {
            return Ok(preview);
        }
        let id = preview["id"].as_str().context("Import preview has no id")?;
        let candidates = preview["candidates"]
            .as_array()
            .context("Import preview has no candidates")?;
        if candidates.is_empty() {
            bail!("No MDX dictionaries were found in the import; use --dry-run to inspect files")
        }
        let selections: Vec<Value> = candidates
            .iter()
            .map(|c| json!({"mdx":c["mdx"],"name":c["name"],"resources":c["resources"]}))
            .collect();
        self.json(
            reqwest::Method::POST,
            &format!("/imports/{}/commit", segment(id)),
            Some(&json!({"dictionaries":selections})),
        )
    }
    fn resources(&self, command: ResourceCommand) -> Result<Value> {
        use ResourceCommand::*;
        match command {
            List { dictionary } => Ok(self.dictionary(&dictionary)?["resources"].clone()),
            Add { dictionary, paths } => self.upload(
                &format!("/dictionaries/{}/resources", segment(&dictionary)),
                &paths,
            ),
            Entries {
                dictionary,
                resource,
                prefix,
                limit,
            } => parse_json(
                self.request(
                    reqwest::Method::GET,
                    &format!(
                        "/dictionaries/{}/resources/{}/entries",
                        segment(&dictionary),
                        segment(&resource)
                    ),
                )
                .query(&[("prefix", prefix), ("limit", limit.to_string())])
                .send()?,
            ),
            Get {
                dictionary,
                resource,
                path,
                output,
            } => {
                let response = checked(
                    self.request(
                        reqwest::Method::GET,
                        &format!(
                            "/dictionaries/{}/resources/{}/content",
                            segment(&dictionary),
                            segment(&resource)
                        ),
                    )
                    .query(&[("path", path)])
                    .send()?,
                )?;
                save_response(response, &output)?;
                Ok(Value::Null)
            }
            command => {
                let dictionary = match &command {
                    Enable { dictionary, .. }
                    | Disable { dictionary, .. }
                    | Remove { dictionary, .. }
                    | Key { dictionary, .. }
                    | Order { dictionary, .. } => dictionary,
                    _ => unreachable!(),
                };
                let mut resources = self.dictionary(dictionary)?["resources"]
                    .as_array()
                    .context("Missing resource list")?
                    .clone();
                match &command {
                    Order { ids, .. } => {
                        if ids.len() != resources.len() {
                            bail!("Order must contain every resource ID exactly once")
                        }
                        let mut ordered = Vec::with_capacity(resources.len());
                        for (order, id) in ids.iter().enumerate() {
                            let index = resources
                                .iter()
                                .position(|r| r["id"].as_str() == Some(id))
                                .with_context(|| format!("Unknown or repeated resource: {id}"))?;
                            let mut resource = resources.remove(index);
                            resource["order"] = json!(order);
                            ordered.push(resource);
                        }
                        resources = ordered;
                    }
                    Enable { resource, .. }
                    | Disable { resource, .. }
                    | Remove { resource, .. }
                    | Key { resource, .. } => {
                        let index = resources
                            .iter()
                            .position(|r| r["id"].as_str() == Some(resource))
                            .with_context(|| format!("Unknown resource: {resource}"))?;
                        match &command {
                            Enable { .. } => resources[index]["enabled"] = json!(true),
                            Disable { .. } => resources[index]["enabled"] = json!(false),
                            Remove { yes, .. } => {
                                require_yes(*yes)?;
                                resources.remove(index);
                            }
                            Key { key, .. } => resources[index]["key"] = json!(key),
                            _ => unreachable!(),
                        }
                    }
                    _ => unreachable!(),
                }
                self.json(
                    reqwest::Method::PUT,
                    &format!("/dictionaries/{}", segment(dictionary)),
                    Some(&json!({"resources":resources})),
                )
            }
        }
    }
    fn maintenance(&self, scope: Maintenance, action: &str) -> Result<Value> {
        let response=self.json(reqwest::Method::POST,"/maintenance",Some(&json!({"target":scope.target.name(),"action":action,"dictionary_id":scope.dictionary})))?;
        let Some(job_id) = response["job_id"].as_str() else {
            return Ok(response);
        };
        if !scope.wait {
            return Ok(response);
        }
        let started = Instant::now();
        loop {
            let bootstrap = self.bootstrap()?;
            let jobs = bootstrap["stats"]["jobs"]
                .as_array()
                .context("Missing maintenance jobs")?;
            let job = jobs
                .iter()
                .find(|j| j["id"].as_str() == Some(job_id))
                .context("Maintenance job disappeared before completion")?;
            match job["state"].as_str() {
                Some("done") => return Ok(job.clone()),
                Some("error") => bail!("Maintenance failed: {}", job["error"]),
                _ => {}
            }
            if started.elapsed() > Duration::from_secs(scope.wait_timeout) {
                bail!("Timed out waiting for job {job_id}; use index status to inspect it")
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

fn segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
fn require_yes(yes: bool) -> Result<()> {
    anyhow::ensure!(yes, "Destructive operation requires --yes");
    Ok(())
}
fn checked(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if let Ok(body) = serde_json::from_str::<Value>(&text) {
        if let Some(message) = body["error"]["message"].as_str() {
            bail!("HTTP {status}: {message}")
        }
    }
    bail!(
        "HTTP {status}: {}",
        text.chars().take(2048).collect::<String>()
    )
}
fn parse_json(response: Response) -> Result<Value> {
    let response = checked(response)?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(json!({"ok":true}));
    }
    let bytes = response.bytes()?;
    if bytes.is_empty() {
        return Ok(json!({"ok":true}));
    }
    serde_json::from_slice(&bytes).context("Service returned invalid JSON")
}
fn read_json(path: &Path) -> Result<Value> {
    let mut contents = String::new();
    if path == Path::new("-") {
        io::stdin().read_to_string(&mut contents)?;
    } else {
        contents =
            fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    }
    serde_json::from_str(&contents).context("Invalid JSON input")
}
fn print_json(value: &Value) -> Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, value)?;
    writeln!(output)?;
    Ok(())
}
fn save_response(mut response: Response, path: &Path) -> Result<()> {
    if path == Path::new("-") {
        io::copy(&mut response, &mut io::stdout().lock())?;
    } else {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        io::copy(&mut response, &mut temporary)?;
        temporary.flush()?;
        temporary.persist(path).map_err(|e| e.error)?;
    }
    Ok(())
}
fn upload_form(paths: &[PathBuf]) -> Result<multipart::Form> {
    let mut form = multipart::Form::new();
    let mut count = 0usize;
    for path in paths {
        let metadata =
            fs::symlink_metadata(path).with_context(|| format!("Reading {}", path.display()))?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "Symlink uploads are not allowed: {}",
            path.display()
        );
        if metadata.is_file() {
            let filename = path
                .file_name()
                .context("File has no name")?
                .to_str()
                .context("Upload filenames must be UTF-8")?
                .to_owned();
            form = form.part("files", multipart::Part::file(path)?.file_name(filename));
            count += 1;
        } else if metadata.is_dir() {
            let canonical = path.canonicalize()?;
            let parent = canonical
                .parent()
                .context("Cannot upload filesystem root")?;
            for entry in walkdir::WalkDir::new(&canonical)
                .follow_links(false)
                .sort_by_file_name()
            {
                let entry = entry?;
                anyhow::ensure!(
                    !entry.file_type().is_symlink(),
                    "Symlink uploads are not allowed: {}",
                    entry.path().display()
                );
                if entry.file_type().is_file() {
                    let filename = entry
                        .path()
                        .strip_prefix(parent)?
                        .to_str()
                        .context("Upload filenames must be UTF-8")?
                        .replace('\\', "/");
                    form = form.part(
                        "files",
                        multipart::Part::file(entry.path())?.file_name(filename),
                    );
                    count += 1;
                }
            }
        } else {
            bail!("Not a regular file or directory: {}", path.display())
        }
    }
    anyhow::ensure!(count > 0, "No files to upload");
    Ok(form)
}

fn default_data_dir(root: &Path) -> PathBuf {
    let current = root.join(".fastdict");
    let legacy = root.join(".mydict");
    if !current.exists() && legacy.is_dir() { legacy } else { current }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn existing_installation_is_used_until_a_new_data_directory_exists() -> Result<()> {
        let root = tempfile::tempdir()?;
        assert_eq!(default_data_dir(root.path()), root.path().join(".fastdict"));
        fs::create_dir(root.path().join(".mydict"))?;
        assert_eq!(default_data_dir(root.path()), root.path().join(".mydict"));
        fs::create_dir(root.path().join(".fastdict"))?;
        assert_eq!(default_data_dir(root.path()), root.path().join(".fastdict"));
        Ok(())
    }
}

/// Synchronous entry point: blocking HTTP clients are never created inside Tokio.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let token = match cli.token_file {
        Some(path) => Some(fs::read_to_string(path)?.trim().to_owned()),
        None => std::env::var("FASTDICT_TOKEN").ok().filter(|v| !v.is_empty()),
    };
    if let Command::Serve {
        listen,
        data_dir,
        import_root,
        static_dir,
    } = cli.command
    {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "fastdict=info,tower_http=info".into()),
            )
            .with_writer(io::stderr)
            .init();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return runtime.block_on(crate::api::serve(crate::api::ServeConfig {
            listen,
            data_dir: data_dir.unwrap_or_else(|| default_data_dir(Path::new("."))),
            token,
            import_roots: import_root,
            static_dir,
        }));
    }
    let mut base = cli.endpoint.trim_end_matches('/').to_owned();
    let endpoint = url::Url::parse(&base).context("Invalid FASTDICT_ENDPOINT / --endpoint")?;
    anyhow::ensure!(
        matches!(endpoint.scheme(), "http" | "https")
            && endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none(),
        "Endpoint must be an HTTP(S) URL without credentials, query or fragment"
    );
    if !base.ends_with("/api/v1") {
        base.push_str("/api/v1")
    }
    let api = Api {
        client: Client::builder()
            .timeout(Duration::from_secs(cli.timeout))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        base,
        token,
    };
    const GET: reqwest::Method = reqwest::Method::GET;
    const POST: reqwest::Method = reqwest::Method::POST;
    const PUT: reqwest::Method = reqwest::Method::PUT;
    const DELETE: reqwest::Method = reqwest::Method::DELETE;
    let output = match cli.command {
        Command::Serve { .. } => unreachable!(),
        Command::Suggest {
            query,
            group,
            limit,
            mode,
        } => parse_json(
            api.request(GET, "/suggest")
                .query(&[
                    ("q", query),
                    ("group", group),
                    ("limit", limit.to_string()),
                    (
                        "mode",
                        match mode {
                            SearchMode::Headword => "headword",
                            SearchMode::Fulltext => "fulltext",
                        }
                        .into(),
                    ),
                ])
                .send()?,
        )?,
        Command::Lookup {
            query,
            dictionary,
            group,
            format,
        } => {
            let mut failures = Vec::new();
            let response = if let Some(dictionary) = dictionary {
                parse_json(
                    api.request(GET, "/lookup")
                        .query(&[("q", &query), ("dictionary", &dictionary)])
                        .send()?,
                )?
            } else {
                use unicode_normalization::UnicodeNormalization;
                let group = group.unwrap_or_else(|| "all".to_owned());
                let suggestions = parse_json(
                    api.request(GET, "/suggest")
                        .query(&[
                            ("q", query.as_str()),
                            ("group", group.as_str()),
                            ("limit", "100"),
                        ])
                        .send()?,
                )?;
                let normalized = query
                    .nfc()
                    .collect::<String>()
                    .to_lowercase()
                    .trim()
                    .to_owned();
                let mut matches = std::collections::HashSet::new();
                for item in suggestions["items"]
                    .as_array()
                    .context("Missing suggestions")?
                {
                    let word = item["word"].as_str().unwrap_or("");
                    if item["match_kind"] == "exact"
                        && word.nfc().collect::<String>().to_lowercase().trim() == normalized
                    {
                        for id in item["dictionary_ids"]
                            .as_array()
                            .context("Missing dictionary IDs")?
                        {
                            if let Some(id) = id.as_str() {
                                matches.insert(id.to_owned());
                            }
                        }
                    }
                }
                let bootstrap = api.bootstrap()?;
                let mut dictionaries: Vec<&Value> = bootstrap["dictionaries"]
                    .as_array()
                    .context("Missing dictionaries")?
                    .iter()
                    .filter(|dictionary| {
                        dictionary["id"]
                            .as_str()
                            .is_some_and(|id| matches.contains(id))
                    })
                    .collect();
                if group != "all" {
                    let definition = bootstrap["groups"]
                        .as_array()
                        .context("Missing groups")?
                        .iter()
                        .find(|item| item["id"].as_str() == Some(group.as_str()))
                        .context("Unknown group")?;
                    let order = definition["dictionary_ids"]
                        .as_array()
                        .context("Missing group order")?;
                    dictionaries.sort_by_key(|dictionary| {
                        order
                            .iter()
                            .position(|id| id == &dictionary["id"])
                            .unwrap_or(usize::MAX)
                    });
                    if let Some(rules) = definition["sort"].as_array() {
                        dictionaries.sort_by(|left, right| {
                            for rule in rules {
                                let comparison = match rule["field"].as_str() {
                                    Some("entries") => {
                                        left["entries"].as_u64().cmp(&right["entries"].as_u64())
                                    }
                                    Some("created") => left["created"]
                                        .as_str()
                                        .unwrap_or("")
                                        .cmp(right["created"].as_str().unwrap_or("")),
                                    _ => {
                                        left["name"].as_str().unwrap_or("").to_lowercase().cmp(
                                            &right["name"].as_str().unwrap_or("").to_lowercase(),
                                        )
                                    }
                                };
                                if !comparison.is_eq() {
                                    return if rule["direction"] == "desc" {
                                        comparison.reverse()
                                    } else {
                                        comparison
                                    };
                                }
                            }
                            std::cmp::Ordering::Equal
                        });
                    }
                }
                let mut results = Vec::new();
                for dictionary in dictionaries {
                    let id = dictionary["id"].as_str().context("Missing dictionary ID")?;
                    match api
                        .request(GET, "/lookup")
                        .query(&[("q", query.as_str()), ("dictionary", id)])
                        .send()
                        .map_err(anyhow::Error::from)
                        .and_then(parse_json)
                    {
                        Ok(mut result) => {
                            result["dictionary_name"] = dictionary["name"].clone();
                            results.push(result);
                        }
                        Err(error) => {
                            failures.push(json!({"dictionary_id":id,"message":error.to_string()}))
                        }
                    }
                }
                json!({"word":query,"group_id":group,"results":results,"errors":failures})
            };
            match format {
                OutputFormat::Json => {
                    if !failures.is_empty() {
                        print_json(&response)?;
                        bail!(
                            "{} dictionary lookup(s) failed; structured partial results were written to stdout",
                            failures.len()
                        );
                    }
                    response
                }
                OutputFormat::Text | OutputFormat::Html => {
                    let mut stdout = io::stdout().lock();
                    let single = [&response];
                    let responses: Vec<&Value> = response["results"]
                        .as_array()
                        .map(|items| items.iter().collect())
                        .unwrap_or_else(|| single.into_iter().collect());
                    for result in responses {
                        if let Some(name) = result["dictionary_name"].as_str() {
                            writeln!(stdout, "{name}\\n")?;
                        }
                        for article in result["articles"]
                            .as_array()
                            .context("Missing lookup articles")?
                        {
                            let html = article["html"].as_str().context("Article missing HTML")?;
                            match format {
                                OutputFormat::Text => {
                                    writeln!(stdout, "{}", crate::render::plain_text(html))?
                                }
                                _ => writeln!(stdout, "{html}")?,
                            }
                        }
                    }
                    drop(stdout);
                    if !failures.is_empty() {
                        bail!("{} dictionary lookup(s) failed", failures.len());
                    }
                    return Ok(());
                }
            }
        }
        Command::Dictionaries(command) => match command {
            DictionaryCommand::List => api.bootstrap()?["dictionaries"].clone(),
            DictionaryCommand::Info { id } => api.dictionary(&id)?,
            DictionaryCommand::Import { paths, dry_run } => {
                api.import(api.upload("/imports", &paths)?, dry_run)?
            }
            DictionaryCommand::ImportPath { path, dry_run } => api.import(
                api.json(POST, "/imports/path", Some(&json!({"path":path})))?,
                dry_run,
            )?,
            DictionaryCommand::Commit { preview_id, file } => api.json(
                POST,
                &format!("/imports/{}/commit", segment(&preview_id)),
                Some(&read_json(&file)?),
            )?,
            DictionaryCommand::Enable { id } => api.json(
                PUT,
                &format!("/dictionaries/{}", segment(&id)),
                Some(&json!({"enabled":true})),
            )?,
            DictionaryCommand::Disable { id } => api.json(
                PUT,
                &format!("/dictionaries/{}", segment(&id)),
                Some(&json!({"enabled":false})),
            )?,
            DictionaryCommand::Update { id, file } => api.json(
                PUT,
                &format!("/dictionaries/{}", segment(&id)),
                Some(&read_json(&file)?),
            )?,
            DictionaryCommand::Remove { id, yes } => {
                require_yes(yes)?;
                api.json(DELETE, &format!("/dictionaries/{}", segment(&id)), None)?
            }
            DictionaryCommand::Resources(command) => {
                let result = api.resources(command)?;
                if result.is_null() {
                    return Ok(());
                }
                result
            }
        },
        Command::Groups(command) => match command {
            GroupCommand::List => api.bootstrap()?["groups"].clone(),
            GroupCommand::Create { name, dictionaries } => api.json(
                POST,
                "/groups",
                Some(&json!({"name":name,"dictionary_ids":dictionaries})),
            )?,
            GroupCommand::Update { id, file } => api.json(
                PUT,
                &format!("/groups/{}", segment(&id)),
                Some(&read_json(&file)?),
            )?,
            GroupCommand::Order { id, ids, empty } => {
                anyhow::ensure!(
                    empty == ids.is_empty(),
                    "Supply dictionary IDs or --empty, not both"
                );
                api.json(
                    PUT,
                    &format!("/groups/{}", segment(&id)),
                    Some(&json!({"dictionary_ids":ids,"sort":[]})),
                )?
            }
            GroupCommand::Remove { id, yes } => {
                require_yes(yes)?;
                api.json(DELETE, &format!("/groups/{}", segment(&id)), None)?
            }
        },
        Command::Settings(command) => match command {
            SettingsCommand::Get => api.bootstrap()?["settings"].clone(),
            SettingsCommand::Set { file } => {
                api.json(PUT, "/settings", Some(&read_json(&file)?))?
            }
        },
        Command::History(command) => match command {
            HistoryCommand::List => api.bootstrap()?["history"].clone(),
            HistoryCommand::Add { word, group } => api.json(
                POST,
                "/history",
                Some(&json!({"word":word,"group_id":group})),
            )?,
            HistoryCommand::Remove { id, yes } => {
                require_yes(yes)?;
                api.json(DELETE, &format!("/history/{}", segment(&id)), None)?
            }
            HistoryCommand::Clear { yes } => {
                require_yes(yes)?;
                api.json(DELETE, "/history", None)?
            }
        },
        Command::Logs(command) => match command {
            LogsCommand::List { limit } => parse_json(
                api.request(GET, "/logs")
                    .query(&[("limit", limit)])
                    .send()?,
            )?,
            LogsCommand::Clear { yes } => {
                require_yes(yes)?;
                api.json(DELETE, "/logs", None)?
            }
        },
        Command::Index(command) => match command {
            IndexCommand::Status { dictionary } => {
                let bootstrap = api.bootstrap()?;
                let dictionaries = bootstrap["dictionaries"]
                    .as_array()
                    .context("Missing dictionaries")?;
                if let Some(id) = &dictionary {
                    anyhow::ensure!(
                        dictionaries.iter().any(|d| d["id"].as_str() == Some(id)),
                        "Unknown dictionary: {id}"
                    )
                };
                json!({"dictionaries":dictionaries.iter().filter(|d|dictionary.as_ref().is_none_or(|id|d["id"].as_str()==Some(id))).map(|d|json!({"id":d["id"],"name":d["name"],"index_status":d["index_status"]})).collect::<Vec<_>>(),"jobs":bootstrap["stats"]["jobs"]})
            }
            IndexCommand::Clear { scope, yes } => {
                require_yes(yes)?;
                api.maintenance(scope, "clear")?
            }
            IndexCommand::Rebuild { scope } => api.maintenance(scope, "rebuild")?,
        },
        Command::Backup {
            file,
            include_files,
        } => {
            let response = checked(
                api.request(GET, "/backup")
                    .query(&[("include_files", include_files)])
                    .send()?,
            )?;
            save_response(response, &file)?;
            return Ok(());
        }
        Command::Restore { file, yes } => {
            require_yes(yes)?;
            parse_json(
                api.request(POST, "/restore")
                    .multipart(multipart::Form::new().part("file", multipart::Part::file(file)?))
                    .send()?,
            )?
        }
    };
    print_json(&output)
}
