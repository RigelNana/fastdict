//! MDX header parsing.
//!
//! The header section of an MDX file:
//! ```text
//! [header_text_size: i32 BE] [header_text: UTF-16LE bytes] [checksum: u32 LE]
//! ```

use crate::Result;
use crate::checksum::{Adler, Verify};
use crate::decode::Decode;
use crate::error::Error;
use crate::limits::Limits;
use crate::types::{Encoding, Encryption, Metadata, StyleSheet, Version};

/// Decode an MDX header, using a borrowed title when its title is absent.
pub struct Header<'t> {
    pub fallback: &'t str,
    pub limits: &'t Limits,
}

impl<'a> Decode<'a> for Header<'_> {
    type Output = Metadata;

    fn decode(&self, data: &'a [u8]) -> Result<(Metadata, usize)> {
        let size_bytes = data
            .get(..4)
            .ok_or_else(|| Error::InvalidHeader("file too small for header size".into()))?;
        let size = i32::from_be_bytes(size_bytes.try_into().unwrap());
        let size = usize::try_from(size)
            .map_err(|_| Error::InvalidHeader("negative header size".into()))?;
        Limits::check("header", size as u64, self.limits.header)?;
        let checksum_offset = 4usize
            .checked_add(size)
            .ok_or_else(|| Error::InvalidHeader("header size overflow".into()))?;
        let consumed = checksum_offset
            .checked_add(4)
            .ok_or_else(|| Error::InvalidHeader("header size overflow".into()))?;
        let content = data
            .get(4..consumed)
            .ok_or_else(|| Error::InvalidHeader("file too small for header content".into()))?;
        let (text, checksum) = content.split_at(size);
        let expected = u32::from_le_bytes(checksum.try_into().unwrap());
        Adler { expected }.verify(text)?;

        // Version 3 switched the header itself to UTF-8. Detect the XML opening
        // rather than interpreting those bytes as plausible UTF-16 code units.
        let utf8 = text.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(text);
        let utf8_header = utf8
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|start| utf8[start] == b'<' && utf8.get(start + 1) != Some(&0));
        Limits::check(
            "metadata",
            (size as u64).saturating_mul(3),
            self.limits.metadata,
        )?;
        let xml = if utf8_header {
            Encoding::Utf8.decode(utf8)?
        } else {
            Encoding::Utf16Le.decode(text)?
        };
        let metadata = self.xml(&xml)?;
        if utf8_header {
            return Err(Error::InvalidHeader(
                "v1/v2 header must use UTF-16LE".into(),
            ));
        }
        Ok((metadata, consumed))
    }
}

