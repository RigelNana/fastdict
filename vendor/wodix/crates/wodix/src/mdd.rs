//! Read-only MDD sources and independent, lazy explicit resource mounts.

use std::io::Read as IoRead;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use memmap2::Mmap;

use crate::index::{Cache, Index, Lookup};
use crate::link::{Link, follow};
use crate::reader::{Read, Reader};
use crate::record_block::Bytes;
use crate::{Error, Limits, Result};

/// Resource-path normalization used for MDD key lookup.
///
/// Normalization is not filesystem path validation or a traversal sandbox.
pub struct Resource<'a> {
    /// Requested path before slash normalization and Unicode lowercasing.
    pub path: &'a str,
}

impl Resource<'_> {
    /// Return a lowercase backslash-separated key with one leading backslash.
    ///
    /// Removes an initial `.\` after slash conversion. This does not resolve
    /// parent-directory components or access the filesystem.
    pub fn key(&self) -> String {
        let path = self.path.replace('/', "\\");
        let path = path
            .strip_prefix(".\\")
            .unwrap_or(&path)
            .trim_start_matches('\\');
        let mut key = String::with_capacity(path.len() + 1);
        key.push('\\');
        for character in path.chars() {
            key.extend(character.to_lowercase());
        }
        key
    }
}

impl std::fmt::Debug for Resource<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Resource")
            .field("path_bytes", &self.path.len())
            .finish_non_exhaustive()
    }
}

/// A binary MDD resource source paired with its own immutable key index.
///
/// [`crate::Mdd::lookup`] returns raw resource bytes, not decoded article text.
/// The MDD header's text encoding describes keys, not arbitrary binary payloads.
pub struct Mdd<R = Reader<Mmap>, I = Index> {
    reader: R,
    index: I,
}

impl Mdd {
    /// Open a memory-mapped MDD with default [`crate::Limits`].
    ///
    /// Uses or creates a neighboring `.mdd.idx` index cache, including the
    /// persistent lock sidecar and publication effects of [`crate::Cache::open`].
    /// Record payloads remain lazily decoded.
    ///
    /// # Errors
    ///
    /// Returns source I/O, parsing or index-building errors. Cache I/O failures
    /// are warned about and fall back to an in-memory index.
    ///
    /// # Safety
    /// The source must remain unmodified and untruncated while mapped.
    /// Advisory locks alone do not establish this guarantee. Use
    /// [`crate::Mdd::parse`] with owned bytes when immutability is uncertain.
    pub unsafe fn open(path: &Path) -> Result<Self> {
        let limits = Limits::default();
        // SAFETY: The caller keeps the backing file immutable.
        unsafe { Self::open_with(path, limits) }
    }

    /// Open a memory-mapped MDD with explicit component budgets.
    ///
    /// Has the same cache-writing side effects as [`crate::Mdd::open`].
    ///
    /// # Errors
    ///
    /// Returns source, parsing or index-building errors, including exceeded
    /// budgets. See [`crate::Mdd::open`] for cache-failure behavior.
    ///
    /// # Safety
    /// The source must remain unmodified and untruncated while mapped.
    pub unsafe fn open_with(path: &Path, limits: Limits) -> Result<Self> {
        // SAFETY: The caller keeps the backing file immutable.
        let reader = unsafe { Reader::open_with(path, limits)? };
        let index = Cache::new(path.with_extension("mdd.idx"), reader.entries(), limits).open()?;
        Ok(Self { reader, index })
    }
}

impl<B: AsRef<[u8]>> Mdd<Reader<B>> {
    /// Parse resource bytes and build an index without filesystem I/O.
    ///
    /// Uses default budgets and `fallback` as a missing title. For custom
    /// budgets, construct a [`crate::Reader::with_limits`] reader and pass it to
    /// [`crate::Mdd::new`].
    ///
    /// # Errors
    ///
    /// Propagates parsing and index-building errors. Damaged record blocks can
    /// still fail when first accessed through [`crate::Mdd::lookup`].
    ///
    /// ```
    /// use wodix::Mdd;
    ///
    /// let data = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdd"));
    /// let resources = Mdd::parse(data.as_slice(), "fallback")?;
    /// assert_eq!(resources.lookup("HELLO.txt")?.as_ref(), b"hello resource");
    /// assert_eq!(resources.lookup("/style.css")?.as_ref(), b"body { color: black; }");
    /// assert!(matches!(resources.lookup("missing"), Err(wodix::Error::KeyNotFound(_))));
    /// # Ok::<(), wodix::Error>(())
    /// ```
    pub fn parse(data: B, fallback: &str) -> Result<Self> {
        let reader = Reader::parse(data, fallback)?;
        Self::new(reader)
    }
}

