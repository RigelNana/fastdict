use fastdict::{api::{ServeConfig, serve}, uploads::CHUNK_BYTES};
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};
use std::{io::{Read, Write}, time::Duration};

struct Server { data: tempfile::TempDir, task: tokio::task::JoinHandle<anyhow::Result<()>>, client: Client, base: String }
impl Drop for Server { fn drop(&mut self) { self.task.abort(); } }
impl Server {
    async fn start() -> Self {
        let data = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let listen = listener.local_addr().unwrap();drop(listener);
        let task = tokio::spawn(serve(ServeConfig { listen, data_dir: data.path().to_owned(), token: Some("upload-test".into()), import_roots: vec![], static_dir: data.path().join("static") }));
        let client = Client::new();let base = format!("http://{listen}/api/v1");
        for _ in 0..100 { if client.get(format!("{base}/health")).send().await.is_ok() { break; }tokio::time::sleep(Duration::from_millis(10)).await; }
        Self { data, task, client, base }
    }
    fn request(&self, method: &str, path: &str) -> RequestBuilder {
        self.client.request(method.parse().unwrap(), format!("{}{path}",self.base)).bearer_auth("upload-test")
    }
    async fn create(&self, files: Value, dictionary: Option<&str>) -> String {
        let v = ok(self.request("POST","/uploads").json(&json!({"kind":if dictionary.is_some(){"resources"}else{"import"},"dictionary_id":dictionary,"files":files}))).await;
        assert_eq!(v["chunk_size"], CHUNK_BYTES);
        v["id"].as_str().unwrap().to_owned()
    }
    async fn write(&self,id:&str,index:usize,offset:u64,bytes:Vec<u8>) -> Value {
        ok(self.request("PUT",&format!("/uploads/{id}/files/{index}?offset={offset}")).header("Content-Type","application/octet-stream").body(bytes)).await
    }
}
async fn ok(request: RequestBuilder) -> Value {
    let response=request.send().await.unwrap();let status=response.status();let body=response.text().await.unwrap();
    assert!(status.is_success(),"{status}: {body}");serde_json::from_str(&body).unwrap()
}

#[tokio::test]
async fn chunk_transport_requires_auth_bounds_blocks_and_survives_duplicate_delivery() {
    let s=Server::start().await;
    let unauthorized=s.client.post(format!("{}/uploads",s.base)).json(&json!({"kind":"import","files":[]})).send().await.unwrap();
    assert_eq!(unauthorized.status(),401);
    for path in ["../escape.mdd", "/absolute.mdd", "nested/../../escape.mdd"] {
        let response=s.request("POST","/uploads").json(&json!({"kind":"import","files":[{"path":path,"size":1}]})).send().await.unwrap();
        assert!(!response.status().is_success());
    }
    let size=CHUNK_BYTES as u64+3;
    let id=s.create(json!([{"path":"folder/large.mdd","size":size}]),None).await;
    let initial=ok(s.request("GET",&format!("/uploads/{id}"))).await;assert_eq!(initial["received"],json!([0]));
    let missing=s.request("POST",&format!("/uploads/{id}/complete")).send().await.unwrap();assert!(!missing.status().is_success());
    let bad=s.request("PUT",&format!("/uploads/{id}/files/0?offset=0")).body(vec![1;CHUNK_BYTES+1]).send().await.unwrap();assert_eq!(bad.status(),413);
    assert!(!s.request("PUT",&format!("/uploads/{id}/files/0?offset={CHUNK_BYTES}")).body(vec![1;3]).send().await.unwrap().status().is_success());
    // Simulate a hard restart leaving an incomplete, unacknowledged block.
    let file=s.data.path().join("uploads").join(&id).join("files/folder/large.mdd");
    std::fs::OpenOptions::new().write(true).open(&file).unwrap().write_all(&[8;13]).unwrap();
    let a=s.write(&id,0,0,vec![5;CHUNK_BYTES]).await;
    let repeated=s.write(&id,0,0,vec![5;CHUNK_BYTES]).await;assert_eq!(a,repeated);
    assert!(!s.request("PUT",&format!("/uploads/{id}/files/0?offset=0")).body(vec![6;CHUNK_BYTES]).send().await.unwrap().status().is_success());
    s.write(&id,0,CHUNK_BYTES as u64,vec![1,2,3]).await;
    let preview=ok(s.request("POST",&format!("/uploads/{id}/complete"))).await;
    assert_eq!(preview["files"][0]["size"],size);
    assert_eq!(preview,ok(s.request("POST",&format!("/uploads/{id}/complete"))).await);
    ok(s.request("DELETE",&format!("/uploads/{id}"))).await;
    assert!(s.data.path().join("staging").join(&id).join("folder/large.mdd").exists());
    assert!(!s.data.path().join("uploads").join(&id).exists());
}

