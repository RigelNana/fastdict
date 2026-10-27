use std::cell::Cell;

use lol_html::{element, rewrite_str, RewriteStrSettings};
use fastdict::render::{render_document, RenderOptions};

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