impl<R: Read, I: Lookup> Mdd<R, I> {
    /// Build a resource index from this exact reader and its budgets.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::Lookup::build`] failures.
    pub fn new(reader: R) -> Result<Self> {
        let index = I::build(reader.entries(), reader.limits())?;
        Ok(Self { reader, index })
    }

    /// Borrow the source for metadata and source-order raw record access.
    pub fn reader(&self) -> &R {
        &self.reader
    }

    /// Borrow the key index whose IDs belong only to this source.
    pub fn index(&self) -> &I {
        &self.index
    }

    /// Find the first matching resource and return its raw decompressed bytes.
    ///
    /// Converts slashes to backslashes, removes an initial `.\`, adds a leading
    /// backslash and lowercases the key before index matching. Duplicate keys
    /// select the first source-order ID. This performs no filesystem lookup,
    /// text decoding, trimming, style expansion or resource-redirect resolution.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::KeyNotFound`] for a miss, [`crate::Error::Limit`]
    /// for an oversized name or record, and propagates raw read failures.
    pub fn lookup(&self, path: &str) -> Result<Bytes> {
        Limits::check(
            "resource name",
            path.len() as u64,
            self.reader.limits().key_index,
        )?;
        let key = Resource { path }.key();
        let id = self
            .index
            .exact(&key)
            .first()
            .ok_or_else(|| Error::KeyNotFound(key.clone()))?;
        self.reader.read(*id)
    }
}

impl<R: Read, I> std::fmt::Debug for Mdd<R, I> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Mdd")
            .field("entries", &self.reader.entries().len())
            .finish_non_exhaustive()
    }
}

/// An explicitly bound immutable file and its virtual resource key.
///
/// For an MDD, `key` is a namespace prefix (empty mounts at the virtual root).
/// For a loose file, `key` is the exact virtual filename.
#[derive(Debug, Clone)]
pub struct ResourceMount {
    /// Absolute managed path or a path relative to the collection root.
    pub path: PathBuf,
    /// Virtual namespace or filename, independent of the physical filename.
    pub key: String,
}

struct Mounted {
    path: PathBuf,
    key: String,
    mdd: bool,
    file: Mutex<Option<Arc<Mdd>>>,
}

/// Ordered resources whose MDD readers are initialized independently on demand.
///
/// Explicit mounts expose only registered files. Discovered collections also
/// permit loose files below their root. A failed unrelated volume is skipped
/// while searching for a valid match, but reported if no match can be located.
#[derive(Default)]
pub struct Resources {
    root: Option<PathBuf>,
    mounts: Vec<Mounted>,
    local_files: bool,
    limits: Limits,
}

impl std::fmt::Debug for Resources {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Resources")
            .field("has_root", &self.root.is_some())
            .field("mounts", &self.mounts.len())
            .finish_non_exhaustive()
    }
}

impl Resources {
    /// Discover the main MDD and numbered volumes, including numbering gaps.
    ///
    /// # Safety
    /// All discovered files and directory bindings must remain immutable for
    /// this collection's lifetime, including before the first lazy read.
    pub unsafe fn discover(source: &Path) -> Result<Self> {
        // SAFETY: The caller guarantees stable paths and immutable volumes.
        unsafe { Self::discover_with(source, Limits::default()) }
    }

