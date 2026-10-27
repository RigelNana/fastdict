//! Read-only composition of a source, its derived index and optional resources.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use memmap2::Mmap;

use crate::index::{Cache, Index, Lookup, Query};
use crate::link::{Link, Trace};
use crate::mdd::Resources;
use crate::reader::{Read, Reader};
use crate::stylesheet::Styles;
use crate::{Error, Limits, Result};

/// An MDX article source paired with its own headword index.
///
/// [`crate::Mdx::lookup`] resolves redirects and expands stylesheet markers;
/// [`crate::Read::read`] and [`crate::Read::text`] on its reader expose lower-level
/// record access. Parsing bytes does not discover filesystem resources, whereas
/// opening a file discovers associated volumes for later lazy reads.
pub struct Mdx<R = Reader<Mmap>, I = Index> {
    reader: R,
    index: I,
    resources: Resources,
}

/// One distinct decoded article with every matching source and terminal record.
///
/// IDs refer to this dictionary's source-order entry directory. Redirect targets
/// are retained in first-traversal order, including paths through shared targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    /// Style-expanded article HTML (not sanitized or rendered).
    pub html: String,
    /// Matching lookup entry IDs that reach this article.
    pub source_ids: Vec<usize>,
    /// Terminal record IDs with the same decoded article text.
    pub resolved_ids: Vec<usize>,
    /// Distinct redirect target names along the contributing paths.
    pub redirects: Vec<String>,
}

enum Step {
    Enter { id: usize, depth: usize },
    Exit(usize),
}

impl Step {
    fn queue(stack: &mut Vec<Self>, ids: &[usize], depth: usize, limits: Limits) -> Result<()> {
        let count = stack
            .len()
            .checked_add(ids.len())
            .ok_or_else(|| Error::Corrupt("redirect queue overflow".into()))?;
        let bytes = count
            .checked_mul(std::mem::size_of::<Self>())
            .ok_or_else(|| Error::Corrupt("redirect queue size overflow".into()))?;
        Limits::check("redirect queue", bytes as u64, limits.metadata)?;
        stack
            .try_reserve(ids.len())
            .map_err(|error| Error::Corrupt(format!("redirect queue allocation: {error}")))?;
        stack.extend(ids.iter().rev().map(|&id| Self::Enter { id, depth }));
        Ok(())
    }
}

impl Mdx {
    /// Open an immutable memory-mapped MDX with default [`crate::Limits`].
    ///
    /// Reads or creates the neighboring `.mdict.idx` derived index cache and
    /// discovers `.mdd` and numbered `.1.mdd`, `.2.mdd` volumes, including gaps.
    /// Cache I/O may create a persistent `.lock` sidecar and atomically replace
    /// the cache; see [`crate::Cache::open`]. MDD mapping is deferred until needed.
    ///
    /// # Errors
    ///
    /// Returns source I/O, parsing, index-building or discovery errors.
    /// Cache I/O/publication failures are warned about and fall back to an
    /// in-memory index; errors in lazily read records or volumes appear later.
    ///
    /// # Safety
    /// MDX and associated MDD files must remain unmodified and untruncated while
    /// this dictionary exists, including before lazy resource reads.
    /// Resource directory paths must remain stable for the same period.
    /// Advisory locking alone does not establish these invariants.
    pub unsafe fn open(path: &Path) -> Result<Self> {
        let limits = Limits::default();
        // SAFETY: The caller guarantees immutable source files.
        unsafe { Self::open_with(path, limits) }
    }

