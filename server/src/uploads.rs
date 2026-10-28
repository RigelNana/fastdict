//! Bounded, resumable transport for files that exceed a reverse proxy's request limit.
//! Callers serialize filesystem operations; only completed files enter import staging.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, fs, io::{Read, Seek, SeekFrom, Write}, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};
use crate::imports;

pub const CHUNK_BYTES: usize = 8 * 1024 * 1024;
const SESSION_TTL: u64 = 24 * 60 * 60;
const MAX_SESSIONS: usize = 8;

#[derive(Clone, Serialize, Deserialize)]
pub struct FileSpec { pub path: String, pub size: u64 }
#[derive(Clone, Serialize, Deserialize)]
pub struct UploadSpec {
    pub kind: String,
    pub dictionary_id: Option<String>,
    pub files: Vec<FileSpec>,
}
#[derive(Serialize, Deserialize)]
struct Manifest { spec: UploadSpec, touched: u64, prepared: bool, result: Option<Value> }
#[derive(Serialize)]
pub struct Status {
    pub id: String,
    pub chunk_size: usize,
    pub received: Vec<u64>,
    pub result: Option<Value>,
}
pub struct Prepared { pub spec: UploadSpec, pub stage: PathBuf, pub result: Option<Value> }
pub fn target(data: &Path, id: &str) -> Result<(UploadSpec, Option<Value>)> {
    let manifest = load(&directory(data, id)?)?;
    Ok((manifest.spec, manifest.result))
}
fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }
fn directory(data: &Path, id: &str) -> Result<PathBuf> {
    ensure!(uuid::Uuid::parse_str(id).is_ok(), "Invalid upload id");
    Ok(data.join("uploads").join(id))
}
fn load(root: &Path) -> Result<Manifest> {
    let bytes = fs::read(root.join("manifest.json"))?;
    ensure!(bytes.len() <= 2 * 1024 * 1024, "Invalid upload manifest");
    Ok(serde_json::from_slice(&bytes)?)
}
fn save(root: &Path, manifest: &Manifest) -> Result<()> {
    let mut file = fs::File::create(root.join("manifest.tmp"))?;
    file.write_all(&serde_json::to_vec(manifest)?)?;
    file.sync_data()?;
    fs::rename(root.join("manifest.tmp"), root.join("manifest.json"))?;
    Ok(())
}
fn cleanup(data: &Path) -> Result<usize> {
    let root = data.join("uploads");fs::create_dir_all(&root)?;
    let mut active = 0;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() { continue; }
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if uuid::Uuid::parse_str(&id).is_err() { continue; }
        let Ok(manifest) = load(&entry.path()) else {
            if entry.metadata()?.modified()?.elapsed().unwrap_or_default().as_secs() > SESSION_TTL {
                fs::remove_dir_all(entry.path())?;
            } else { active += 1; }
            continue;
        };
        if now().saturating_sub(manifest.touched) > SESSION_TTL {
            // An import preview already belongs to the normal import lifecycle.
            // Do not remove a prepared stage here.
            fs::remove_dir_all(entry.path())?;
        } else if manifest.result.is_none() { active += 1; }
    }
    Ok(active)
}
pub fn create(data: &Path, spec: UploadSpec) -> Result<Status> {
    ensure!(matches!(spec.kind.as_str(), "import" | "resources"), "Unsupported upload target");
    ensure!(!spec.files.is_empty() && spec.files.len() <= imports::MAX_FILES, "Invalid upload file count");
    ensure!((spec.kind == "resources") == spec.dictionary_id.is_some(), "Invalid upload target");
    let mut total = 0u64;let mut names = HashSet::new();
    for file in &spec.files {
        imports::safe_path(&file.path)?;
        ensure!(names.insert(file.path.clone()), "Duplicate upload path");
        total = total.checked_add(file.size).context("Upload too large")?;
        ensure!(total <= imports::MAX_BYTES, "本次上传总大小不能超过 2 GiB");
        if spec.kind == "resources" { ensure!(imports::kind(&file.path) != "mdx", "Cannot append MDX as a resource"); }
    }
    for file in &spec.files {
        let mut path = Path::new(&file.path).parent();
        while let Some(parent) = path {
            ensure!(!names.contains(parent.to_string_lossy().as_ref()), "Conflicting file paths");
            path = parent.parent();
        }
    }
    ensure!(cleanup(data)? < MAX_SESSIONS, "已有太多上传任务，请稍后重试");
    let id = crate::model::id();let root = directory(data, &id)?;
    let result = (|| {
        fs::create_dir_all(root.join("files"))?;
        for file in &spec.files {
            let path = root.join("files").join(imports::safe_path(&file.path)?);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::OpenOptions::new().create_new(true).write(true).open(path)?;
        }
        save(&root, &Manifest { spec, touched: now(), prepared: false, result: None })?;
        status(data, &id)
    })();
    if result.is_err() { let _ = fs::remove_dir_all(&root); }
    result
}
pub fn status(data: &Path, id: &str) -> Result<Status> {
    let root = directory(data, id)?;let manifest = load(&root)?;
    let files = if manifest.prepared { data.join("staging").join(id) } else { root.join("files") };
    let received = manifest.spec.files.iter().map(|file| {
        if manifest.result.is_some() { file.size }
        else { fs::metadata(files.join(&file.path)).map(|m| m.len()).unwrap_or(0) }
    }).collect();
    Ok(Status { id: id.to_owned(), chunk_size: CHUNK_BYTES, received, result: manifest.result })
}
pub fn append(data: &Path, id: &str, index: usize, offset: u64, bytes: &[u8]) -> Result<u64> {
    ensure!(!bytes.is_empty() && bytes.len() <= CHUNK_BYTES, "Invalid upload chunk size");
    let root = directory(data, id)?;let mut manifest = load(&root)?;
    ensure!(!manifest.prepared && manifest.result.is_none(), "Upload is already being processed");
    let spec = manifest.spec.files.get(index).context("Unknown upload file")?;
    let end = offset.checked_add(bytes.len() as u64).context("Invalid chunk offset")?;
    ensure!(end <= spec.size, "Chunk exceeds the declared file size");
    ensure!(offset % CHUNK_BYTES as u64 == 0 && bytes.len() as u64 == (spec.size - offset).min(CHUNK_BYTES as u64), "Invalid upload chunk boundary");
    let mut file = fs::OpenOptions::new().read(true).write(true).open(root.join("files").join(imports::safe_path(&spec.path)?))?;
    let received = file.metadata()?.len();
    ensure!(offset <= received, "Upload chunk is out of order");
    file.seek(SeekFrom::Start(offset))?;
    if end <= received {
        let mut existing = vec![0; bytes.len()];file.read_exact(&mut existing)?;
        ensure!(existing == bytes, "Retried upload chunk has different content");
    } else {
        // A hard restart may have left only part of this fixed-size block.
        file.set_len(offset)?;
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_data()) {
            file.set_len(offset)?;
            return Err(error.into());
        }
    }
    manifest.touched = now();save(&root, &manifest)?;
    Ok(received.max(end))
}
pub fn prepare(data: &Path, id: &str) -> Result<Prepared> {
    let root = directory(data, id)?;let mut manifest = load(&root)?;
    let stage = data.join("staging").join(id);
    if manifest.result.is_some() { return Ok(Prepared { spec: manifest.spec, stage, result: manifest.result }); }
    let source = if stage.is_dir() { stage.clone() } else { root.join("files") };
    for file in &manifest.spec.files {
        ensure!(fs::metadata(source.join(imports::safe_path(&file.path)?))?.len() == file.size, "文件尚未上传完成");
    }
    // Mark immutable before publication; a retry can finish a prepared rename.
    manifest.prepared = true;manifest.touched = now();save(&root, &manifest)?;
    if source != stage {
        fs::create_dir_all(stage.parent().unwrap())?;
        fs::rename(source, &stage)?;
    }
    Ok(Prepared { spec: manifest.spec, stage, result: None })
}
pub fn finish(data: &Path, id: &str, result: Value) -> Result<()> {
    let root = directory(data, id)?;let mut manifest = load(&root)?;
    manifest.result = Some(result);manifest.touched = now();save(&root, &manifest)
}
pub fn discard(data: &Path, id: &str) -> Result<()> {
    let root = directory(data, id)?;
    if !root.exists() { return Ok(()); }
    let manifest = load(&root)?;
    // Successful import previews remain available for the user's subsequent commit.
    if manifest.prepared && manifest.result.is_none() {
        let stage = data.join("staging").join(id);
        if stage.exists() { fs::remove_dir_all(stage)?; }
    }
    fs::remove_dir_all(root)?;Ok(())
}