    /// Discover associated resources with explicit component budgets.
    ///
    /// Directory I/O and volume-count errors are returned immediately; volume
    /// parsing and mapping are deferred. Numeric volumes are ordered numerically.
    ///
    /// # Safety
    /// All discovered files and directory bindings must remain immutable for
    /// this collection's lifetime, including before the first lazy read.
    pub unsafe fn discover_with(source: &Path, limits: Limits) -> Result<Self> {
        let parent = source
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let root = parent.canonicalize()?;
        let mut volumes = Vec::new();
        if let Some(stem) = source.file_stem().and_then(|name| name.to_str()) {
            let main = format!("{stem}.mdd");
            let prefix = format!("{stem}.");
            for entry in std::fs::read_dir(&root)? {
                let entry = entry?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                let number = if name == main {
                    Some(0)
                } else {
                    name.strip_prefix(&prefix)
                        .and_then(|name| name.strip_suffix(".mdd"))
                        .filter(|number| {
                            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                        })
                        .and_then(|number| number.parse::<u64>().ok())
                        .filter(|number| *number > 0)
                };
                if let Some(number) = number {
                    Limits::check(
                        "resource volumes",
                        (volumes.len() + 1) as u64,
                        limits.blocks,
                    )?;
                    volumes.push((number, entry.path()));
                }
            }
        }
        volumes.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        let mounts: Vec<_> = volumes
            .into_iter()
            .map(|(_, path)| ResourceMount {
                path,
                key: String::new(),
            })
            .collect();
        // SAFETY: The caller guarantees immutable discovered files.
        let mut resources = unsafe { Self::mount_with(&root, &mounts, limits)? };
        resources.local_files = true;
        Ok(resources)
    }

