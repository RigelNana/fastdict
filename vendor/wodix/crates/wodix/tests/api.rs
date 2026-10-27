use wodix::{
    Bytes, Encoding, Encryption, Entry, Error, Index, Limits, Mdx, Metadata, Read, Reader,
    StyleSheet, Version,
};

struct Memory {
    metadata: Metadata,
    entries: Vec<Entry>,
}

impl Memory {
    fn new() -> Self {
        Self {
            metadata: Metadata {
                version: Version::V2,
                encoding: Encoding::Utf8,
                encrypted: Encryption::NONE,
                title: "Custom source".to_owned(),
                description: String::new(),
                stylesheets: StyleSheet::new(),
                rtl: false,
                attributes: Default::default(),
            },
            entries: vec![
                Entry {
                    headword: "entry".to_owned(),
                    record_offset: 0,
                },
                Entry {
                    headword: "alias".to_owned(),
                    record_offset: 12,
                },
            ],
        }
    }
}

impl Read for Memory {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn limits(&self) -> Limits {
        Limits::default()
    }

    fn read(&self, id: usize) -> wodix::Result<Bytes> {
        let text = match id {
            0 => "from memory\0",
            1 => "@@@LINK=entry\0",
            _ => return Err(Error::Corrupt("invalid source-local ordinal".to_owned())),
        };
        Ok(text.as_bytes().to_vec().into())
    }
}

#[test]
fn public_root_traits_support_a_non_format_reader() -> wodix::Result<()> {
    let dictionary = Mdx::<_, Index>::new(Memory::new())?;
    assert_eq!(dictionary.reader().text(1)?, "@@@LINK=entry");
    assert_eq!(
        dictionary.lookup(" ALIAS ")?,
        vec!["from memory".to_owned()]
    );
    assert_eq!(dictionary.prefix("en", 1)?, vec!["entry"]);
    assert!(dictionary.lookup("missing")?.is_empty());
    Ok(())
}

#[test]
fn public_byte_views_survive_source_destruction() -> wodix::Result<()> {
    let held = {
        let data = include_bytes!("data/basic.mdx");
        let mut limits = Limits::default();
        limits.cache = 0;
        let reader = Reader::with_limits(data, "fixture", limits)?;
        let bytes = reader.read(0)?;
        assert_eq!(reader.cache_stats().resident, 0);
        bytes
    };
    assert_eq!(held.as_ref(), b"<p>A fruit.</p>\0");
    Ok(())
}