    /// Open an immutable memory-mapped MDX with explicit component budgets.
    ///
    /// Has the same cache-writing and resource-discovery effects as
    /// [`crate::Mdx::open`]; budgets are not an aggregate RSS cap.
    ///
    /// # Errors
    ///
    /// Returns the source, index or discovery errors from [`crate::Mdx::open`],
    /// including [`crate::Error::Limit`] when a component budget is exhausted.
    ///
    /// # Safety
    /// The MDX and associated MDD files must remain unmodified and untruncated
    /// for this dictionary's lifetime, including before lazy volume opening.
    /// Resource directory paths must remain stable. Advisory locks alone do
    /// not ensure this; use [`crate::Mdx::parse`] with owned bytes when uncertain.
    pub unsafe fn open_with(path: &Path, limits: Limits) -> Result<Self> {
        // SAFETY: The caller keeps all backing files immutable.
        let reader = unsafe { Reader::open_with(path, limits)? };
        let index =
            Cache::new(path.with_extension("mdict.idx"), reader.entries(), limits).open()?;
        // SAFETY: Associated volumes have the same caller-provided guarantee.
        let resources = unsafe { Resources::discover_with(path, limits)? };
        Ok(Self {
            reader,
            index,
            resources,
        })
    }
}

impl<B: AsRef<[u8]>> Mdx<Reader<B>> {
    /// Parse source bytes and build their index using default budgets.
    ///
    /// `fallback` supplies a missing title, not an encoding. No files are read or
    /// written and the associated resource collection is empty.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::Reader::parse`] and [`crate::Lookup::build`] failures.
    /// Records are lazy, so later lookups can still report damaged payloads.
    ///
    /// ```
    /// use wodix::Mdx;
    ///
    /// let data = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
    /// let dictionary = Mdx::parse(data.as_slice(), "fallback")?;
    /// assert_eq!(dictionary.lookup("apples")?, vec!["<p>A fruit.</p>"]);
    /// assert_eq!(dictionary.prefix("ap", 10)?, vec!["apple", "apples", "apricot"]);
    /// assert!(dictionary.lookup("not-in-this-fixture")?.is_empty());
    /// # Ok::<(), wodix::Error>(())
    /// ```
    pub fn parse(data: B, fallback: &str) -> Result<Self> {
        let reader = Reader::parse(data, fallback)?;
        Self::new(reader)
    }

    /// Parse and index source bytes with explicit [`crate::Limits`].
    ///
    /// Like [`crate::Mdx::parse`], this performs no filesystem I/O or resource
    /// discovery. `fallback` is used for a missing source title.
    ///
    /// # Errors
    ///
    /// Returns parsing or index-building errors, including exceeded budgets.
    pub fn with_limits(data: B, fallback: &str, limits: Limits) -> Result<Self> {
        let reader = Reader::with_limits(data, fallback, limits)?;
        Self::new(reader)
    }
}

impl<R: Read, I: Lookup> Mdx<R, I> {
    /// Build the index from this exact source; callers cannot replace either half.
    ///
    /// The new dictionary has no associated filesystem resources.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::Lookup::build`] errors using the reader's limits.
    pub fn new(reader: R) -> Result<Self> {
        let index = I::build(reader.entries(), reader.limits())?;
        Ok(Self {
            reader,
            index,
            resources: Resources::default(),
        })
    }

    /// Borrow the source for metadata, source-order entries and raw/text reads.
    pub fn reader(&self) -> &R {
        &self.reader
    }

    /// Borrow the index whose entry IDs refer only to this dictionary's reader.
    pub fn index(&self) -> &I {
        &self.index
    }

    /// Borrow the associated lazy resource collection.
    ///
    /// File-opened dictionaries search local files and discovered MDD volumes;
    /// byte-parsed dictionaries have an empty collection. Its `load` operation
    /// may perform filesystem I/O, open volume index caches and resolve redirects.
    pub fn resources(&self) -> &Resources {
        &self.resources
    }

