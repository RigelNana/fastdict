use fastdict::api::{serve, ServeConfig};
use reqwest::{Client, multipart::{Form, Part}};
use serde_json::{Value, json};
use std::time::Duration;

async fn json(response: reqwest::Response) -> Value {
    let status=response.status();let body=response.text().await.unwrap();
    assert!(status.is_success(),"{status}: {body}");serde_json::from_str(&body).unwrap()
}
#[tokio::test]
async fn speex_resources_play_as_wav_through_preview_and_scoped_article_urls() {
    let data=tempfile::tempdir().unwrap();
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listen=listener.local_addr().unwrap();drop(listener);
    let server=tokio::spawn(serve(ServeConfig { listen, data_dir:data.path().into(), token:None, import_roots:vec![], static_dir:data.path().join("static") }));
    let client=Client::new();let base=format!("http://{listen}/api/v1");
    for _ in 0..100 { if client.get(format!("{base}/health")).send().await.is_ok(){break}tokio::time::sleep(Duration::from_millis(10)).await; }
    let source=include_bytes!("fixtures/tone.spx");
    let preview=json(client.post(format!("{base}/imports")).multipart(Form::new()
        .part("files",Part::bytes(include_bytes!("../../vendor/wodix/crates/wodix/tests/data/basic.mdx").to_vec()).file_name("test.mdx"))
        .part("files",Part::bytes(source.to_vec()).file_name("tone.spx"))).send().await.unwrap()).await;
    json(client.post(format!("{base}/imports/{}/commit",preview["id"].as_str().unwrap()))
        .json(&json!({"dictionaries":[{"name":"Audio","mdx":"test.mdx","resources":["tone.spx"]}]})).send().await.unwrap()).await;
    let bootstrap=json(client.get(format!("{base}/bootstrap")).send().await.unwrap()).await;
    let d=&bootstrap["dictionaries"][0];let id=d["id"].as_str().unwrap();let resource=&d["resources"][0];
    assert_eq!(resource["kind"],"audio");
    let original=data.path().join("revisions").join(d["revision"].as_str().unwrap()).join("tone.spx");
    let lookup=json(client.get(format!("{base}/lookup?dictionary={id}&q=apple")).send().await.unwrap()).await;
    let html=lookup["articles"][0]["html"].as_str().unwrap();
    let assets=html.split("data-fastdict-assets=\"").nth(1).unwrap().split('"').next().unwrap();
    let url=format!("{}/tone.spx",assets.trim_end_matches('/'));
    let playback=client.get(&url).send().await.unwrap();assert!(playback.status().is_success());
    assert!(playback.headers()["content-type"].to_str().unwrap().contains("wav"));
    let wav=playback.bytes().await.unwrap();assert!(wav.starts_with(b"RIFF"));assert_eq!(&wav[8..12],b"WAVE");
    let again=client.get(&url).send().await.unwrap().bytes().await.unwrap();assert_eq!(wav,again);
    let preview=client.get(format!("{base}/dictionaries/{id}/resources/{}/content",resource["id"].as_str().unwrap())).send().await.unwrap();
    assert!(preview.headers()["content-type"].to_str().unwrap().contains("wav"));assert_eq!(preview.bytes().await.unwrap(),wav);
    assert_eq!(std::fs::read(original).unwrap(),source);
    let capability=assets.trim_end_matches('/').rsplit('/').next().unwrap();
    let invalid=url.replace(capability,"invalid");
    assert_ne!(invalid,url);
    assert_eq!(client.get(invalid).send().await.unwrap().status(),403);
    server.abort();
}