#[tokio::test]
async fn chunked_import_and_resource_publication_preserve_indexes_and_are_idempotent() {
    let s=Server::start().await;
    let mdx=include_bytes!("../../vendor/wodix/crates/wodix/tests/data/basic.mdx");
    let id=s.create(json!([{"path":"test.mdx","size":mdx.len()}]),None).await;
    s.write(&id,0,0,mdx.to_vec()).await;
    ok(s.request("POST",&format!("/uploads/{id}/complete"))).await;
    ok(s.request("DELETE",&format!("/uploads/{id}"))).await;
    ok(s.request("POST",&format!("/imports/{id}/commit")).json(&json!({"dictionaries":[{"name":"Chunk test","mdx":"test.mdx","resources":[]}]}))).await;
    let bootstrap=ok(s.request("GET","/bootstrap")).await;
    let dictionary=bootstrap["dictionaries"][0]["id"].as_str().unwrap();
    let job=ok(s.request("POST","/maintenance").json(&json!({"target":"fulltext","action":"rebuild","dictionary_id":dictionary}))).await;
    for _ in 0..100 {
        let status=ok(s.request("GET",&format!("/jobs/{}",job["job_id"].as_str().unwrap()))).await;
        if status["state"]=="done"{break}tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let before=ok(s.request("GET","/suggest?q=fruit&mode=fulltext")).await;
    let resource=s.create(json!([{"path":"style.css","size":17}]),Some(dictionary)).await;
    s.write(&resource,0,0,b"p { color: red; }".to_vec()).await;
    let changed=ok(s.request("POST",&format!("/uploads/{resource}/complete"))).await;
    assert_eq!(changed["revision"],resource);
    assert_eq!(changed["index_status"]["state"],"ready");
    assert_eq!(changed["resources"].as_array().unwrap().len(),1);
    assert_eq!(changed,ok(s.request("POST",&format!("/uploads/{resource}/complete"))).await);
    // Recovery if the process stopped after catalog publication but before saving its reply.
    let manifest_path=s.data.path().join("uploads").join(&resource).join("manifest.json");
    let mut manifest:Value=serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();manifest["result"]=Value::Null;
    std::fs::write(&manifest_path,serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_eq!(changed,ok(s.request("POST",&format!("/uploads/{resource}/complete"))).await);
    ok(s.request("DELETE",&format!("/uploads/{resource}"))).await;
    let after=ok(s.request("GET","/suggest?q=fruit&mode=fulltext")).await;assert_eq!(before["items"],after["items"]);
}

#[tokio::test]
#[ignore = "Streams 1 GiB through the HTTP upload API; run explicitly for large-file verification"]
async fn upload_one_gib_without_a_large_request_or_whole_file_buffer() {
    let s=Server::start().await;
    let size=1024u64*1024*1024+3;
    let id=s.create(json!([{"path":"one-gib.mdd","size":size}]),None).await;
    let chunk=vec![0x5a;CHUNK_BYTES];let mut offset=0;
    while offset<size {
        let len=(size-offset).min(CHUNK_BYTES as u64) as usize;
        let result=s.write(&id,0,offset,chunk[..len].to_vec()).await;
        offset+=len as u64;assert_eq!(result["received"],offset);
    }
    let preview=ok(s.request("POST",&format!("/uploads/{id}/complete"))).await;assert_eq!(preview["files"][0]["size"],size);
    let path=s.data.path().join("staging").join(&id).join("one-gib.mdd");
    let mut file=std::fs::File::open(path).unwrap();let mut buf=vec![0;CHUNK_BYTES];let mut checked=0;
    loop {let n=file.read(&mut buf).unwrap();if n==0{break}assert!(buf[..n].iter().all(|byte|*byte==0x5a));checked+=n as u64;}
    assert_eq!(checked,size);
}
