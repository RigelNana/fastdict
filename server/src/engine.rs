//! Synchronous dictionary boundary for immutable, service-managed revisions.
//!
//! Files passed to `open` must never be modified or truncated while an engine or
//! its returned byte views exist. The service publishes new revision directories
//! instead of editing mapped sources. Virtual resources expose explicit mounts
//! only; an article cannot turn an arbitrary server path into a filesystem read.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use wodix::{Limits, Mdx, Read, Reader, Resources, Version};

pub use wodix::ResourceMount;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineMetadata {
    pub title: String,
    pub description: String,
    pub format_version: String,
    pub encoding: String,
    pub encrypted: u32,
    pub key_blocks: usize,
    pub record_blocks: usize,
    pub attributes: BTreeMap<String, String>,
    pub created: Option<String>,
    pub dictionary_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineArticle {
    pub html: String,
    pub source_ids: Vec<usize>,
    pub resolved_ids: Vec<usize>,
    pub redirects: Vec<String>,
}

pub struct Engine {
    dictionary: Mdx,
    resources: Resources,
}

impl Engine {
    /// Open a service-owned immutable revision. Relative paths are root-relative.
    ///
    /// The root and all source bindings must remain immutable for this engine's
    /// lifetime. Containment is checked before mapping, and resource volumes are
    /// checked again at their independent lazy-open boundary. The cache budget
    /// is divided between the MDX and every explicitly mounted MDD, so their sum
    /// cannot exceed `cache_bytes`. Caller-retained byte views are not included.
    pub fn open(
        mdx_path: &Path,
        root: &Path,
        mounts: &[ResourceMount],
        cache_bytes: usize,
    ) -> Result<Self> {
        let root = root.canonicalize().context("无法打开托管词典目录")?;
        ensure!(root.is_dir(), "托管词典根路径不是目录");
        let relative = if mdx_path.is_absolute() {
            mdx_path
                .strip_prefix(&root)
                .context("词典路径不在托管目录内")?
        } else {
            mdx_path
        };
        ensure!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_) | Component::CurDir)),
            "词典路径不能越出托管目录"
        );
        let source = root
            .join(relative)
            .canonicalize()
            .context("无法打开词典文件")?;
        ensure!(
            source.starts_with(&root) && source.is_file(),
            "词典文件必须位于托管目录内"
        );
        let volume_count = mounts
            .iter()
            .filter(|mount| {
                mount
                    .path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("mdd"))
            })
            .count();
        let mut limits = Limits::default();
        limits.cache = cache_bytes / volume_count.saturating_add(1);
        // SAFETY: The service owns immutable revision files and never rewrites
        // published paths; the checked canonical source belongs to that revision.
        let reader = unsafe { Reader::open_with(&source, limits) }.context("无法解析 MDX 词典")?;
        let dictionary = Mdx::new(reader).context("无法建立词头索引")?;
        // SAFETY: The same managed immutable revision contract covers each mount,
        // including paths opened lazily. Resources validates root containment.
        let resources = unsafe { Resources::mount_with(&root, mounts, limits) }
            .context("无效的词典资源绑定")?;
        Ok(Self {
            dictionary,
            resources,
        })
    }

    pub fn metadata(&self) -> EngineMetadata {
        let reader = self.dictionary.reader();
        let metadata = reader.metadata();
        let attribute = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| {
                    metadata
                        .attributes
                        .get(*name)
                        .filter(|value| !value.trim().is_empty())
                })
                .cloned()
        };
        EngineMetadata {
            title: metadata.title.clone(),
            description: metadata.description.clone(),
            format_version: metadata
                .attributes
                .get("GeneratedByEngineVersion")
                .cloned()
                .unwrap_or_else(|| match metadata.version {
                    Version::V1 => "1.x".to_owned(),
                    Version::V2 => "2.x".to_owned(),
                }),
            encoding: metadata.encoding.as_ref().to_owned(),
            encrypted: metadata.encrypted.0,
            key_blocks: reader.key_block_count(),
            record_blocks: reader.record_block_count(),
            attributes: metadata.attributes.clone(),
            created: attribute(&["CreationDate", "CreatedDate", "CreationTime", "Created"]),
            dictionary_version: attribute(&["DictionaryVersion", "DictVersion", "Version"]),
        }
    }

    /// Source-order headwords, without touching any article payload blocks.
    pub fn entries(&self) -> &[wodix::Entry] {
        self.dictionary.reader().entries()
    }

    pub fn lookup(&self, word: &str) -> Result<Vec<EngineArticle>> {
        Ok(self
            .dictionary
            .lookup_detailed(word)?
            .into_iter()
            .map(|article| EngineArticle {
                html: article.html,
                source_ids: article.source_ids,
                resolved_ids: article.resolved_ids,
                redirects: article.redirects,
            })
            .collect())
    }

    /// Raw decoded record text for the explicit background full-text indexer.
    pub fn record_text(&self, id: usize) -> Result<String> {
        Ok(self.dictionary.reader().text(id)?)
    }

    pub fn resource(&self, path: &str) -> Result<wodix::Bytes> {
        Ok(self.resources.load(path)?)
    }

    pub fn resource_entries(
        &self,
        mount_path: &Path,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        Ok(self.resources.entries(mount_path, prefix, limit)?)
    }

    pub fn cache_bytes(&self) -> usize {
        self.dictionary
            .reader()
            .cache_stats()
            .resident
            .saturating_add(self.resources.cache_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_uses_real_sources_and_respects_zero_cache_budget() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Engine>();
        let revision = tempfile::tempdir().unwrap();
        std::fs::write(
            revision.path().join("dictionary.mdx"),
            include_bytes!("../../vendor/wodix/crates/wodix/tests/data/basic.mdx"),
        )
        .unwrap();
        std::fs::write(
            revision.path().join("arbitrary-volume.mdd"),
            include_bytes!("../../vendor/wodix/crates/wodix/tests/data/basic.mdd"),
        )
        .unwrap();
        let mounts = [ResourceMount {
            path: "arbitrary-volume.mdd".into(),
            key: String::new(),
        }];
        let engine =
            Engine::open(Path::new("dictionary.mdx"), revision.path(), &mounts, 0).unwrap();
        let metadata = engine.metadata();
        assert_eq!(
            metadata.format_version,
            metadata.attributes["GeneratedByEngineVersion"]
        );
        assert!(metadata.created.is_none());
        assert!(metadata.dictionary_version.is_none());
        assert!(metadata.key_blocks > 0);
        assert!(metadata.record_blocks > 0);
        assert_eq!(engine.entries().len(), 6);
        let articles = engine.lookup("apples").unwrap();
        assert_eq!(articles[0].html, "<p>A fruit.</p>");
        assert_eq!(articles[0].source_ids, vec![1]);
        assert_eq!(articles[0].resolved_ids, vec![0]);
        assert_eq!(articles[0].redirects, vec!["apple"]);
        assert_eq!(engine.record_text(1).unwrap(), "@@@LINK=apple");
        assert_eq!(
            engine.resource("hello.txt").unwrap().as_ref(),
            b"hello resource"
        );
        assert_eq!(
            engine
                .resource_entries(Path::new("arbitrary-volume.mdd"), "hello", 10)
                .unwrap(),
            vec![("/hello.txt".into(), 14)]
        );
        assert_eq!(engine.cache_bytes(), 0);
        assert!(Engine::open(Path::new("../dictionary.mdx"), revision.path(), &[], 0).is_err());
    }
}