    /// Return resolved, style-expanded articles for a normalized headword.
    ///
    /// An absent headword returns an empty vector. Duplicate IDs and aliases
    /// are traversed in source order; identical decoded article text is emitted
    /// once before style expansion. Input matching follows [`crate::Index`].
    /// Unlike [`crate::Read::text`], this follows `@@@LINK=` targets and expands
    /// the source's stylesheet markers. It does not fetch resources or render HTML.
    ///
    /// # Errors
    ///
    /// Missing redirect targets, cycles and depth exhaustion return
    /// [`crate::Error::RedirectMissing`], [`crate::Error::RedirectCycle`] or
    /// [`crate::Error::RedirectLimit`]. Read, encoding and component-budget errors
    /// are propagated. On failure no partial article vector is returned.
    pub fn lookup(&self, word: &str) -> Result<Vec<String>> {
        Ok(self
            .lookup_detailed(word)?
            .into_iter()
            .map(|article| article.html)
            .collect())
    }

    /// Resolve articles while retaining source, terminal and redirect provenance.
    ///
    /// Uses the same source ordering, text deduplication, stylesheet expansion
    /// and strict error semantics as [`Self::lookup`]. Shared redirect subgraphs
    /// decode each record once; provenance is merged rather than discarded.
    pub fn lookup_detailed(&self, word: &str) -> Result<Vec<Article>> {
        let limits = self.reader.limits();
        let roots = self.index.exact(word);
        let mut stack = Vec::new();
        Step::queue(&mut stack, roots, 0, limits)?;
        let mut trace = Trace {
            active: HashSet::new(),
            limit: limits.redirects,
        };
        let mut complete: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut heights: HashMap<usize, usize> = HashMap::new();
        let mut edges: HashMap<usize, &[usize]> = HashMap::new();
        let mut redirects = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut articles: Vec<Article> = Vec::new();
        let mut output_size = 0usize;
        let mut visits = 0u64;
        let mut decoded_size = 0usize;
        let mut provenance_size = 0usize;
        let mut charge = |amount: usize| -> Result<()> {
            provenance_size = provenance_size
                .checked_add(amount)
                .ok_or_else(|| Error::Corrupt("lookup provenance size overflow".into()))?;
            Limits::check("lookup provenance", provenance_size as u64, limits.metadata)
        };

        while let Some(step) = stack.pop() {
            match step {
                Step::Exit(id) => {
                    trace.active.remove(&id);
                    let mut outcomes = Vec::new();
                    let mut height = 0usize;
                    for target in edges.remove(&id).unwrap() {
                        height = height.max(heights[target].saturating_add(1));
                        for &article in &complete[target] {
                            if !outcomes.contains(&article) {
                                charge(std::mem::size_of::<usize>())?;
                                outcomes.push(article);
                            }
                        }
                    }
                    complete.insert(id, outcomes);
                    heights.insert(id, height);
                }
                Step::Enter { id, depth } => {
                    if complete.contains_key(&id) {
                        if depth.saturating_add(heights[&id]) > limits.redirects {
                            return Err(Error::RedirectLimit(limits.redirects));
                        }
                        continue;
                    }
                    trace.enter(id, depth)?;
                    visits = visits
                        .checked_add(1)
                        .ok_or_else(|| Error::Corrupt("lookup work overflow".into()))?;
                    Limits::check("lookup visits", visits, limits.entries)?;
                    charge(std::mem::size_of::<(usize, Vec<usize>)>())?;
                    let raw = self.reader.text(id)?;
                    decoded_size = decoded_size
                        .checked_add(raw.capacity())
                        .ok_or_else(|| Error::Corrupt("lookup decoded size overflow".into()))?;
                    Limits::check("lookup decoded bytes", decoded_size as u64, limits.metadata)?;
                    if let Some(link) = Link::parse(&raw)? {
                        let targets = self.redirect(link.target);
                        if targets.is_empty() {
                            return Err(Error::RedirectMissing(link.target.to_owned()));
                        }
                        let next = depth
                            .checked_add(1)
                            .ok_or(Error::RedirectLimit(limits.redirects))?;
                        charge(link.target.len() + std::mem::size_of::<(usize, String)>())?;
                        redirects.push((id, link.target.to_owned()));
                        edges.insert(id, targets);
                        stack.push(Step::Exit(id));
                        Step::queue(&mut stack, targets, next, limits)?;
                    } else {
                        trace.active.remove(&id);
                        let article_id = if let Some(&article_id) = seen.get(&raw) {
                            article_id
                        } else {
                            let styles = Styles {
                                rules: &self.reader.metadata().stylesheets,
                                limit: limits.metadata - output_size,
                            };
                            let html = styles.apply(&raw)?;
                            output_size += html.len();
                            let article_id = articles.len();
                            charge(std::mem::size_of::<Article>())?;
                            articles.push(Article {
                                html,
                                source_ids: Vec::new(),
                                resolved_ids: Vec::new(),
                                redirects: Vec::new(),
                            });
                            seen.insert(raw, article_id);
                            article_id
                        };
                        charge(2 * std::mem::size_of::<usize>())?;
                        articles[article_id].resolved_ids.push(id);
                        complete.insert(id, vec![article_id]);
                        heights.insert(id, 0);
                    }
                }
            }
        }
        for &source in roots {
            for &article in &complete[&source] {
                if !articles[article].source_ids.contains(&source) {
                    charge(std::mem::size_of::<usize>())?;
                    articles[article].source_ids.push(source);
                }
            }
        }
        for (id, target) in redirects {
            for &article in &complete[&id] {
                if !articles[article].redirects.contains(&target) {
                    charge(target.len() + std::mem::size_of::<String>())?;
                    articles[article].redirects.push(target.clone());
                }
            }
        }
        Ok(articles)
    }

