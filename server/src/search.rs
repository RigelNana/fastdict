use crate::model::{Dictionary, Snapshot};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use unicode_normalization::UnicodeNormalization;
pub fn normalize(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase().trim().to_owned()
}
#[derive(Clone, Serialize)]
pub struct Suggestion {
    pub word: String,
    pub dictionary_ids: Vec<String>,
    pub match_kind: String,
    pub preview: Option<String>,
}
pub struct Catalog {
    entries: Vec<(String, String, Vec<String>)>,
}
impl Catalog {
    pub fn new(words: Vec<(String, String)>) -> Self {
        let mut map: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for (id, word) in words {
            let ids = map.entry((normalize(&word), word)).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        Self {
            entries: map.into_iter().map(|((n, w), ids)| (n, w, ids)).collect(),
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn suggest(
        &self,
        q: &str,
        allowed_ids: &[String],
        limit: usize,
        distance: usize,
    ) -> (Vec<Suggestion>, bool) {
        let normalized = normalize(q);
        let allowed: HashSet<&str> = allowed_ids.iter().map(String::as_str).collect();
        let mut out = Vec::new();
        let start = self
            .entries
            .partition_point(|(n, _, _)| n.as_str() < normalized.as_str());
        for (n, w, ids) in self.entries[start..]
            .iter()
            .take_while(|(n, _, _)| n.starts_with(&normalized))
        {
            let ids: Vec<_> = allowed_ids
                .iter()
                .filter(|id| ids.contains(id))
                .cloned()
                .collect();
            if ids.is_empty() {
                continue;
            }
            out.push(Suggestion {
                word: w.clone(),
                dictionary_ids: ids,
                match_kind: if n == &normalized { "exact" } else { "prefix" }.into(),
                preview: None,
            });
            if out.len() > limit {
                break;
            }
        }
        let mut exhausted = false;
        if out.is_empty() && distance > 0 {
            let chars: Vec<_> = normalized.chars().collect();
            let mut row = vec![0; chars.len() + 1];
            let mut candidates: Vec<(usize, usize)> = Vec::with_capacity(limit + 2);
            let mut examined = 0usize;
            let budget = (2_000_000 / chars.len().max(1)).min(200_000);
            for (index, (n, _, ids)) in self.entries.iter().enumerate() {
                if !ids.iter().any(|id| allowed.contains(id.as_str())) {
                    continue;
                }
                if examined == budget {
                    exhausted = true;
                    break;
                }
                examined += 1;
                if let Some(distance) = edit_distance(&chars, n, distance, &mut row) {
                    let candidate = (distance, index);
                    let position = candidates.binary_search(&candidate).unwrap_or_else(|i| i);
                    if position <= limit {
                        candidates.insert(position, candidate);
                        candidates.truncate(limit + 1);
                    }
                }
            }
            for (_, index) in candidates {
                let (_, word, ids) = &self.entries[index];
                out.push(Suggestion {
                    word: word.clone(),
                    dictionary_ids: allowed_ids
                        .iter()
                        .filter(|id| ids.contains(id))
                        .cloned()
                        .collect(),
                    match_kind: "fuzzy".into(),
                    preview: None,
                });
            }
        }
        let truncated = exhausted || out.len() > limit;
        out.truncate(limit);
        (out, truncated)
    }
}
fn edit_distance(a: &[char], b: &str, max: usize, row: &mut [usize]) -> Option<usize> {
    if a.len().abs_diff(b.chars().count()) > max {
        return None;
    }
    for (i, value) in row.iter_mut().enumerate() {
        *value = i;
    }
    for (index, cb) in b.chars().enumerate() {
        let i = index + 1;
        let start = i.saturating_sub(max).max(1);
        let end = (i + max).min(a.len());
        let mut previous = row[0];
        row[0] = i;
        if start > 1 {
            previous = row[start - 1];
            row[start - 1] = max + 1;
        }
        let mut minimum = if i <= max { row[0] } else { max + 1 };
        for j in start..=end {
            let old = row[j];
            row[j] = (previous + usize::from(a[j - 1] != cb))
                .min(row[j - 1] + 1)
                .min(old + 1);
            previous = old;
            minimum = minimum.min(row[j]);
        }
        if end < a.len() {
            row[end + 1] = max + 1;
        }
        if minimum > max {
            return None;
        }
    }
    (row[a.len()] <= max).then_some(row[a.len()])
}
pub fn ordered_ids(s: &Snapshot, group: &str) -> Option<Vec<String>> {
    let ds: Vec<&Dictionary> = if group == "all" {
        s.dictionaries.iter().filter(|d| d.enabled).collect()
    } else {
        let g = s.groups.iter().find(|g| g.id == group)?;
        let mut ds: Vec<_> = g
            .dictionary_ids
            .iter()
            .filter_map(|id| s.dictionaries.iter().find(|d| d.id == *id && d.enabled))
            .collect();
        let names: std::collections::HashMap<_, _> = if g.sort.iter().any(|r| r.field == "name") {
            ds.iter()
                .map(|d| (d.id.as_str(), d.name.to_lowercase()))
                .collect()
        } else {
            Default::default()
        };
        ds.sort_by(|a, b| {
            for rule in &g.sort {
                let cmp = match rule.field.as_str() {
                    "name" => names[a.id.as_str()].cmp(&names[b.id.as_str()]),
                    "entries" => a.entries.cmp(&b.entries),
                    "created" => a.created.cmp(&b.created),
                    _ => std::cmp::Ordering::Equal,
                };
                let cmp = if rule.direction == "desc" {
                    cmp.reverse()
                } else {
                    cmp
                };
                if !cmp.is_eq() {
                    return cmp;
                }
            }
            std::cmp::Ordering::Equal
        });
        ds
    };
    Some(ds.into_iter().map(|d| d.id.clone()).collect())
}
pub fn tokenize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c as u32,0x3400..=0x9fff|0x20000..=0x3134f|0x3040..=0x30ff|0xac00..=0xd7af) {
            out.push(' ');
            out.push(c);
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}
pub fn fts_query(q: &str) -> String {
    format!("\"{}\"", tokenize(q).replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_matches_engine_without_compatibility_folding() {
        assert_eq!(normalize("  CAFÉ  "), "café");
        assert_eq!(normalize("Cafe\u{301}"), "café");
        assert_ne!(normalize("ａｐｐｌｅ"), normalize("apple"));
    }
    #[test]
    fn group_filter_precedes_prefix_limit() {
        let catalog = Catalog::new(vec![
            ("other".into(), "aardvark".into()),
            ("selected".into(), "apple".into()),
            ("selected".into(), "apricot".into()),
        ]);
        let (items, truncated) = catalog.suggest("a", &["selected".into()], 1, 1);
        assert_eq!(items[0].word, "apple");
        assert!(truncated);
    }
    #[test]
    fn fuzzy_distance_counts_unicode_characters() {
        let catalog = Catalog::new(vec![("d".into(), "词典".into())]);
        let (items, truncated) = catalog.suggest("辞典", &["d".into()], 20, 1);
        assert_eq!(items[0].word, "词典");
        assert_eq!(items[0].match_kind, "fuzzy");
        assert!(!truncated);
    }
    #[test]
    fn cjk_queries_are_contiguous_fts_phrases() {
        assert_eq!(fts_query("词典"), "\" 词  典 \"");
        assert_eq!(tokenize("dictionary 中文"), "dictionary  中  文 ");
    }
}
