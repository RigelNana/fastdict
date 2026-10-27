mod support;

use support::{Corpus, Row};
use wodix::{Encoding, Lookup, Mdd, Mdx, Read, Reader, Version};

#[test]
fn basic_articles_preserve_source_order_unicode_and_redirects() {
    let bytes = Corpus::basic_mdx().serialize().unwrap();
    let dictionary = Mdx::parse(bytes.as_slice(), "unused fallback").unwrap();
    let reader = dictionary.reader();
    assert_eq!(reader.metadata().title, "Offline fixture");
    assert_eq!(reader.metadata().version, Version::V2);
    assert_eq!(reader.metadata().encoding, Encoding::Utf8);
    let expected = [
        ("apple", "<p>A fruit.</p>"),
        ("apples", "@@@LINK=apple"),
        ("apricot", "<p>A stone fruit.</p>"),
        ("banana", "<p>A yellow fruit.</p>"),
        ("café", "<p>A cafe.</p>"),
        ("猫", "<p>A cat.</p>"),
    ];
    assert_eq!(reader.entries().len(), expected.len());
    for (id, (key, text)) in expected.into_iter().enumerate() {
        assert_eq!(reader.entries()[id].headword, key);
        assert_eq!(reader.text(id).unwrap(), text);
        assert_eq!(
            reader.read(id).unwrap().as_ref(),
            format!("{text}\0").as_bytes()
        );
    }
    assert_eq!(dictionary.lookup("apples").unwrap(), ["<p>A fruit.</p>"]);
    assert_eq!(dictionary.lookup(" CAFÉ ").unwrap(), ["<p>A cafe.</p>"]);
    assert_eq!(dictionary.lookup("猫").unwrap(), ["<p>A cat.</p>"]);
    assert_eq!(
        dictionary.prefix("ap", 10).unwrap(),
        ["apple", "apples", "apricot"]
    );
    assert_eq!(dictionary.fuzzy("applf", 1, 10).unwrap(), ["apple"]);
    assert!(dictionary.lookup("not in this fixture").unwrap().is_empty());
}

#[test]
fn resources_keep_raw_bytes_and_utf16_keys() {
    let bytes = Corpus::basic_mdd().serialize().unwrap();
    let resources = Mdd::parse(bytes, "unused fallback").unwrap();
    assert_eq!(resources.reader().metadata().encoding, Encoding::Utf16Le);
    assert_eq!(resources.reader().entries().len(), 2);
    assert_eq!(resources.reader().entries()[0].headword, "\\hello.txt");
    assert_eq!(
        resources.lookup("hello.txt").unwrap().as_ref(),
        b"hello resource"
    );
    assert_eq!(
        resources.lookup("/style.css").unwrap().as_ref(),
        b"body { color: black; }"
    );

    let bytes = Corpus::resources(vec![
        Row::new("\\alpha.bin", [1, 2, 3]),
        Row::new("\\猫\\𐐀.bin", [0, 255, 128, 0]),
        Row::new("\\末尾.bin", [7]),
    ])
    .serialize()
    .unwrap();
    let resources = Mdd::parse(bytes, "binary resources").unwrap();
    assert_eq!(
        resources.lookup("猫/𐐀.bin").unwrap().as_ref(),
        &[0, 255, 128, 0]
    );
    assert_eq!(resources.lookup("末尾.bin").unwrap().as_ref(), &[7]);
    assert!(matches!(
        resources.lookup("missing.bin"),
        Err(wodix::Error::KeyNotFound(_))
    ));
}

#[test]
fn duplicates_normalization_and_redirects_preserve_distinct_articles() {
    let bytes = Corpus::articles(vec![
        Row::new("echo", b"first"),
        Row::new("ECHO", b"second"),
        Row::new("echo", b"first"),
        Row::new("café", b"composed"),
        Row::new("cafe\u{301}", b"decomposed"),
        Row::new("alias", b"@@@LINK=echo"),
    ])
    .serialize()
    .unwrap();
    let dictionary = Mdx::parse(bytes, "duplicates").unwrap();
    assert_eq!(dictionary.index().exact("echo"), &[0, 1, 2]);
    assert_eq!(dictionary.lookup("echo").unwrap(), ["first", "second"]);
    assert_eq!(dictionary.lookup("alias").unwrap(), ["first", "second"]);
    assert_eq!(
        dictionary.lookup("CAFÉ").unwrap(),
        ["composed", "decomposed"]
    );
    let raw: Vec<_> = dictionary
        .reader()
        .records()
        .map(|record| {
            let (entry, bytes) = record.unwrap();
            (entry.headword.clone(), bytes.as_ref().to_vec())
        })
        .collect();
    assert_eq!(raw[0], ("echo".to_owned(), b"first\0".to_vec()));
    assert_eq!(raw[2], ("echo".to_owned(), b"first\0".to_vec()));
    assert_eq!(raw[5], ("alias".to_owned(), b"@@@LINK=echo\0".to_vec()));
}

#[test]
fn benchmark_records_survive_key_and_record_block_boundaries() {
    // 515 crosses both the 512-key and 128-record boundaries, including remainders.
    let corpus = Corpus::benchmark(515).unwrap();
    let bytes = corpus.serialize().unwrap();
    let reader = Reader::parse(bytes, "benchmark").unwrap();
    assert_eq!(reader.entries().len(), corpus.rows.len());
    for ((entry, record), expected) in reader.records().map(Result::unwrap).zip(&corpus.rows) {
        assert_eq!(entry.headword, expected.key);
        assert_eq!(&record.as_ref()[..511], expected.value);
        assert_eq!(record.as_ref()[511..], [0]);
    }
}

#[test]
fn committed_fixtures_match_the_offline_generator() {
    assert_eq!(
        include_bytes!("data/basic.mdx").as_slice(),
        Corpus::basic_mdx().serialize().unwrap(),
    );
    assert_eq!(
        include_bytes!("data/basic.mdd").as_slice(),
        Corpus::basic_mdd().serialize().unwrap(),
    );
}
