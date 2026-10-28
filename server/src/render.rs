use anyhow::{Context, Result};
use cssparser::{Parser, ParserInput, ToCss, Token};
use lol_html::{RewriteStrSettings, element, rewrite_str, text};
use std::cell::RefCell;
use url::Url;

/// asset_base is an absolute, expiring, dictionary/revision-scoped HTTP capability.
/// It must never contain the application's bearer token.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub asset_base: String,
    pub trust_scripts: bool,
    pub custom_css: String,
    pub font_family: String,
    pub typography_mode: String,
    pub fonts: Vec<(String, String)>,
    pub stylesheets: Vec<String>,
    pub scripts: Vec<String>,
}

fn resource_url(value: &str, base: &str, resource_path: &str) -> Option<String> {
    let value = value.trim().replace('\\', "/");
    if value.starts_with('#') {
        return Some(value);
    }
    if value.is_empty() || value.starts_with("//") {
        return None;
    }
    if let Ok(parsed) = Url::parse(&value) {
        // Only inert raster/font/audio data are permitted; never SVG or HTML data.
        let lower = value.to_ascii_lowercase();
        if parsed.scheme() == "data"
            && [
                "data:image/png;",
                "data:image/jpeg;",
                "data:image/gif;",
                "data:image/webp;",
                "data:font/",
                "data:audio/",
            ]
            .iter()
            .any(|p| lower.starts_with(p))
        {
            return Some(value);
        }
        return None;
    }
    let root = Url::parse("https://dictionary.invalid/").ok()?;
    let current = root.join(resource_path.trim_start_matches('/')).ok()?;
    let resolved = current.join(&value).ok()?;
    if resolved.host_str() != Some("dictionary.invalid") {
        return None;
    }
    let mut output = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        resolved.path().trim_start_matches('/')
    );
    if let Some(q) = resolved.query() {
        output.push('?');
        output.push_str(q);
    }
    if let Some(f) = resolved.fragment() {
        output.push('#');
        output.push_str(f);
    }
    Some(output)
}

fn css_string(value: &str) -> String {
    let mut out = String::new();
    cssparser::serialize_string(value, &mut out).expect("String formatting cannot fail");
    out
}

fn css_tokens(parser: &mut Parser<'_, '_>, base: &str, path: &str) -> String {
    let mut out = String::new();
    let mut import = false;
    while let Ok(token) = parser.next_including_whitespace_and_comments().cloned() {
        match &token {
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => {
                import = true;
                out.push_str("@import");
            }
            Token::QuotedString(value) if import => {
                out.push_str(&css_string(
                    &resource_url(value, base, path).unwrap_or_else(|| "about:blank".into()),
                ));
                import = false;
            }
            Token::UnquotedUrl(value) => {
                out.push_str("url(");
                out.push_str(&css_string(
                    &resource_url(value, base, path).unwrap_or_else(|| "about:blank".into()),
                ));
                out.push(')');
                import = false;
            }
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                let value = parser
                    .parse_nested_block(|nested| {
                        let value = nested.expect_url_or_string()?.to_string();
                        nested.expect_exhausted()?;
                        Ok::<_, cssparser::ParseError<'_, ()>>(value)
                    })
                    .ok();
                out.push_str("url(");
                out.push_str(&css_string(
                    &value
                        .and_then(|v| resource_url(&v, base, path))
                        .unwrap_or_else(|| "about:blank".into()),
                ));
                out.push(')');
                import = false;
            }
            Token::Function(name) => {
                let nested = parser
                    .parse_nested_block(|p| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(css_tokens(p, base, path))
                    })
                    .unwrap_or_default();
                if !name.eq_ignore_ascii_case("expression") {
                    out.push_str(&token.to_css_string());
                    out.push_str(&nested);
                    out.push(')');
                }
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let end = match &token {
                    Token::ParenthesisBlock => ')',
                    Token::SquareBracketBlock => ']',
                    _ => '}',
                };
                out.push_str(&token.to_css_string());
                let nested = parser
                    .parse_nested_block(|p| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(css_tokens(p, base, path))
                    })
                    .unwrap_or_default();
                out.push_str(&nested);
                out.push(end);
            }
            Token::Semicolon => {
                import = false;
                out.push(';');
            }
            _ => out.push_str(&token.to_css_string()),
        }
    }
    out
}

