//! Creates original MDX/MDD inputs for manual end-to-end verification; never auto-imported.
#[path = "../../vendor/wodix/crates/wodix/tests/support/mod.rs"]
mod support;

use anyhow::{Result, ensure};
use std::path::PathBuf;
use support::{Corpus, Row};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("usage: make_fixture <new-directory> [font.woff2]"))?,
    );
    ensure!(!output.exists(), "output must be a new directory");
    let alpha = output.join("Reference");
    let beta = output.join("Usage");
    std::fs::create_dir_all(alpha.join("assets"))?;
    std::fs::create_dir_all(beta.join("media"))?;
    let mut article = String::from(
        r##"<link rel="stylesheet" href="assets/style.css"><h2>apple</h2><p class="phonetic">/ˈæp.əl/</p><p class="definition">A round fruit with firm flesh and green or red skin.</p><p>苹果。一种常见的水果。</p><p><a href="entry://banana">banana</a> · <a href="#examples">examples</a> · <a href="sound://media/tone.wav">发音示例</a> · <a href="https://example.com/">外部链接</a></p><img alt="Original test icon" src="icon.png" width="32"><script>window.__dictionaryScriptRan=true;try{parent.__escaped=true}catch{}</script><p onclick="window.__eventRan=true">This is an original verification dictionary, not a commercial definition.</p>"##,
    );
    for number in 1..=28 {
        article.push_str(&format!("<p>Example {number}. An apple a day is a familiar English expression. 这是用于检验滚动位置与字体的原创示例。</p>"));
    }
    article.push_str("<h3 id=\"examples\">Examples</h3><p>She picked an apple from the tree.</p>");
    std::fs::write(alpha.join("reference.mdx"), Corpus::articles(vec![
        Row::new("apple", article.as_bytes()),
        Row::new("apple", b"<p>A second, distinct sense: an apple-shaped object.</p>"),
        Row::new("apples", b"@@@LINK=apple"),
        Row::new("apricot", b"<h2>apricot</h2><p>A small orange stone fruit.</p>"),
        Row::new("banana", b"<h2>banana</h2><p>A long curved fruit with a yellow skin.</p><a href=\"entry://apple#examples\">apple examples</a>"),
        Row::new("café", b"<p>A place serving coffee.</p>"),
        Row::new("猫", "<h2>猫</h2><p>一种常见的家养动物。</p>".as_bytes()),
        Row::new("broken", b"@@@LINK=missing"),
        Row::new("cycle", b"@@@LINK=cycle"),
    ]).serialize()?)?;
    std::fs::write(beta.join("usage.mdx"), Corpus::articles(vec![
        Row::new("apple", b"<h2>apple in use</h2><p>apple pie; an apple orchard; the apple of someone's eye.</p><a href=\"entry://orchard\">orchard</a>"),
        Row::new("banana", b"<p>banana bread; a bunch of bananas.</p>"),
        Row::new("orchard", b"<p>An area of land planted with fruit trees.</p>"),
        Row::new("苹果", "<p>apple，常见水果，用于验证中文全文索引。</p>".as_bytes()),
    ]).serialize()?)?;
    let png = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5,
        1, 1, 39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    std::fs::write(
        alpha.join("reference.2.mdd"),
        Corpus::resources(vec![
            Row::new("\\icon.png", png.as_slice()),
            Row::new(
                "\\volume-only.txt",
                b"Resource in an explicitly mounted gapped volume.",
            ),
        ])
        .serialize()?,
    )?;
    let sound: Vec<u8> = {
        let rate = 8000u32;
        let samples = 1600u32;
        let size = samples * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&size.to_le_bytes());
        for index in 0..samples {
            let sample =
                ((index as f32 * std::f32::consts::TAU * 440. / rate as f32).sin() * 2000.) as i16;
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    };
    std::fs::create_dir_all(alpha.join("media"))?;
    std::fs::write(alpha.join("media/tone.wav"), sound)?;
    std::fs::write(
        alpha.join("assets/style.css"),
        "@font-face { font-family: Fixture; src: url('./fixture.woff2') format('woff2'); } .definition { color: #315d45; } body {font-family: Fixture,sans-serif} p {margin: 0 0 18px} .phonetic {font-family: sans-serif; color: #647064}",
    )?;
    std::fs::write(
        alpha.join("assets/enhance.js"),
        "window.__mountedScriptRan = true;",
    )?;
    if let Some(font) = args.next() {
        std::fs::copy(font, alpha.join("assets/fixture.woff2"))?;
    }
    println!(
        "Created original real MDX/MDD corpus at {}. Import it explicitly using the UI or CLI.",
        output.display()
    );
    Ok(())
}