    /// Bind arbitrary ordered MDDs and loose files without opening the volumes.
    ///
    /// Paths must be inside `root`; missing or damaged volumes fail only when
    /// requested. No unregistered root file is accessible through this collection.
    /// Empty MDD keys mount at root; nonempty keys are namespace prefixes.
    ///
    /// # Safety
    /// The root must be a managed immutable revision. Mounted files and path
    /// bindings must remain unmodified until the collection and its bytes drop.
    pub unsafe fn mount_with(
        root: &Path,
        mounts: &[ResourceMount],
        limits: Limits,
    ) -> Result<Self> {
        Limits::check("resource mounts", mounts.len() as u64, limits.blocks)?;
        let root = root.canonicalize()?;
        let mut mounted = Vec::with_capacity(mounts.len());
        for mount in mounts {
            let path = Self::managed_path(&root, &mount.path)?;
            let key = Self::safe_key(&mount.key, limits)?;
            let mdd = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("mdd"));
            if !mdd && key == "\\" {
                return Err(Error::Corrupt(
                    "loose resource needs a virtual filename".into(),
                ));
            }
            mounted.push(Mounted {
                path,
                key,
                mdd,
                file: Mutex::new(None),
            });
        }
        Ok(Self {
            root: Some(root),
            mounts: mounted,
            local_files: false,
            limits,
        })
    }

    fn managed_path(root: &Path, path: &Path) -> Result<PathBuf> {
        let relative = if path.is_absolute() {
            path.strip_prefix(root)
                .map_err(|_| Error::Corrupt("resource path escapes managed root".into()))?
        } else {
            path
        };
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        {
            return Err(Error::Corrupt("resource path escapes managed root".into()));
        }
        Ok(root.join(relative))
    }

    fn safe_key(path: &str, limits: Limits) -> Result<String> {
        Limits::check("resource name", path.len() as u64, limits.key_index)?;
        let path = path.replace('\\', "/");
        let mut parts = Vec::new();
        for part in path.split('/') {
            if part == ".." || part.contains([':', '\0']) {
                return Err(Error::Corrupt("unsafe virtual resource path".into()));
            }
            if !part.is_empty() && part != "." {
                parts.push(part);
            }
        }
        Ok(Resource {
            path: &parts.join("/"),
        }
        .key())
    }

    fn canonical(&self, path: &Path) -> Result<PathBuf> {
        let canonical = path.canonicalize()?;
        if !self
            .root
            .as_ref()
            .is_some_and(|root| canonical.starts_with(root))
        {
            return Err(Error::Corrupt(
                "resource symlink escapes managed root".into(),
            ));
        }
        if !canonical.is_file() {
            return Err(Error::Corrupt("resource is not a regular file".into()));
        }
        Ok(canonical)
    }

    fn volume(&self, mount: &Mounted) -> Result<Arc<Mdd>> {
        let mut state = mount
            .file
            .lock()
            .map_err(|_| Error::Corrupt("resource initialization lock poisoned".into()))?;
        if let Some(file) = &*state {
            return Ok(Arc::clone(file));
        }
        let open = || -> Result<Mdd> {
            let canonical = self.canonical(&mount.path)?;
            // SAFETY: Construction requires managed immutable files and paths.
            unsafe { Mdd::open_with(&canonical, self.limits) }
        };
        let file = Arc::new(open().map_err(|source| Error::Resource {
            path: mount.path.clone(),
            source: Box::new(source),
        })?);
        *state = Some(Arc::clone(&file));
        Ok(file)
    }

    fn loose(&self, path: &Path) -> Result<Bytes> {
        let file = std::fs::File::open(self.canonical(path)?)?;
        Limits::check("resource bytes", file.metadata()?.len(), self.limits.record)?;
        let maximum = (self.limits.record as u64)
            .checked_add(1)
            .ok_or_else(|| Error::Corrupt("resource limit overflow".into()))?;
        let mut data = Vec::new();
        file.take(maximum).read_to_end(&mut data)?;
        Limits::check("resource bytes", data.len() as u64, self.limits.record)?;
        Ok(data.into())
    }

    /// Load the first matching explicit mount, following strict binary redirects.
    ///
    /// Unopened volumes are initialized one at a time. A broken unrelated volume
    /// cannot block a later known-good resource; if none matches, its error is
    /// returned instead of disguising the failed search as a missing key.
    /// A matching record's decode errors are never hidden by a later duplicate.
    /// Loose file bytes are returned unchanged, without redirect interpretation.
    pub fn load(&self, resource: &str) -> Result<Bytes> {
        let key = Self::safe_key(resource, self.limits)?;
        if self.local_files {
            if let Some(root) = &self.root {
                let relative = resource.replace('\\', "/");
                let local = root.join(relative.trim_start_matches('/'));
                if local.is_file() {
                    return self.loose(&local);
                }
            }
        }
        follow(&key, self.limits.redirects, |key| {
            // Redirect names are subject to the same sandbox as initial names.
            let key = Self::safe_key(key, self.limits)?;
            let mut failure = None;
            for mount in &self.mounts {
                if !mount.mdd {
                    if mount.key == key {
                        return self.loose(&mount.path).map(|bytes| (bytes, None));
                    }
                    continue;
                }
                let relative = if mount.key == "\\" {
                    key.as_str()
                } else {
                    let Some(relative) = key
                        .strip_prefix(&mount.key)
                        .and_then(|suffix| suffix.strip_prefix('\\'))
                    else {
                        continue;
                    };
                    relative
                };
                let volume = match self.volume(mount) {
                    Ok(volume) => volume,
                    Err(error) => {
                        if failure.is_none() {
                            failure = Some(error);
                        }
                        continue;
                    }
                };
                match volume.lookup(relative) {
                    Ok(bytes) => {
                        let target = Link::utf16(bytes.as_ref())?
                            .map(|target| {
                                let target = Self::safe_key(&target, self.limits)?;
                                if mount.key == "\\" {
                                    Ok(target)
                                } else {
                                    Self::safe_key(&format!("{}{}", mount.key, target), self.limits)
                                }
                            })
                            .transpose()?;
                        return Ok((bytes, target));
                    }
                    Err(Error::KeyNotFound(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(failure.unwrap_or_else(|| Error::KeyNotFound(key)))
        })
    }

    /// List virtual names and raw record sizes for one registered mount.
    ///
    /// Only that MDD is initialized. Payload blocks are not decompressed.
    /// `prefix` uses the same slash/case normalization as resource lookup.
    pub fn entries(
        &self,
        mount_path: &Path,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        let root = self
            .root
            .as_ref()
            .ok_or_else(|| Error::KeyNotFound(mount_path.display().to_string()))?;
        let path = Self::managed_path(root, mount_path)?;
        let mount = self
            .mounts
            .iter()
            .find(|mount| mount.path == path)
            .ok_or_else(|| Error::KeyNotFound(mount_path.display().to_string()))?;
        let prefix = Self::safe_key(prefix, self.limits)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        if !mount.mdd {
            return if mount.key.starts_with(&prefix) {
                Ok(vec![(
                    mount.key.replace('\\', "/"),
                    std::fs::metadata(self.canonical(&mount.path)?)?.len(),
                )])
            } else {
                Ok(Vec::new())
            };
        }
        let volume = self.volume(mount)?;
        let mut entries = Vec::new();
        for (id, entry) in volume.reader().entries().iter().enumerate() {
            let inner = Resource {
                path: &entry.headword,
            }
            .key();
            let key = if mount.key == "\\" {
                inner
            } else {
                format!("{}{}", mount.key, inner)
            };
            if key.starts_with(&prefix) {
                let original = entry.headword.replace('\\', "/");
                let original = original
                    .strip_prefix("./")
                    .unwrap_or(&original)
                    .trim_start_matches('/');
                let display = if mount.key == "\\" {
                    format!("/{original}")
                } else {
                    format!("{}/{original}", mount.key.replace('\\', "/"))
                };
                entries.push((display, volume.reader().record_size(id)?));
                if entries.len() == limit {
                    break;
                }
            }
        }
        Ok(entries)
    }

    /// Decompressed bytes currently owned by already-open volume caches.
    pub fn cache_bytes(&self) -> usize {
        self.mounts
            .iter()
            .map(|mount| {
                let state = mount
                    .file
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                state
                    .as_ref()
                    .map_or(0, |file| file.reader().cache_stats().resident)
            })
            .fold(0usize, usize::saturating_add)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn canonical_resource_keys() {
        assert_eq!(
            Resource {
                path: "./img/pic.png"
            }
            .key(),
            "\\img\\pic.png"
        );
        assert_eq!(
            Resource {
                path: "/css/style.css"
            }
            .key(),
            "\\css\\style.css"
        );
        assert_eq!(
            Resource {
                path: "\\fonts\\a.ttf"
            }
            .key(),
            "\\fonts\\a.ttf"
        );
        assert_eq!(Resource { path: ".hidden" }.key(), "\\.hidden");
        assert_eq!(
            Resource {
                path: "/IMG/Photo.PNG"
            }
            .key(),
            "\\img\\photo.png"
        );
    }

    #[test]
    fn volume_failures_are_not_cached_as_missing_resources() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("dict.mdx");
        fs::write(directory.path().join("dict.mdd"), b"broken").unwrap();
        // SAFETY: Test inputs remain immutable throughout the resource lifetime.
        let resources = unsafe { Resources::discover(&source) }.unwrap();
        for _ in 0..2 {
            match resources.load("missing.css") {
                Err(Error::Resource { path, source }) => {
                    assert!(
                        same_file::is_same_file(&path, directory.path().join("dict.mdd")).unwrap()
                    );
                    assert!(matches!(
                        *source,
                        Error::InvalidHeader(_) | Error::Limit { .. }
                    ));
                }
                other => panic!("expected volume error, got {other:?}"),
            }
        }
    }

    #[test]
    fn local_resources_obey_scope_and_size_budget() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("dictionary");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("style.css"), b"local CSS").unwrap();
        fs::write(directory.path().join("private.txt"), b"private").unwrap();
        let limits = Limits {
            record: 9,
            ..Limits::default()
        };
        // SAFETY: No MDD volumes exist and test files remain unchanged.
        let resources =
            unsafe { Resources::discover_with(&root.join("dict.mdx"), limits) }.unwrap();
        assert_eq!(
            resources.load("./style.css").unwrap().as_ref(),
            b"local CSS"
        );
        assert!(matches!(
            resources.load("../private.txt"),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            resources.load("missing.css"),
            Err(Error::KeyNotFound(_))
        ));
        let limits = Limits {
            record: 8,
            ..Limits::default()
        };
        // SAFETY: The same immutable source set is used.
        let limited = unsafe { Resources::discover_with(&root.join("dict.mdx"), limits) }.unwrap();
        assert!(matches!(
            limited.load("style.css"),
            Err(Error::Limit { .. })
        ));
    }
}