impl Header<'_> {
    fn xml(&self, text: &str) -> Result<Metadata> {
        let cleaned = text.trim_start_matches('\u{feff}').trim_end_matches('\0');
        if cleaned.chars().any(|c| {
            (c < '\u{20}' && !matches!(c, '\n' | '\r' | '\t'))
                || matches!(c, '\u{fffe}' | '\u{ffff}')
        }) {
            return Err(Error::InvalidHeader("invalid XML control character".into()));
        }
        let attrs = self.attributes(cleaned)?;
        let version = match attrs.get("GeneratedByEngineVersion") {
            Some(value) => value
                .parse::<f64>()
                .map_err(|_| Error::InvalidHeader("invalid engine version".into()))?,
            None => 1.0,
        };
        let version = Version::try_from(version)?;
        let encoding = Encoding::try_from(attrs.get("Encoding").map(String::as_str).unwrap_or(""))?;
        let encrypted = match attrs.get("Encrypted").map(|value| value.trim()) {
            None | Some("No") => 0,
            Some("Yes") => 1,
            Some(value) => value
                .parse::<u32>()
                .map_err(|_| Error::InvalidHeader("invalid Encrypted attribute".into()))?,
        };
        if encrypted & !2 != 0 {
            return Err(Error::UnsupportedEncryption(encrypted));
        }
        let raw_title = attrs.get("Title").map(String::as_str).unwrap_or("");
        let title_size = if raw_title.is_empty() || raw_title == "Title (No HTML code allowed)" {
            self.fallback.len()
        } else {
            raw_title.len()
        };
        let attribute_size = attrs.iter().try_fold(0usize, |total, (key, value)| {
            total
                .checked_add(key.capacity())
                .and_then(|size| size.checked_add(value.capacity()))
                .and_then(|size| size.checked_add(std::mem::size_of::<(String, String)>()))
                .ok_or_else(|| Error::Corrupt("header attributes size overflow".into()))
        })?;
        let base_size = title_size
            .checked_add(attribute_size)
            .and_then(|size| size.checked_add(attrs.get("Description").map_or(0, String::capacity)))
            .and_then(|size| size.checked_add(std::mem::size_of::<Metadata>()))
            .ok_or_else(|| Error::Corrupt("header metadata size overflow".into()))?;
        Limits::check("metadata", base_size as u64, self.limits.metadata)?;
        let title = self.title(raw_title);
        let stylesheets = self.styles(
            attrs.get("StyleSheet").map(String::as_str).unwrap_or(""),
            base_size,
        )?;
        let rtl = attrs.get("Left2Right").map(String::as_str).unwrap_or("") != "Yes";

        Ok(Metadata {
            version,
            encoding,
            encrypted: Encryption(encrypted),
            title,
            description: attrs.get("Description").cloned().unwrap_or_default(),
            stylesheets,
            rtl,
            attributes: attrs,
        })
    }

    /// Parse newline-separated (id, prefix, suffix) triplets.
    fn styles(&self, text: &str, mut metadata: usize) -> Result<StyleSheet> {
        let mut rules = StyleSheet::new();
        let normalized = if text.contains("\r\n") {
            std::borrow::Cow::Owned(text.replace("\r\n", "\n"))
        } else {
            std::borrow::Cow::Borrowed(text)
        };
        let mut lines = normalized.split(['\r', '\n']);
        while let (Some(id), Some(prefix), Some(suffix)) =
            (lines.next(), lines.next(), lines.next())
        {
            if let Ok(id) = id.parse::<i32>() {
                let size = prefix
                    .len()
                    .checked_add(suffix.len())
                    .and_then(|size| {
                        size.checked_add(std::mem::size_of::<(i32, (String, String))>())
                    })
                    .and_then(|size| size.checked_add(metadata))
                    .ok_or_else(|| Error::Corrupt("stylesheet metadata size overflow".into()))?;
                Limits::check("metadata", size as u64, self.limits.metadata)?;
                metadata = size;
                rules.insert(id, (prefix.to_owned(), suffix.to_owned()));
            }
        }
        Ok(rules)
    }

    fn attributes(&self, xml: &str) -> Result<std::collections::BTreeMap<String, String>> {
        use quick_xml::events::Event;
        use quick_xml::reader::Reader;
        use std::collections::BTreeMap;

        let mut reader = Reader::from_str(xml);
        let mut attrs = BTreeMap::new();
        let mut root = false;
        let mut closed = false;
        let mut metadata = 0usize;
        loop {
            let event = reader
                .read_event()
                .map_err(|error| Error::InvalidHeader(error.to_string()))?;
            let empty = matches!(&event, Event::Empty(_));
            match event {
                Event::Start(element) | Event::Empty(element) => {
                    if root {
                        return Err(Error::InvalidHeader("unexpected XML element".into()));
                    }
                    root = true;
                    // A nonempty root has a matching end event; quick-xml checks names.
                    closed = empty;
                    for attr in element.attributes() {
                        let attr = attr.map_err(|error| Error::InvalidHeader(error.to_string()))?;
                        metadata = metadata
                            .checked_add(attr.key.as_ref().len())
                            .and_then(|size| size.checked_add(attr.value.len()))
                            .and_then(|size| {
                                size.checked_add(std::mem::size_of::<(String, String)>())
                            })
                            .ok_or_else(|| {
                                Error::Corrupt("header attributes size overflow".into())
                            })?;
                        Limits::check("metadata", metadata as u64, self.limits.metadata)?;
                        let key = std::str::from_utf8(attr.key.as_ref())
                            .map_err(|error| Error::InvalidHeader(error.to_string()))?
                            .to_owned();
                        let value = attr
                            .unescape_value()
                            .map_err(|error| Error::InvalidHeader(error.to_string()))?
                            .into_owned();
                        attrs.insert(key, value);
                    }
                }
                Event::End(_) => closed = true,
                Event::Text(text) if text.as_ref().iter().all(u8::is_ascii_whitespace) => {}
                Event::Decl(_) | Event::Comment(_) => {}
                Event::Eof => break,
                _ => return Err(Error::InvalidHeader("unexpected XML content".into())),
            }
        }
        if root && !closed {
            return Err(Error::InvalidHeader("unclosed XML header".into()));
        }
        if attrs.is_empty() {
            return Err(Error::InvalidHeader(
                "no XML attributes found in header".into(),
            ));
        }
        Ok(attrs)
    }

    fn title(&self, raw: &str) -> String {
        if raw.is_empty() || raw == "Title (No HTML code allowed)" {
            return self.fallback.to_string();
        }
        if !raw.contains('<') && !raw.contains('>') {
            return raw.to_string();
        }

        let mut result = String::with_capacity(raw.len());
        let mut inside_tag = false;
        for ch in raw.chars() {
            match ch {
                '<' => inside_tag = true,
                '>' => inside_tag = false,
                _ if !inside_tag => result.push(ch),
                _ => {}
            }
        }
        result.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture;

    impl Fixture {
        fn header(xml: &str) -> Vec<u8> {
            let text: Vec<u8> = xml.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            Self::bytes(&text)
        }

        fn bytes(text: &[u8]) -> Vec<u8> {
            let size = i32::try_from(text.len()).unwrap();
            let checksum = adler::adler32_slice(text);

            let mut data = Vec::new();
            data.extend_from_slice(&size.to_be_bytes());
            data.extend_from_slice(text);
            data.extend_from_slice(&checksum.to_le_bytes());
            data
        }
    }

    #[test]
    fn parse_header_v2_utf8() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Encrypted="0" Title="Test Dict" Description="A test" Left2Right="Yes" />"#;
        let data = Fixture::header(xml);

        let (info, consumed) = Header {
            fallback: "fallback",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap();
        assert_eq!(info.version, Version::V2);
        assert_eq!(info.encoding, Encoding::Utf8);
        assert_eq!(info.encrypted.0, 0);
        assert_eq!(info.title, "Test Dict");
        assert_eq!(info.description, "A test");
        assert!(!info.rtl);
        assert_eq!(consumed, data.len());
    }

    #[test]
    fn parse_header_v1_gbk() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="1.2" Encoding="GBK" Encrypted="0" Title="中文词典" Description="" Left2Right="Yes" />"#;
        let data = Fixture::header(xml);

        let (info, _) = Header {
            fallback: "fallback",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap();
        assert_eq!(info.version, Version::V1);
        assert_eq!(info.encoding, Encoding::Gb18030);
        assert_eq!(info.title, "中文词典");
    }

    #[test]
    fn parse_header_with_stylesheet() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Encrypted="0" StyleSheet="1
&lt;div class=&quot;s1&quot;&gt;
&lt;/div&gt;" Title="SS Test" Description="" Left2Right="Yes" />"#;
        let data = Fixture::header(xml);

        let (info, _) = Header {
            fallback: "fallback",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap();
        assert_eq!(info.stylesheets.len(), 1);
        let (prefix, suffix) = info.stylesheets.get(&1).unwrap();
        assert_eq!(prefix, r#"<div class="s1">"#);
        assert_eq!(suffix, "</div>");
    }

    #[test]
    fn parse_header_empty_title_uses_filename() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Encrypted="0" Title="" Description="" Left2Right="Yes" />"#;
        let data = Fixture::header(xml);

        let (info, _) = Header {
            fallback: "my_dictionary",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap();
        assert_eq!(info.title, "my_dictionary");
    }

    #[test]
    fn parse_header_html_title_strips_tags() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Encrypted="0" Title="&lt;b&gt;Bold Title&lt;/b&gt;" Description="" Left2Right="Yes" />"#;
        let data = Fixture::header(xml);

        let (info, _) = Header {
            fallback: "fallback",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap();
        assert_eq!(info.title, "Bold Title");
    }

    #[test]
    fn parse_header_bad_checksum_errors() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Title="X" />"#;
        let mut data = Fixture::header(xml);

        // Corrupt the checksum
        let len = data.len();
        data[len - 1] ^= 0xFF;

        let result = Header {
            fallback: "fallback",
            limits: &Limits::default(),
        }
        .decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn parse_header_truncated_errors() {
        for data in [
            &[0x00, 0x00, 0x01, 0x00][..], // Claims 256 bytes with no content.
            &[0xff, 0xff, 0xff, 0xff][..], // Negative signed size.
            &[0x00, 0x00, 0x00, 0x00][..], // Missing checksum.
        ] {
            let result = Header {
                fallback: "fallback",
                limits: &Limits::default(),
            }
            .decode(data);
            assert!(matches!(result, Err(Error::InvalidHeader(_))));
        }
    }

    #[test]
    fn parse_header_rtl_default() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Title="RTL" Description="" />"#;
        let data = Fixture::header(xml);

        let (info, _) = Header {
            fallback: "fallback",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap();
        // No Left2Right attribute → rtl = true (GoldenDict: "Left2Right != Yes" → RTL)
        assert!(info.rtl);
    }

    #[test]
    fn parse_header_encrypted_key_info() {
        let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Encrypted="2" Title="Enc" Description="" Left2Right="Yes" />"#;
        let data = Fixture::header(xml);

        let (info, _) = Header {
            fallback: "fallback",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap();
        assert_eq!(info.encrypted.0 & 2, 2);
    }

    #[test]
    fn utf8_v3_headers_report_unsupported_version() {
        let xml = br#"<Dictionary GeneratedByEngineVersion="3.0" Encoding="UTF-8" Title="v3" />"#;
        for text in [
            xml.to_vec(),
            [vec![0xef, 0xbb, 0xbf], xml.to_vec()].concat(),
        ] {
            let data = Fixture::bytes(&text);
            assert!(matches!(
                Header {
                    fallback: "",
                    limits: &Limits::default()
                }
                .decode(&data),
                Err(Error::UnsupportedVersion(3.0))
            ));
        }
    }

    #[test]
    fn unsupported_versions_and_encodings_are_explicit() {
        let limits = Limits::default();
        let header = Header {
            fallback: "",
            limits: &limits,
        };
        for version in ["0", "3.0", "NaN", "inf"] {
            let data = Fixture::header(&format!(
                r#"<Dictionary GeneratedByEngineVersion="{version}" />"#
            ));
            assert!(matches!(
                header.decode(&data),
                Err(Error::UnsupportedVersion(_))
            ));
        }
        let data = Fixture::header(r#"<Dictionary GeneratedByEngineVersion="banana" />"#);
        assert!(matches!(header.decode(&data), Err(Error::InvalidHeader(_))));
        let data = Fixture::header(r#"<Dictionary Encoding="unsupported" />"#);
        assert!(matches!(
            header.decode(&data),
            Err(Error::UnsupportedEncoding(_))
        ));
        let data = Fixture::header(r#"<Dictionary Encoding="Big5" />"#);
        assert_eq!(header.decode(&data).unwrap().0.encoding, Encoding::Big5);
    }

    #[test]
    fn encryption_attributes_reject_unsupported_flags() {
        let limits = Limits::default();
        let header = Header {
            fallback: "",
            limits: &limits,
        };
        for value in ["Yes", "1", "3", "4", "4294967295"] {
            let data = Fixture::header(&format!(r#"<Dictionary Encrypted="{value}" />"#));
            assert!(matches!(
                header.decode(&data),
                Err(Error::UnsupportedEncryption(_))
            ));
        }
        for value in ["", "-1", "maybe", "4294967296"] {
            let data = Fixture::header(&format!(r#"<Dictionary Encrypted="{value}" />"#));
            assert!(matches!(header.decode(&data), Err(Error::InvalidHeader(_))));
        }
        let data = Fixture::header(r#"<Dictionary Encrypted="No" />"#);
        assert_eq!(header.decode(&data).unwrap().0.encrypted.0, 0);
    }

    #[test]
    fn malformed_xml_attributes_are_not_ignored() {
        for xml in [
            r#"<Dictionary Title="valid" broken />"#,
            r#"<Dictionary Title="&unknown;" />"#,
            r#"<Dictionary Title="one" Title="two" />"#,
            r#"<Dictionary Title="unclosed">"#,
            r#"<Dictionary Title="valid"/><Dictionary Title="extra"/>"#,
        ] {
            assert!(matches!(
                Header {
                    fallback: "",
                    limits: &Limits::default()
                }
                .decode(&Fixture::header(xml)),
                Err(Error::InvalidHeader(_))
            ));
        }
    }

    #[test]
    fn invalid_header_text_is_not_replaced() {
        for text in [
            &[0x00, 0xd8][..],
            &[0x61][..],
            b"<Dictionary Title=\"\xff\"/>",
        ] {
            assert!(matches!(
                Header {
                    fallback: "",
                    limits: &Limits::default()
                }
                .decode(&Fixture::bytes(text)),
                Err(Error::EncodingError(_))
            ));
        }
    }

    #[test]
    fn header_and_metadata_budgets_precede_allocation() {
        let limits = Limits {
            header: 1024,
            ..Limits::default()
        };
        assert!(matches!(
            Header {
                fallback: "",
                limits: &limits
            }
            .decode(&i32::MAX.to_be_bytes()),
            Err(Error::Limit { kind: "header", .. })
        ));
        let limits = Limits {
            metadata: 1,
            ..Limits::default()
        };
        assert!(matches!(
            Header {
                fallback: "",
                limits: &limits
            }
            .decode(&Fixture::header(r#"<Dictionary Title="x"/>"#)),
            Err(Error::Limit {
                kind: "metadata",
                ..
            })
        ));
    }

    #[test]
    fn stylesheet_accepts_single_quotes_crlf_and_empty_suffix() {
        let data = Fixture::header("<Dictionary StyleSheet='1\r\n&lt;b&gt;\r\n' Title='x'/>");
        let metadata = Header {
            fallback: "",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap()
        .0;
        assert_eq!(
            metadata.stylesheets.get(&1),
            Some(&("<b>".to_owned(), String::new()))
        );
    }

    #[test]
    fn metadata_keeps_complete_exact_header_attributes() {
        let data = Fixture::header(
            r#"<Dictionary GeneratedByEngineVersion="2.00" Encoding="UTF-8" Title="&lt;b&gt;Name&lt;/b&gt;" Description="A &amp; B" DictionaryVersion="2026.04" CreationDate="2026-04-03" Custom=" untouched " />"#,
        );
        let metadata = Header {
            fallback: "",
            limits: &Limits::default(),
        }
        .decode(&data)
        .unwrap()
        .0;
        assert_eq!(metadata.title, "Name");
        assert_eq!(metadata.description, "A & B");
        assert_eq!(metadata.attributes["GeneratedByEngineVersion"], "2.00");
        assert_eq!(metadata.attributes["Title"], "<b>Name</b>");
        assert_eq!(metadata.attributes["Description"], "A & B");
        assert_eq!(metadata.attributes["DictionaryVersion"], "2026.04");
        assert_eq!(metadata.attributes["CreationDate"], "2026-04-03");
        assert_eq!(metadata.attributes["Custom"], " untouched ");
    }
}
