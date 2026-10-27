//! MDX stylesheet marker expansion with an explicit output budget.

use crate::types::StyleSheet;
use crate::{Error, Limits, Result};

pub struct Styles<'s> {
    pub rules: &'s StyleSheet,
    pub limit: usize,
}

impl Styles<'_> {
    pub fn apply(&self, article: &str) -> Result<String> {
        if self.rules.is_empty() || !article.contains('`') {
            Limits::check("rendered article", article.len() as u64, self.limit)?;
            return Ok(article.to_owned());
        }
        let mut result = String::new();
        result
            .try_reserve_exact(article.len().min(self.limit))
            .map_err(|error| Error::Corrupt(format!("stylesheet allocation: {error}")))?;
        let mut suffix = "";
        let mut rest = article;
        while let Some(start) = rest.find('`') {
            self.append(&mut result, &rest[..start])?;
            rest = &rest[start + 1..];
            let end = rest.bytes().take_while(u8::is_ascii_digit).count();
            let digits = &rest[..end];
            let closed = end > 0 && rest.as_bytes().get(end) == Some(&b'`');
            if closed {
                rest = &rest[end + 1..];
                if let Ok(id) = digits.parse::<i32>() {
                    self.append(&mut result, suffix)?;
                    if let Some((prefix, next_suffix)) = self.rules.get(&id) {
                        self.append(&mut result, prefix)?;
                        suffix = next_suffix;
                    } else {
                        suffix = "";
                    }
                    continue;
                }
            } else {
                rest = &rest[end..];
            }
            self.append(&mut result, "`")?;
            self.append(&mut result, digits)?;
            if closed {
                self.append(&mut result, "`")?;
            }
        }
        self.append(&mut result, rest)?;
        result.truncate(result.trim_end_matches('\0').len());
        self.append(&mut result, suffix)?;
        Ok(result)
    }

    fn append(&self, output: &mut String, text: &str) -> Result<()> {
        let required = output
            .len()
            .checked_add(text.len())
            .ok_or_else(|| Error::Corrupt("stylesheet length overflow".into()))?;
        Limits::check("rendered article", required as u64, self.limit)?;
        if required > output.capacity() {
            let capacity = required
                .max(output.capacity().saturating_mul(2))
                .min(self.limit);
            output
                .try_reserve_exact(capacity - output.len())
                .map_err(|error| Error::Corrupt(format!("stylesheet allocation: {error}")))?;
        }
        output.push_str(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture;

    impl Fixture {
        fn styles(entries: &[(i32, &str, &str)]) -> StyleSheet {
            let mut rules = StyleSheet::new();
            for &(id, prefix, suffix) in entries {
                rules.insert(id, (prefix.to_owned(), suffix.to_owned()));
            }
            rules
        }
    }

    #[test]
    fn substitute_single_style() {
        let rules = Fixture::styles(&[(1, "<b>", "</b>")]);
        let result = Styles {
            rules: &rules,
            limit: 1024,
        }
        .apply("`1`Hello`1`")
        .unwrap();
        assert_eq!(result, "<b>Hello</b><b></b>");
    }

    #[test]
    fn substitute_nested_styles() {
        let rules =
            Fixture::styles(&[(1, "<div class=\"a\">", "</div>"), (2, "<span>", "</span>")]);
        let result = Styles {
            rules: &rules,
            limit: 1024,
        }
        .apply("`1`outer`2`inner`2`rest`1`")
        .unwrap();
        assert_eq!(
            result,
            "<div class=\"a\">outer</div><span>inner</span><span>rest</span><div class=\"a\"></div>"
        );
    }

    #[test]
    fn substitute_unknown_id() {
        let rules = Fixture::styles(&[(1, "<b>", "</b>")]);
        let result = Styles {
            rules: &rules,
            limit: 1024,
        }
        .apply("`1`text`99`more")
        .unwrap();
        assert_eq!(result, "<b>text</b>more");
    }

    #[test]
    fn substitute_no_styles_or_markers() {
        let empty = StyleSheet::new();
        assert_eq!(
            Styles {
                rules: &empty,
                limit: 1024
            }
            .apply("`1`unchanged\0")
            .unwrap(),
            "`1`unchanged\0"
        );
        let rules = Fixture::styles(&[(1, "<b>", "</b>")]);
        assert_eq!(
            Styles {
                rules: &rules,
                limit: 1024
            }
            .apply("plain text")
            .unwrap(),
            "plain text"
        );
    }

    #[test]
    fn substitute_trailing_suffix() {
        let rules = Fixture::styles(&[(1, "<p>", "</p>")]);
        let result = Styles {
            rules: &rules,
            limit: 1024,
        }
        .apply("`1`content\0\0")
        .unwrap();
        assert_eq!(result, "<p>content</p>");
    }

    #[test]
    fn preserve_malformed_markers_and_utf8() {
        let rules = Fixture::styles(&[(1, "<b>", "</b>")]);
        let result = Styles {
            rules: &rules,
            limit: 1024,
        }
        .apply("`1`中文 `x = 1` `2147483648` `12末尾")
        .unwrap();
        assert_eq!(result, "<b>中文 `x = 1` `2147483648` `12末尾</b>");
    }

    #[test]
    fn expansion_and_plain_text_obey_output_budget() {
        let rules = Fixture::styles(&[(1, "12345678", "90")]);
        assert_eq!(
            Styles {
                rules: &rules,
                limit: 10
            }
            .apply("`1`")
            .unwrap(),
            "1234567890"
        );
        assert!(matches!(
            Styles {
                rules: &rules,
                limit: 9
            }
            .apply("`1`"),
            Err(Error::Limit { .. })
        ));
        assert!(matches!(
            Styles {
                rules: &rules,
                limit: 10
            }
            .apply("`1``1`"),
            Err(Error::Limit { .. })
        ));
        let empty = StyleSheet::new();
        assert!(matches!(
            Styles {
                rules: &empty,
                limit: 2
            }
            .apply("abc"),
            Err(Error::Limit { .. })
        ));
    }
}
