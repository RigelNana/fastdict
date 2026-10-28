use std::cell::Cell;

use lol_html::{element, rewrite_str, RewriteStrSettings};
use fastdict::render::{render_document, render_document_with_styles, RenderOptions};

#[test]
fn safe_mode_css_cannot_create_navigation_elements() {
    let options = RenderOptions {
        asset_base: "http://127.0.0.1:8791/api/v1/content/dictionary/revision/capability/".into(),
        trust_scripts: false,
        custom_css: String::new(),
        font_family: String::new(),
        typography_mode: "global".into(),
        fonts: Vec::new(),
        stylesheets: Vec::new(),
        scripts: Vec::new(),
    };
    let article = r#"<style>p::before { content: "\3c /style>\3c meta http-equiv='refresh' content='0;url=http://127.0.0.1:8791/probe-external'>"; }</style><p>Definition.</p>"#;
    let document = render_document(article, &options).unwrap();
    let refresh = Cell::new(false);
    rewrite_str(&document, RewriteStrSettings {
        element_content_handlers: vec![element!("meta[http-equiv]", |element| {
            if element.get_attribute("http-equiv").is_some_and(|value| value.eq_ignore_ascii_case("refresh")) {
                refresh.set(true);
            }
            Ok(())
        })],
        ..RewriteStrSettings::default()
    }).unwrap();
    assert!(!refresh.get(), "CSS rewriting created a live refresh element after sanitization");
}

#[test]
fn script_compatibility_is_authorized_early_and_preserves_css_fonts() {
    let mut options = RenderOptions {
        asset_base: "https://dictionary.example/api/v1/content/d/r/c/".into(),
        trust_scripts: true, custom_css: String::new(), font_family: String::new(),
        typography_mode: "global".into(), fonts: vec![], stylesheets: vec![], scripts: vec![],
    };
    let raw = r#"<style>body {font-family:Bookerly}</style><p onclick="this.textContent='clicked'">Definition</p><script nonce="untrusted">localStorage.getItem('setting')</script>"#;
    let html = render_document(raw, &options).unwrap();
    assert!(html.find("Object.defineProperty(window,area+'Storage'").unwrap() < html.find("localStorage.getItem('setting')").unwrap());
    assert!(html.contains("script-src 'nonce-"));
    assert!(html.contains("script-src-attr 'unsafe-inline'"));
    assert!(!html.contains("nonce=\"untrusted\""));
    assert!(!html.contains("font-family:\"\""));
    assert!(html.contains("font-family:Bookerly"));
    assert!(html.contains("connect-src 'none'"));
    assert!(html.contains("base-uri 'none'"));

    options.trust_scripts = false;
    let safe = render_document(raw, &options).unwrap();
    assert!(!safe.contains("localStorage.getItem('setting')"));
    assert!(!safe.contains("Object.defineProperty(window,area+'Storage'"));
    assert!(!safe.contains("onclick="));
    assert!(safe.contains("script-src-attr 'none'"));
}

#[test]
fn critical_css_is_inlined_in_place_with_media_paths_and_cascade_preserved() {
    let options = RenderOptions {
        asset_base: "https://dictionary.example/api/v1/content/d/r/c/".into(),
        trust_scripts: false, custom_css: String::new(), font_family: String::new(),
        typography_mode: "global".into(), fonts: vec![],
        stylesheets: vec!["styles/main.css".into(), "extra.css".into()], scripts: vec![],
    };
    let html = render_document_with_styles(
        r#"<style>p{color:red}</style><link rel="stylesheet" href="styles/main.css" media="screen"><style>p{color:blue}</style><p>Definition</p>"#,
        &options,
        |path| match path {
            "styles/main.css" => Some("p{background:url(../image.png);color:green}p::before{content:'</style><script>bad()</script>'}".into()),
            "extra.css" => Some("p{margin:0}".into()),
            _ => None,
        },
    ).unwrap();
    assert!(!html.contains("rel=\"stylesheet\""));
    assert!(html.contains("<style media=\"screen\">"));
    assert!(html.contains("https://dictionary.example/api/v1/content/d/r/c/image.png"));
    assert!(html.find("color:red").unwrap() < html.find("color:green").unwrap());
    assert!(html.find("color:green").unwrap() < html.find("color:blue").unwrap());
    assert_eq!(html.matches("color:green").count(), 1);
    assert!(html.find("p{margin:0}").unwrap() < html.find("<body").unwrap());
    assert!(!html.contains("<script>bad()"));
}

#[test]
fn oversized_missing_and_alternate_css_keep_their_links_and_scripts_preload_once() {
    let options = RenderOptions {
        asset_base: "https://dictionary.example/api/v1/content/d/r/c/".into(),
        trust_scripts: true, custom_css: String::new(), font_family: String::new(),
        typography_mode: "global".into(), fonts: vec![], stylesheets: vec!["missing.css".into()],
        scripts: vec!["first.js".into(), "second.js".into()],
    };
    let html = render_document_with_styles(
        r#"<link rel="stylesheet" href="large.css"><link rel="alternate stylesheet" title="Other" href="alternate.css"><script src="first.js"></script>"#,
        &options,
        |path| if path == "large.css" { Some("a".repeat(256 * 1024 + 1)) } else { None },
    ).unwrap();
    assert!(html.contains("large.css\""));
    assert!(html.contains("missing.css\""));
    assert!(html.contains("rel=\"alternate stylesheet\""));
    assert_eq!(html.matches("rel=\"preload\" as=\"script\"").count(), 2);
    assert!(html.find("as=\"script\"").unwrap() < html.find("<body").unwrap());
    assert!(html.find("<script src=\"https://dictionary.example/api/v1/content/d/r/c/first.js").unwrap() < html.find("<script src=\"https://dictionary.example/api/v1/content/d/r/c/second.js").unwrap());
}
