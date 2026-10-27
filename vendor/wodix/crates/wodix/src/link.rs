//! Redirect parsing and one shared cycle/depth policy for text and resources.

use std::collections::HashSet;
use std::fmt::Display;
use std::hash::Hash;

use crate::record_block::Bytes;
use crate::{Error, Result};

#[derive(Debug, PartialEq, Eq)]
pub struct Link<'a> {
    pub target: &'a str,
}

impl<'a> Link<'a> {
    pub fn parse(article: &'a str) -> Result<Option<Self>> {
        let Some(target) = article.trim().strip_prefix("@@@LINK=") else {
            return Ok(None);
        };
        let target = target.trim();
        if target.is_empty() {
            return Err(Error::RedirectMissing(String::new()));
        }
        Ok(Some(Self { target }))
    }

    pub fn utf16(data: &[u8]) -> Result<Option<String>> {
        const PREFIX: &[u8] = b"@\0@\0@\0L\0I\0N\0K\0=\0";
        let Some(payload) = data.strip_prefix(PREFIX) else {
            return Ok(None);
        };
        if payload.len() % 2 != 0 {
            return Err(Error::EncodingError("odd UTF-16 redirect length".into()));
        }
        let units: Vec<u16> = payload
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        let text = String::from_utf16(&units)
            .map_err(|error| Error::EncodingError(format!("UTF-16 redirect: {error}")))?;
        let target = text.trim();
        if target.is_empty() {
            return Err(Error::RedirectMissing(String::new()));
        }
        Ok(Some(target.to_owned()))
    }
}

/// `depth` counts redirect edges; an ordinary entry at depth zero is always legal.
pub(crate) struct Trace<K> {
    pub active: HashSet<K>,
    pub limit: usize,
}

impl<K: Eq + Hash + Display> Trace<K> {
    pub fn enter(&mut self, key: K, depth: usize) -> Result<()> {
        if self.active.contains(&key) {
            return Err(Error::RedirectCycle(key.to_string()));
        }
        if depth > self.limit {
            return Err(Error::RedirectLimit(self.limit));
        }
        self.active.insert(key);
        Ok(())
    }
}

#[cfg(test)]
pub struct Chain<L> {
    pub load: L,
    pub limit: usize,
}

#[cfg(test)]
impl<L: FnMut(&str) -> Result<Bytes>> Chain<L> {
    pub fn bytes(&mut self, path: &str) -> Result<Bytes> {
        follow(path, self.limit, |path| {
            let data = (self.load)(path)?;
            let target = Link::utf16(data.as_ref())?;
            Ok((data, target))
        })
    }
}

/// Shared binary redirect traversal; the loader supplies any namespace mapping.
pub(crate) fn follow(
    path: &str,
    limit: usize,
    mut load: impl FnMut(&str) -> Result<(Bytes, Option<String>)>,
) -> Result<Bytes> {
    let mut current = path.to_owned();
    let mut trace = Trace {
        active: HashSet::new(),
        limit,
    };
    let mut depth = 0usize;
    loop {
        trace.enter(current.clone(), depth)?;
        let (data, target) = match load(&current) {
            Ok(value) => value,
            Err(Error::KeyNotFound(_)) if depth > 0 => {
                return Err(Error::RedirectMissing(current));
            }
            Err(error) => return Err(error),
        };
        match target {
            Some(target) => {
                current = target;
                depth = depth.checked_add(1).ok_or(Error::RedirectLimit(limit))?;
            }
            None => return Ok(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture;

    impl Fixture {
        fn binary(target: &str) -> Bytes {
            let text = format!("@@@LINK={target}\0");
            let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
            bytes.into()
        }
    }

    #[test]
    fn text_redirect_borrows_trimmed_target() {
        let article = String::from("  @@@LINK=world  ");
        let link = Link::parse(&article).unwrap().unwrap();
        assert_eq!(link.target, "world");
        assert!(matches!(
            Link::parse("@@@LINK="),
            Err(Error::RedirectMissing(_))
        ));
        assert!(matches!(
            Link::parse("@@@LINK=   "),
            Err(Error::RedirectMissing(_))
        ));
        assert_eq!(Link::parse("<p>Hello</p>").unwrap(), None);
        assert_eq!(Link::parse("@@LINK=foo").unwrap(), None);
    }

    #[test]
    fn binary_redirect_encoding() {
        assert_eq!(
            Link::utf16(Fixture::binary("\\img\\real.png").as_ref()).unwrap(),
            Some("\\img\\real.png".into())
        );
        assert_eq!(Link::utf16(b"<html>not a link</html>").unwrap(), None);
        assert_eq!(Link::utf16(b"@\0@\0").unwrap(), None);
        assert!(matches!(
            Link::utf16(b"@\0@\0@\0L\0I\0N\0K\0=\0x"),
            Err(Error::EncodingError(_))
        ));
    }

    #[test]
    fn binary_chain_and_missing_resource() {
        let mut chain = Chain {
            load: |path: &str| match path {
                "\\a.css" => Ok(Fixture::binary("\\b.css")),
                "\\b.css" => Ok(b"body { color: red; }".to_vec().into()),
                "\\broken" => Ok(Fixture::binary("\\missing")),
                _ => Err(Error::KeyNotFound(path.to_owned())),
            },
            limit: 1,
        };
        assert_eq!(
            chain.bytes("\\a.css").unwrap().as_ref(),
            b"body { color: red; }"
        );
        assert!(matches!(
            chain.bytes("\\missing"),
            Err(Error::KeyNotFound(_))
        ));
        assert!(matches!(
            chain.bytes("\\broken"),
            Err(Error::RedirectMissing(_))
        ));
        chain.limit = 0;
        assert!(matches!(
            chain.bytes("\\a.css"),
            Err(Error::RedirectLimit(0))
        ));
    }

    #[test]
    fn binary_cycle_is_an_error() {
        let mut chain = Chain {
            load: |path: &str| {
                let target = if path == "\\a" { "\\b" } else { "\\a" };
                Ok(Fixture::binary(target))
            },
            limit: 10,
        };
        assert!(matches!(chain.bytes("\\a"), Err(Error::RedirectCycle(_))));
    }
}