/// Rewrite a loaded CSS file with its original virtual path, not just its basename.
pub fn rewrite_css(css: &str, asset_base: &str, resource_path: &str) -> String {
    let mut input = ParserInput::new(css);
    css_tokens(&mut Parser::new(&mut input), asset_base, resource_path)
}

/// HTML parser-backed extraction used by full-text indexing and CLI text output.
pub fn plain_text(html: &str) -> String {
    let separated = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!(
                "p,div,br,li,td,th,tr,section,article,h1,h2,h3,h4,h5,h6,dt,dd,blockquote,hr",
                |el| {
                    el.before(" ", lol_html::html_content::ContentType::Text);
                    el.after(" ", lol_html::html_content::ContentType::Text);
                    Ok(())
                }
            )],
            ..RewriteStrSettings::default()
        },
    )
    .unwrap_or_else(|_| html.to_owned());
    let cleaned = ammonia::Builder::default()
        .tags(std::collections::HashSet::new())
        .clean(&separated)
        .to_string();
    html_escape::decode_html_entities(&cleaned)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn rewrite_srcset(value: &str, base: &str) -> String {
    // srcset's URL token can contain commas (notably data URLs); separators follow descriptors.
    let mut items = Vec::new();
    let mut input = value.trim();
    while !input.is_empty() {
        input = input.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if input.is_empty() {
            break;
        }
        let end = input.find(char::is_whitespace).unwrap_or(input.len());
        let raw = &input[..end];
        input = &input[end..];
        let comma_ended = raw.ends_with(',');
        let url = raw.trim_end_matches(',');
        let descriptor = if comma_ended {
            ""
        } else {
            let end = input.find(',').unwrap_or(input.len());
            let descriptor = input[..end].trim();
            input = &input[end..];
            descriptor
        };
        if let Some(url) = resource_url(url, base, "") {
            if descriptor.split_whitespace().all(|s| {
                s.chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'w' | 'x' | 'h'))
            }) {
                items.push(if descriptor.is_empty() {
                    url
                } else {
                    format!("{url} {descriptor}")
                });
            }
        }
    }
    items.join(", ")
}

pub fn render_document(raw: &str, options: &RenderOptions) -> Result<String> {
    render_document_with_styles(raw, options, |_| None)
}

