use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};
pub const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 20000;
#[derive(Clone, Serialize, Deserialize)]
pub struct ImportFile {
    pub path: String,
    pub size: u64,
    pub kind: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub mdx: String,
    pub name: String,
    pub resources: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub id: String,
    pub files: Vec<ImportFile>,
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
}
pub fn safe_path(input: &str) -> Result<PathBuf> {
    if input.is_empty()
        || input.len() > 1024
        || input.contains('\0')
        || input.contains('\\')
        || input.contains(':')
    {
        bail!("Invalid relative path");
    }
    let p = Path::new(input);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("Path traversal is not allowed");
    }
    Ok(p.to_owned())
}
pub fn kind(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mdx" => "mdx",
        "mdd" => "mdd",
        "css" => "css",
        "js" => "js",
        "woff" | "woff2" | "ttf" | "otf" => "font",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => "image",
        "mp3" | "wav" | "ogg" | "m4a" => "audio",
        _ => "file",
    }
    .into()
}
pub fn inspect(root: &Path, id: String) -> Result<ImportPreview> {
    let files = list(root)?;
    let mdx: Vec<_> = files.iter().filter(|f| f.kind == "mdx").collect();
    let mut candidates = Vec::new();
    for file in &mdx {
        let path = Path::new(&file.path);
        let parent = path.parent().unwrap_or(Path::new(""));
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let mut warnings = Vec::new();
        let peers = mdx
            .iter()
            .filter(|f| Path::new(&f.path).parent() == Some(parent))
            .count();
        let resources: Vec<String> = files
            .iter()
            .filter(|f| f.kind != "mdx" && Path::new(&f.path).starts_with(parent))
            .filter(|f| {
                if peers == 1 {
                    return true;
                }
                let name = Path::new(&f.path)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                name == stem
                    || name.starts_with(&format!("{stem}."))
                    || Path::new(&f.path).starts_with(parent.join(&stem))
            })
            .map(|f| f.path.clone())
            .collect();
        if peers > 1 {
            warnings.push("同目录存在多个词典，请确认共享资源关联".into());
        }
        let mut volumes = HashSet::new();
        for resource in &resources {
            let p = Path::new(resource);
            if kind(resource) == "mdd" {
                let s = p.file_stem().unwrap_or_default().to_string_lossy();
                if s == stem {
                    volumes.insert(0usize);
                } else if let Some(n) = s
                    .strip_prefix(&format!("{stem}."))
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    volumes.insert(n);
                }
            }
        }
        if let Some(max) = volumes.iter().max() {
            let gaps: Vec<_> = (0..=(*max).min(10_000))
                .filter(|n| !volumes.contains(n))
                .map(|n| n.to_string())
                .collect();
            if !gaps.is_empty() {
                warnings.push(format!(
                    "MDD 分卷编号缺失：{}；已发现的分卷仍可显式挂载",
                    gaps.join(", ")
                ));
            }
        }
        candidates.push(Candidate {
            mdx: file.path.clone(),
            name: stem,
            resources,
            warnings,
        });
    }
    let warnings = if candidates.is_empty() {
        vec!["未发现 MDX 文件".into()]
    } else {
        vec![]
    };
    Ok(ImportPreview {
        id,
        files,
        candidates,
        warnings,
    })
}
pub fn list(root: &Path) -> Result<Vec<ImportFile>> {
    let mut files = Vec::new();
    let mut size = 0u64;
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!("Symlinks are not allowed");
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_str()
            .context("Paths must be UTF-8")?
            .to_owned();
        safe_path(&relative)?;
        let len = entry.metadata()?.len();
        size = size.checked_add(len).context("Import too large")?;
        if size > MAX_BYTES || files.len() >= MAX_FILES {
            bail!("Import exceeds size or file limit");
        }
        files.push(ImportFile {
            kind: kind(&relative),
            path: relative,
            size: len,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
pub fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    let files = list(source)?;
    std::fs::create_dir_all(dest)?;
    for file in files {
        let path = safe_path(&file.path)?;
        let target = dest.join(&path);
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::copy(source.join(path), target)?;
    }
    Ok(())
}
pub fn copy_authorized(source: &Path, roots: &[PathBuf], dest: &Path) -> Result<()> {
    let canonical = source.canonicalize()?;
    let allowed = roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .find(|r| canonical.starts_with(r))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Path is outside configured import roots",
            )
        })?;
    if !canonical.is_dir() {
        bail!("Import path must be a directory");
    }
    let directory = cap_std::fs::Dir::open_ambient_dir(&allowed, cap_std::ambient_authority())?;
    let relative_root = canonical.strip_prefix(&allowed)?;
    let files = list(&canonical)?;
    std::fs::create_dir_all(dest)?;
    for file in files {
        let relative = safe_path(&file.path)?;
        let mut source = directory.open(relative_root.join(&relative))?;
        if !source.metadata()?.is_file() {
            bail!("Import source is not a regular file");
        }
        let target = dest.join(relative);
        std::fs::create_dir_all(target.parent().unwrap())?;
        let mut output = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target)?;
        let copied = std::io::copy(
            &mut std::io::Read::by_ref(&mut source).take(file.size + 1),
            &mut output,
        )?;
        if copied != file.size {
            bail!("Import source changed while copying");
        }
    }
    Ok(())
}