    /// Return at most `limit` distinct original headwords matching a prefix.
    ///
    /// With [`crate::Index`], normalized keys are visited lexicographically and
    /// duplicate groups retain source order. No article payloads are decoded.
    /// A zero limit returns an empty vector without evaluating the query.
    ///
    /// # Errors
    ///
    /// Propagates query validation or budget errors from [`crate::Lookup::visit`].
    /// On error no partial result vector is returned.
    pub fn prefix(&self, word: &str, limit: usize) -> Result<Vec<&str>> {
        let query = Query::Prefix(word);
        self.search(query, limit)
    }

    /// Return at most `limit` distinct headwords within the given edit distance.
    ///
    /// [`crate::Index`] measures Levenshtein distance in Unicode scalar values
    /// after normalization, not UTF-8 bytes or grapheme clusters. Results follow
    /// normalized lexical order, not a relevance or distance ranking. A zero
    /// limit returns no results without query evaluation.
    ///
    /// # Errors
    ///
    /// Propagates search and fuzzy work-budget errors from [`crate::Lookup::visit`].
    /// No partial result vector is returned if the search fails.
    pub fn fuzzy(&self, word: &str, distance: u32, limit: usize) -> Result<Vec<&str>> {
        let query = Query::Fuzzy { word, distance };
        self.search(query, limit)
    }

    fn search(&self, query: Query<'_>, limit: usize) -> Result<Vec<&str>> {
        let mut results = Vec::new();
        if limit == 0 {
            return Ok(results);
        }
        let entries = self.reader.entries();
        let mut seen = HashSet::new();
        self.index.visit(query, |ids| {
            for &id in ids {
                if let Some(entry) = entries.get(id) {
                    if seen.insert(entry.headword.as_str()) {
                        results.push(entry.headword.as_str());
                        if results.len() == limit {
                            return false;
                        }
                    }
                }
            }
            true
        })?;
        Ok(results)
    }

    fn redirect(&self, target: &str) -> &[usize] {
        let entries = self.index.exact(target);
        if !entries.is_empty() {
            return entries;
        }
        let trimmed = target
            .trim()
            .trim_end_matches(|character: char| character.is_ascii_punctuation());
        if trimmed != target {
            return self.index.exact(trimmed);
        }
        &[]
    }
}