/// Inline bounded local CSS in its original cascade position. Missing, oversized
/// or alternate stylesheets keep their normal resource URL and loading behavior.
pub fn render_document_with_styles(
    raw: &str,
    options: &RenderOptions,
    mut load_style: impl FnMut(&str) -> Option<String>,
) -> Result<String> {
    let base =
        Url::parse(&options.asset_base).context("Content asset_base must be an absolute URL")?;
    anyhow::ensure!(
        matches!(base.scheme(), "http" | "https")
            && base.username().is_empty()
            && base.password().is_none()
            && base.query().is_none()
            && base.fragment().is_none(),
        "Invalid content capability URL"
    );
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let css_text = RefCell::new(String::new());
    let referenced_styles = RefCell::new(std::collections::HashSet::new());
    let referenced_scripts = RefCell::new(std::collections::HashSet::new());
    let script_preloads = RefCell::new(Vec::new());
    let mut style_bytes = 0;
    let mut style_cache = std::collections::HashMap::<String, Option<String>>::new();
    let mut inline_style = |url: &str| -> Option<String> {
        let parsed = Url::parse(url).ok()?;
        let encoded = parsed.path().strip_prefix(base.path())?;
        let path = percent_encoding::percent_decode_str(encoded).decode_utf8().ok()?;
        if let Some(cached) = style_cache.get(path.as_ref()) { return cached.clone(); }
        let css = if style_cache.len() < 16 && style_bytes < 512 * 1024 {
            load_style(&path).filter(|css| css.len() <= 256 * 1024 && style_bytes + css.len() <= 512 * 1024)
                .and_then(|css| {
                    let rewritten = rewrite_css(&css, &options.asset_base, &path).replace('<', "\\3c ");
                    if style_bytes + rewritten.len() > 512 * 1024 { return None; }
                    style_bytes += rewritten.len();
                    Some(rewritten)
                })
        } else { None };
        style_cache.insert(path.into_owned(), css.clone());
        css
    };
    let body = rewrite_str(
        raw,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("*", |el| {
                    let tag = el.tag_name().to_ascii_lowercase();
                    if matches!(
                        tag.as_str(),
                        "base"
                            | "meta"
                            | "iframe"
                            | "frame"
                            | "frameset"
                            | "object"
                            | "embed"
                            | "applet"
                            | "portal"
                            | "noscript"
                    ) || (!options.trust_scripts && tag == "script")
                    {
                        el.remove();
                        return Ok(());
                    }
                    if matches!(tag.as_str(), "html" | "head" | "body" | "form") {
                        el.remove_and_keep_content();
                        return Ok(());
                    }
                    let attrs: Vec<(String, String)> = el
                        .attributes()
                        .iter()
                        .map(|a| {
                            (
                                a.name(),
                                html_escape::decode_html_entities(&a.value()).into_owned(),
                            )
                        })
                        .collect();
                    for (name, value) in attrs {
                        let lower = name.to_ascii_lowercase();
                        if lower.starts_with("on")
                            || matches!(
                                lower.as_str(),
                                "nonce"
                                    | "integrity"
                                    | "crossorigin"
                                    | "srcdoc"
                                    | "action"
                                    | "formaction"
                                    | "target"
                                    | "ping"
                                    | "download"
                                    | "autofocus"
                                    | "http-equiv"
                                    | "is"
                            )
                        {
                            if !(options.trust_scripts && lower.starts_with("on")) {
                                el.remove_attribute(&name);
                            }
                        } else if lower == "style" {
                            el.set_attribute(
                                &name,
                                &html_escape::encode_double_quoted_attribute(&rewrite_css(
                                    &value,
                                    &options.asset_base,
                                    "",
                                )),
                            )?;
                        } else if lower == "srcset" || lower == "imagesrcset" {
                            el.set_attribute(
                                &name,
                                &html_escape::encode_double_quoted_attribute(&rewrite_srcset(
                                    &value,
                                    &options.asset_base,
                                )),
                            )?;
                        } else if matches!(
                            lower.as_str(),
                            "src"
                                | "href"
                                | "xlink:href"
                                | "poster"
                                | "background"
                                | "data"
                                | "cite"
                        ) {
                            if matches!(tag.as_str(), "a" | "area")
                                && matches!(lower.as_str(), "href" | "xlink:href")
                            {
                                let trimmed = value.trim();
                                let lower_value = trimmed.to_ascii_lowercase();
                                if lower_value.starts_with("sound://") {
                                    if let Some(url) =
                                        resource_url(&trimmed[8..], &options.asset_base, "")
                                    {
                                        el.set_attribute(
                                            "data-fastdict-sound",
                                            &html_escape::encode_double_quoted_attribute(&url),
                                        )?;
                                    }
                                    el.set_attribute(&name, "#")?;
                                } else if lower_value.starts_with("entry://")
                                    || trimmed.starts_with('#')
                                    || Url::parse(trimmed).is_ok_and(|u| {
                                        matches!(u.scheme(), "https" | "http" | "mailto")
                                    })
                                {
                                    el.set_attribute(
                                        "data-fastdict-link",
                                        &html_escape::encode_double_quoted_attribute(trimmed),
                                    )?;
                                    el.set_attribute(&name, "#")?;
                                } else {
                                    el.set_attribute(&name, "#")?;
                                }
                            } else if let Some(url) = resource_url(&value, &options.asset_base, "")
                            {
                                let identity = url.split(['?', '#']).next().unwrap_or(&url).to_owned();
                                if tag == "link" && el.get_attribute("rel").is_some_and(|rel| rel.split_whitespace().any(|part| part.eq_ignore_ascii_case("stylesheet"))) {
                                    referenced_styles.borrow_mut().insert(identity.clone());
                                }
                                if tag == "script" {
                                    if referenced_scripts.borrow_mut().insert(identity) {
                                        script_preloads.borrow_mut().push(url.clone());
                                    }
                                }
                                el.set_attribute(
                                    &name,
                                    &html_escape::encode_double_quoted_attribute(&url),
                                )?;
                            } else {
                                el.remove_attribute(&name);
                            }
                        }
                    }
                    // Dictionary content cannot replace the trusted bridge's bookkeeping.
                    if let Some(id) = el.get_attribute("id") {
                        if html_escape::decode_html_entities(&id).starts_with("fastdict-") {
                            el.remove_attribute("id");
                        }
                    }
                    if options.trust_scripts && tag == "script" {
                        el.set_attribute("nonce", &nonce)?;
                    }
                    if tag == "link" && el.get_attribute("rel").is_some_and(|rel| rel.eq_ignore_ascii_case("stylesheet"))
                        && !el.has_attribute("disabled") && !el.has_attribute("title")
                    {
                        if let Some(css) = el.get_attribute("href")
                            .map(|url| html_escape::decode_html_entities(&url).into_owned())
                            .and_then(|url| inline_style(&url))
                        {
                            let attrs = ["media", "id", "class"].iter().filter_map(|name| {
                                el.get_attribute(name).map(|value| format!(" {name}=\"{}\"", html_escape::encode_double_quoted_attribute(&html_escape::decode_html_entities(&value))))
                            }).collect::<String>();
                            el.replace(&format!("<style{attrs}>{css}</style>"), lol_html::html_content::ContentType::Html);
                        }
                    }
                    Ok(())
                }),
                text!("style", move |text| {
                    let mut buffer = css_text.borrow_mut();
                    buffer.push_str(text.as_str());
                    if text.last_in_text_node() {
                        text.set_str(rewrite_css(&buffer, &options.asset_base, "").replace('<', "\\3c "));
                        buffer.clear();
                    } else {
                        text.remove();
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::default()
        },
    )
    .context("Rewriting dictionary HTML")?;
    let source = format!(
        "{}{}/",
        base.origin().ascii_serialization(),
        base.path().trim_end_matches('/')
    );
    let scripts = if options.trust_scripts {
        format!("'nonce-{nonce}' {source}")
    } else {
        format!("'nonce-{nonce}'")
    };
    let handlers = if options.trust_scripts { "'unsafe-inline'" } else { "'none'" };
    let csp = format!(
        "default-src 'none'; script-src {scripts}; script-src-attr {handlers}; style-src 'unsafe-inline' {source}; img-src {source} data:; font-src {source} data:; media-src {source} data:; connect-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; form-action 'none'; base-uri 'none'; manifest-src 'none'"
    );
    let csp = csp.replace('&', "&amp;").replace('"', "&quot;");
    let custom = rewrite_css(&options.custom_css, &options.asset_base, "").replace('<', "\\3c ");
    let font_rule = if options.typography_mode != "original" && !options.font_family.trim().is_empty() {
        format!("font-family:{};", css_string(&options.font_family).replace('<', "\\3c "))
    } else { String::new() };
    let mut mounts = String::new();
    for key in &options.stylesheets {
        if let Some(url) = resource_url(key, &options.asset_base, "") {
            if !referenced_styles.borrow_mut().insert(url.split(['?', '#']).next().unwrap_or(&url).to_owned()) {
                continue;
            }
            if let Some(css) = inline_style(&url) {
                mounts.push_str(&format!("<style>{css}</style>"));
            } else {
                mounts.push_str(&format!(
                    "<link rel=\"stylesheet\" href=\"{}\">",
                    html_escape::encode_double_quoted_attribute(&url)
                ));
            }
        }
    }
    let mut font_faces = String::new();
    for (alias, key) in &options.fonts {
        if let Some(url) = resource_url(key, &options.asset_base, "") {
            font_faces.push_str(&format!(
                "@font-face{{font-family:{};src:url({});font-display:swap;}}",
                css_string(alias),
                css_string(&url)
            ));
        }
    }
    let font_faces = font_faces.replace('<', "\\3c ");
    let typography = match options.typography_mode.as_str() {
        "original" => "original",
        "override" => "override",
        _ => "global",
    };
    let mut mounted_scripts = String::new();
    if options.trust_scripts {
        for key in &options.scripts {
            if let Some(url) = resource_url(key, &options.asset_base, "") {
                if !referenced_scripts.borrow_mut().insert(url.split(['?', '#']).next().unwrap_or(&url).to_owned()) {
                    continue;
                }
                mounted_scripts.push_str(&format!(
                    "<script src=\"{}\"></script>",
                    html_escape::encode_double_quoted_attribute(&url)
                ));
                script_preloads.borrow_mut().push(url);
            }
        }
    }
    let font_attribute = html_escape::encode_double_quoted_attribute(&options.font_family);
    let compatibility = if options.trust_scripts {
        format!(r#"<script type="application/json" id="fastdict-script-storage">{{}}</script><script nonce="{nonce}">{SCRIPT_STORAGE}</script>"#)
    } else { String::new() };
    let asset_attribute = html_escape::encode_double_quoted_attribute(&options.asset_base);
    let preloads = script_preloads.into_inner().iter().map(|url| format!(
        "<link rel=\"preload\" as=\"script\" href=\"{}\">", html_escape::encode_double_quoted_attribute(url)
    )).collect::<String>();
    Ok(format!(
        r#"<!doctype html><html data-fastdict-assets="{asset_attribute}" style="background:transparent!important;overflow:hidden!important;color-scheme:light!important"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="{csp}"><meta name="viewport" content="width=device-width,initial-scale=1"><script type="application/json" id="fastdict-initial-style">{{}}</script><script nonce="{nonce}">{STYLE_LOAD}</script>{preloads}{compatibility}{mounts}<style>{font_faces}body{{color:#18181b;overflow-wrap:anywhere;{font_rule}}}img,video,svg{{max-width:100%}}</style></head><body data-fastdict-typography="{typography}" data-fastdict-font="{font_attribute}" style="background:transparent!important;overflow:hidden!important"><main id="fastdict-content">{body}</main><style>{custom}
html,body{{margin:0;padding:0;background:transparent!important;overflow:hidden!important;min-height:0!important;height:auto!important}}
#fastdict-content{{display:flow-root;min-width:0}}
html[data-fastdict-theme=dark]:not([data-fastdict-theme-ready]) #fastdict-content{{visibility:hidden}}
body[data-reading=true] #fastdict-content :where(p,li,dd){{line-height:inherit}}
body[data-reading=true] #fastdict-content :where(pre){{white-space:pre-wrap}}
body[data-reading=true] #fastdict-content :where(table){{max-width:100%;table-layout:fixed}}
body[data-fastdict-typography=override] #fastdict-content :where(p,li,dd,dt){{font-family:inherit!important;font-size:inherit!important;line-height:inherit!important}}
</style><script nonce="{nonce}">{BRIDGE}</script>{mounted_scripts}</body></html>"#
    ))
}

// Capture failures before the body bridge exists, so missing companion CSS
// does not leave the first display waiting for an event that already happened.
const STYLE_LOAD: &str = r#"for(const event of ['load','error'])document.addEventListener(event,e=>{if(e.target?.matches?.('link[rel~=stylesheet]'))e.target.dataset.fastdictLoaded=event;},true);"#;

// Compatibility storage is installed before dictionary scripts execute. It is
// bounded, dictionary-scoped by the parent and never exposes application storage.
const SCRIPT_STORAGE: &str = r#"(()=>{'use strict';
const seed=document.getElementById('fastdict-script-storage');let initial={};
try{initial=JSON.parse(seed?.textContent||'{}');}catch{}seed?.remove();
const notify=(area,method,key,value)=>parent.postMessage({type:'fastdict:storage',area,method,key,value},'*');
for(const area of ['local','session']){
 const values=Object.create(null);const saved=initial[area];
 if(saved&&typeof saved==='object')for(const [key,value] of Object.entries(saved))if(typeof value==='string')values[key]=value;
 const setItem=(key,value)=>{
  key=String(key);value=String(value);const next=Object.assign(Object.create(null),values,{[key]:value});
  if(key.length>1024||value.length>65536||Object.keys(next).length>512||JSON.stringify(next).length>262144)throw new DOMException('Dictionary storage limit exceeded','QuotaExceededError');
  values[key]=value;notify(area,'setItem',key,value);
 };
 const removeItem=key=>{key=String(key);delete values[key];notify(area,'removeItem',key);};
 const methods={getItem:key=>values[String(key)]??null,setItem,removeItem,
  clear:()=>{for(const key of Object.keys(values))delete values[key];notify(area,'clear');},
  key:index=>Object.keys(values)[Number(index)>>>0]??null};
 const storage=new Proxy(Object.create(null),{
  get:(_,key)=>key==='length'?Object.keys(values).length:key===Symbol.toStringTag?'Storage':Object.hasOwn(methods,key)?methods[key]:values[key],
  set:(_,key,value)=>{setItem(key,value);return true;},deleteProperty:(_,key)=>{removeItem(key);return true;},
  ownKeys:()=>Object.keys(values),has:(_,key)=>Object.hasOwn(values,key)||Object.hasOwn(methods,key)||key==='length',
  getOwnPropertyDescriptor:(_,key)=>Object.hasOwn(values,key)?{configurable:true,enumerable:true,writable:true,value:values[key]}:undefined,
 });
 Object.defineProperty(window,area+'Storage',{value:storage,configurable:false});
}
})();"#;

const BRIDGE: &str = r#"(()=>{'use strict';
const root=document.getElementById('fastdict-content');let pending=false,last=0,audio=null;
let domReady=false,stylesReady=false,fontsReady=false,revealed=false,readyPending=false;
// Hidden iframes can throttle animation frames. Bound only the initial layout
// checkpoint; normal reading/resize measurements still use animation frames.
const layoutTurn=callback=>{
 let called=false;const run=()=>{if(called)return;called=true;clearTimeout(timer);callback();};
 const timer=setTimeout(run,32);requestAnimationFrame(run);
};
const height=()=>Math.max(1,Math.min(200000,Math.ceil(Math.max(root.getBoundingClientRect().height,root.scrollHeight))));
const canReveal=()=>domReady&&stylesReady&&fontsReady&&(document.documentElement.dataset.fastdictTheme!=='dark'||document.documentElement.dataset.fastdictThemeReady==='true');
const revealArticle=()=>{
 if(revealed||readyPending||!canReveal())return;readyPending=true;
 layoutTurn(()=>layoutTurn(()=>{
  readyPending=false;if(!canReveal())return;revealed=true;
  send({type:'fastdict:ready',height:height()});
 }));
};
const nonce=document.currentScript?.nonce;let themeLoading=false,themeRequestId=0;const themeRequests=new Map();
const send=(message)=>parent.postMessage(message,'*');
const measure=()=>{pending=false;const next=height();if(next!==last){last=next;send({type:'fastdict:height',height:next});}};
const schedule=()=>{if(!pending){pending=true;requestAnimationFrame(measure);}};
const revealTheme=()=>layoutTurn(()=>layoutTurn(()=>{
 if(![...themeRequests.values()].some(request=>request.critical)){document.documentElement.dataset.fastdictThemeReady='true';schedule();revealArticle();}
}));
const applyTheme=()=>{
 const reader=globalThis.DarkReader;if(!reader){revealArticle();return;}
 if(document.documentElement.dataset.fastdictTheme==='dark'){
  if(!reader.isEnabled())reader.enable({brightness:100,contrast:100,sepia:0,darkSchemeBackgroundColor:'#09090b',darkSchemeTextColor:'#fafafa',immediateModify:true},{disableStyleSheetsProxy:true,disableCustomElementRegistryProxy:true});
 }else if(reader.isEnabled())reader.disable();
 revealTheme();
};
const loadTheme=(url)=>{
 if(globalThis.DarkReader){applyTheme();return;}if(themeLoading||typeof url!=='string')return;
 try{const parsed=new URL(url);if(!['http:','https:'].includes(parsed.protocol))return;}catch{return;}
 themeLoading=true;const script=document.createElement('script');script.nonce=nonce;script.src=url;
 const failed=()=>{themeLoading=false;document.documentElement.dataset.fastdictThemeReady='true';document.body.style.setProperty('background','#fff','important');send({type:'fastdict:error',message:'词典深色适配加载失败，请重新打开词条。'});revealArticle();};
 const runtimeTimer=setTimeout(failed,8000);
 script.onload=()=>{clearTimeout(runtimeTimer);themeLoading=false;const fetchResource=(url,critical=/\.css(?:[?#]|$)/i.test(url))=>new Promise((resolve,reject)=>{
  const requestId=++themeRequestId;const timer=setTimeout(()=>{themeRequests.delete(requestId);reject(new Error('Theme resource timeout'));revealTheme();},10000);
  if(critical&&!revealed)delete document.documentElement.dataset.fastdictThemeReady;
  themeRequests.set(requestId,{resolve,reject,timer,critical});send({type:'fastdict:theme-resource',requestId,url});
 });
 globalThis.DarkReader.setFetchMethod(url=>fetchResource(url));
 // The parent checks the article capability before reading localhost or cross-origin resources.
 // Dark Reader's host fetch hook supports opaque srcdoc origins without relaxing the iframe CSP.
 globalThis.DarkReader.Plugins={fetch:async request=>{
  const response=await fetchResource(request.url,request.responseType!=='data-url');
  if(request.responseType==='data-url'){
   const blob=await response.blob();return await new Promise((resolve,reject)=>{
    const reader=new FileReader();reader.onload=()=>resolve(reader.result);reader.onerror=()=>reject(reader.error);reader.readAsDataURL(blob);
   });
  }
  return await response.text();
 }};applyTheme();};
 script.onerror=()=>{clearTimeout(runtimeTimer);failed();};
 document.head.append(script);
};
new ResizeObserver(schedule).observe(root);addEventListener('load',schedule);document.fonts?.ready.then(schedule);
const decode=(value)=>{try{return decodeURIComponent(value);}catch{return value;}};
const anchor=(id)=>{if(typeof id!=='string'||id.length>4096)return;id=decode(id.replace(/^#/,''));const target=id?(document.getElementById(id)||document.getElementsByName(id)[0]):root;if(target)send({type:'fastdict:anchor',offset:Math.max(0,Math.min(200000,target.getBoundingClientRect().top+scrollY))});};
document.addEventListener('click',(event)=>{
 const link=event.target.closest?.('a,area');if(!link||event.defaultPrevented)return;event.preventDefault();
 const sound=link.getAttribute('data-fastdict-sound');if(sound){
  if(!audio){audio=new Audio();audio.id='fastdict-audio';audio.hidden=true;document.body.append(audio);}
  audio.pause();audio.src=sound;audio.play().catch(error=>{if(error.name!=='AbortError')send({type:'fastdict:error',message:'音频无法播放。资源可能缺失或链接已过期，请重新打开词条。'});});return;
 }
 const href=link.getAttribute('data-fastdict-link')||link.getAttribute('href')||'';if(!href||href==='#'||href==='##')return;if(href.startsWith('#')){anchor(href);return;}
 if(/^entry:\/\//i.test(href)){const value=href.slice(8),split=value.indexOf('#'),word=decode(split<0?value:value.slice(0,split));if(word&&word.length<=4096)send({type:'fastdict:navigate',word,...(split<0?{}:{anchor:decode(value.slice(split+1)).slice(0,4096)})});return;}
 try{const url=new URL(href);if(['http:','https:','mailto:'].includes(url.protocol)&&href.length<=8192)send({type:'fastdict:external',url:href});}catch{}
});
document.addEventListener('submit',event=>event.preventDefault(),true);
document.addEventListener('pointerdown',()=>send({type:'fastdict:focus'}),true);
const applyStyle=s=>{
 const body=document.body;if(s.theme==='dark'||s.theme==='light'){
  document.documentElement.dataset.fastdictTheme=s.theme;
  if(s.theme==='dark')loadTheme(s.themeRuntimeUrl);else applyTheme();
 }
 if(body.dataset.fastdictTypography!=='original'){
  if(Number.isFinite(s.font_size))body.style.fontSize=Math.max(10,Math.min(48,s.font_size))+'px';
  if(Number.isFinite(s.line_height))body.style.lineHeight=String(Math.max(1,Math.min(3,s.line_height)));
  if(typeof s.font_family==='string'&&s.font_family.length<=256){const family=body.dataset.fastdictFont||s.font_family;if(family)body.style.fontFamily=family;else body.style.removeProperty('font-family');}
 }
 body.dataset.reading=String(s.reading_mode===true);schedule();revealArticle();
};
addEventListener('message',event=>{
 if(event.source!==parent)return;const s=event.data;if(!s)return;
 if(s.type==='fastdict:theme-resource-result'){
  const request=themeRequests.get(s.requestId);if(!request)return;themeRequests.delete(s.requestId);clearTimeout(request.timer);
  if(s.error||!(s.body instanceof ArrayBuffer))request.reject(new Error('Theme resource unavailable'));
  else request.resolve(new Response(s.body,{headers:{'Content-Type':s.contentType}}));revealTheme();return;
 }
 if(s.type==='fastdict:clear-selection'){getSelection()?.removeAllRanges();return;}
 if(s.type==='fastdict:anchor'){anchor(s.anchor);return;}if(s.type!=='fastdict:style')return;
 applyStyle(s);
});
// Begin from the embedded settings, without waiting for images or iframe load.
const initial=document.getElementById('fastdict-initial-style');
if(initial?.textContent){
 try{applyStyle(JSON.parse(initial.textContent));}catch{}initial.remove();
 const prepare=()=>{
  domReady=true;
  const styles=[...document.querySelectorAll('link[rel~=stylesheet]:not([disabled])')];
  const waitForStyle=link=>new Promise(resolve=>{
   if(link.sheet||link.dataset?.fastdictLoaded){resolve();return;}
   const done=()=>{clearTimeout(timer);link.removeEventListener('load',done);link.removeEventListener('error',done);resolve();};
   const timer=setTimeout(done,5000);link.addEventListener('load',done,{once:true});link.addEventListener('error',done,{once:true});
  });
  Promise.all(styles.map(waitForStyle)).then(()=>{
   stylesReady=true;
   const done=()=>{clearTimeout(timer);fontsReady=true;schedule();revealArticle();};
   const timer=setTimeout(done,700);if(document.fonts)document.fonts.ready.then(done,done);else done();
  });
 };
 if(document.readyState==='loading')addEventListener('DOMContentLoaded',prepare,{once:true});else prepare();
}
schedule();
})();"#;
