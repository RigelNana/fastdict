use fastdict::api::{serve, ServeConfig};
use reqwest::Client;
use std::time::Duration;

#[tokio::test]
async fn hashed_assets_are_immutable_but_html_and_api_responses_are_not() {
    let data = tempfile::tempdir().unwrap();
    let static_dir = data.path().join("static");
    std::fs::create_dir_all(static_dir.join("assets")).unwrap();
    std::fs::write(static_dir.join("index.html"), "<!doctype html><title>fastdict</title>").unwrap();
    std::fs::write(static_dir.join("assets/index-aB12_cdE.js"), "export default true;").unwrap();
    std::fs::write(static_dir.join("assets/geist-DjL33-gN.woff2"), b"font fixture").unwrap();
    std::fs::write(static_dir.join("assets/runtime.js"), "export default true;").unwrap();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = socket.local_addr().unwrap();
    drop(socket);
    let server = tokio::spawn(serve(ServeConfig {
        listen, data_dir: data.path().into(), static_dir, token: None, import_roots: vec![],
    }));
    let client = Client::new();
    let base = format!("http://{listen}");
    for _ in 0..100 {
        if client.get(format!("{base}/api/v1/health")).send().await.is_ok() { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    for path in ["/assets/index-aB12_cdE.js", "/assets/geist-DjL33-gN.woff2"] {
        let response = client.get(format!("{base}{path}")).send().await.unwrap();
        assert!(response.status().is_success());
        assert_eq!(response.headers()["cache-control"], "public, max-age=31536000, immutable");
    }
    for path in ["/", "/word/apple", "/assets/runtime.js", "/assets/missing-aB12_cdE.js"] {
        let response = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.headers()["cache-control"], "no-cache", "{path}");
    }
    let response = client.get(format!("{base}/api/v1/bootstrap")).send().await.unwrap();
    assert!(!response.headers().get("cache-control").is_some_and(|v| v.to_str().unwrap().contains("immutable")));
    server.abort();
}