impl<R: Read, I> std::fmt::Debug for Mdx<R, I> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Mdx")
            .field("title_bytes", &self.reader.metadata().title.len())
            .field("entries", &self.reader.entries().len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_block::Bytes;
    use crate::types::{Encoding, Encryption, Entry, Metadata, StyleSheet, Version};

    #[test]
    fn debug_omits_unbounded_metadata() {
        let mut reader = Memory::new(&[("word", "article")], Limits::default());
        reader.metadata.title = "private-title".repeat(100_000);
        let dictionary: Mdx<_> = Mdx::new(reader).unwrap();
        for rendered in [format!("{dictionary:?}"), format!("{dictionary:#?}")] {
            assert!(rendered.len() < 256);
            assert!(!rendered.contains("private-title"));
        }
    }

    struct Memory<'a> {
        data: &'a [(&'a str, &'a str)],
        entries: Vec<Entry>,
        metadata: Metadata,
        limits: Limits,
    }

    impl<'a> Memory<'a> {
        fn new(data: &'a [(&'a str, &'a str)], limits: Limits) -> Self {
            let entries = data
                .iter()
                .enumerate()
                .map(|(id, (word, _))| Entry {
                    headword: (*word).to_owned(),
                    record_offset: id as u64,
                })
                .collect();
            Self {
                data,
                entries,
                limits,
                metadata: Metadata {
                    version: Version::V2,
                    encoding: Encoding::Utf8,
                    encrypted: Encryption::NONE,
                    title: "memory source".into(),
                    description: String::new(),
                    stylesheets: StyleSheet::new(),
                    rtl: false,
                    attributes: Default::default(),
                },
            }
        }
    }

    impl Read for Memory<'_> {
        fn metadata(&self) -> &Metadata {
            &self.metadata
        }
        fn entries(&self) -> &[Entry] {
            &self.entries
        }
        fn limits(&self) -> Limits {
            self.limits
        }
        fn read(&self, id: usize) -> Result<Bytes> {
            let (_, text) = self
                .data
                .get(id)
                .ok_or_else(|| Error::Corrupt("bad memory entry".into()))?;
            Ok(text.as_bytes().to_vec().into())
        }
    }

    #[test]
    fn unique_candidates_are_not_limited_by_duplicate_postings() {
        let data = [("aa", "A"); 6]
            .into_iter()
            .chain([("ab", "B")])
            .collect::<Vec<_>>();
        let dictionary = Mdx::<_, Index>::new(Memory::new(&data, Limits::default())).unwrap();
        assert_eq!(dictionary.prefix("a", 2).unwrap(), vec!["aa", "ab"]);
        assert_eq!(dictionary.fuzzy("a", 1, 2).unwrap(), vec!["aa", "ab"]);
        assert!(dictionary.prefix("a", 0).unwrap().is_empty());
        assert!(dictionary.fuzzy("a", 1, 0).unwrap().is_empty());
    }

    #[test]
    fn fuzzy_distance_counts_scalars_and_reports_exhausted_work() {
        let data = [
            ("猫", "cat"),
            ("狗", "dog"),
            ("猫咪", "kitten"),
            ("\u{20000}", "han"),
            ("\u{10400}", "deseret"),
        ];
        let dictionary = Mdx::<_, Index>::new(Memory::new(&data, Limits::default())).unwrap();
        assert_eq!(
            dictionary.fuzzy("猫", 1, 10).unwrap(),
            vec!["狗", "猫", "猫咪", "\u{10400}", "\u{20000}"]
        );
        let limits = Limits {
            fuzzy_cells: 4,
            ..Limits::default()
        };
        let short = [("a", "A"), ("b", "B")];
        let limited = Mdx::<_, Index>::new(Memory::new(&short, limits)).unwrap();
        assert!(matches!(
            limited.fuzzy("z", 1, 10),
            Err(Error::Limit {
                kind: "fuzzy cells",
                ..
            })
        ));
    }

    #[test]
    fn redirects_distinguish_missing_cycles_and_real_misses() {
        let data = [
            ("x", "@@@LINK=missing"),
            ("y", "@@@LINK=z"),
            ("z", "@@@LINK=y"),
        ];
        let dictionary = Mdx::<_, Index>::new(Memory::new(&data, Limits::default())).unwrap();
        assert!(matches!(
            dictionary.lookup("x"),
            Err(Error::RedirectMissing(_))
        ));
        assert!(matches!(
            dictionary.lookup("y"),
            Err(Error::RedirectCycle(_))
        ));
        assert!(dictionary.lookup("not present").unwrap().is_empty());
    }

    #[test]
    fn shared_redirect_targets_are_not_cycles() {
        let data = [
            ("root", "@@@LINK=target"),
            ("root", "@@@LINK=target"),
            ("target", "definition"),
        ];
        let dictionary = Mdx::<_, Index>::new(Memory::new(&data, Limits::default())).unwrap();
        assert_eq!(dictionary.lookup("root").unwrap(), vec!["definition"]);
        let detailed = dictionary.lookup_detailed("root").unwrap();
        assert_eq!(detailed[0].source_ids, vec![0, 1]);
        assert_eq!(detailed[0].resolved_ids, vec![2]);
        assert_eq!(detailed[0].redirects, vec!["target"]);
        let limits = Limits {
            redirects: 0,
            ..Limits::default()
        };
        let dictionary = Mdx::<_, Index>::new(Memory::new(&data, limits)).unwrap();
        assert!(matches!(
            dictionary.lookup("root"),
            Err(Error::RedirectLimit(0))
        ));
        assert_eq!(dictionary.lookup("target").unwrap(), vec!["definition"]);
    }

    #[test]
    fn detailed_articles_preserve_senses_styles_and_shared_paths() {
        let data = [
            ("root", "@@@LINK=left"),
            ("root", "@@@LINK=right"),
            ("root", "`1`sense two"),
            ("left", "@@@LINK=target"),
            ("right", "@@@LINK=target"),
            ("target", "`1`sense one"),
            ("target", "`1`sense one"),
        ];
        let mut reader = Memory::new(&data, Limits::default());
        reader
            .metadata
            .stylesheets
            .insert(1, ("<b>".into(), "</b>".into()));
        let dictionary = Mdx::<_, Index>::new(reader).unwrap();
        let articles = dictionary.lookup_detailed("root").unwrap();
        assert_eq!(articles.len(), 2);
        assert_eq!(articles[0].html, "<b>sense one</b>");
        assert_eq!(articles[0].source_ids, vec![0, 1]);
        assert_eq!(articles[0].resolved_ids, vec![5, 6]);
        assert_eq!(articles[0].redirects, vec!["left", "target", "right"]);
        assert_eq!(articles[1].html, "<b>sense two</b>");
        assert_eq!(articles[1].source_ids, vec![2]);
        assert_eq!(articles[1].resolved_ids, vec![2]);
        assert!(articles[1].redirects.is_empty());
        assert_eq!(
            dictionary.lookup("root").unwrap(),
            vec!["<b>sense one</b>", "<b>sense two</b>"]
        );
    }

    #[test]
    fn shared_cached_subgraph_still_obeys_depth_budget() {
        let data = [
            ("root", "@@@LINK=target"),
            ("root", "@@@LINK=middle"),
            ("middle", "@@@LINK=target"),
            ("target", "article"),
        ];
        let limits = Limits {
            redirects: 1,
            ..Limits::default()
        };
        let dictionary = Mdx::<_, Index>::new(Memory::new(&data, limits)).unwrap();
        assert!(matches!(
            dictionary.lookup("root"),
            Err(Error::RedirectLimit(1))
        ));
    }

    #[test]
    fn empty_and_truncated_sources_are_errors() {
        assert!(matches!(
            Mdx::parse(Vec::new(), "empty"),
            Err(Error::InvalidHeader(_))
        ));
        assert!(matches!(
            Mdx::parse(100_i32.to_be_bytes(), "short"),
            Err(Error::InvalidHeader(_))
        ));
        // SAFETY: No backing file exists.
        assert!(matches!(
            unsafe { Mdx::open(Path::new("/nonexistent/dict.mdx")) },
            Err(Error::Io(_))
        ));
    }
}
