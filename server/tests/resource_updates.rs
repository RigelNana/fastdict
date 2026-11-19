use fastdict::api::{ServeConfig, serve};
use reqwest::{Client, multipart::{Form, Part}};
use serde_json::{Value, json};
use std::time::Duration;

#[path = "../../vendor/wodix/crates/wodix/tests/support/mod.rs"]
mod corpus;

async fn lookup_html(client: &Client, base: &str, id: &str) -> String {
    json_response(client.get(format!("{base}/lookup?q=apple&dictionary={id}")).send().await.unwrap()).await["articles"][0]["html"].as_str().unwrap().to_owned()
}

fn asset_url(html: &str, key: &str) -> String {
    let base = html.split("data-fastdict-assets=\"").nth(1).unwrap().split('"').next().unwrap();
    format!("{}/{key}", base.trim_end_matches('/'))
}

#[tokio::test]
async fn css_switches_block_embedded_fallback_and_reuploads_replace_immutable_resources() {
    let data = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = listener.local_addr().unwrap(); drop(listener);
    let server = tokio::spawn(serve(ServeConfig { listen, data_dir:data.path().into(), token:None, import_roots:vec![], static_dir:data.path().join("static") }));
    let client = Client::new(); let base = format!("http://{listen}/api/v1");
    for _ in 0..100 {
        if client.get(format!("{base}/health")).send().await.is_ok() { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mdx = corpus::Corpus::articles(vec![corpus::Row::new("apple", br#"<link rel="stylesheet" href="style.css"><link rel="stylesheet" href="extra.css"><style>@import url(style.css);</style><p>A fruit.</p>"#)]).serialize().unwrap();
    let mdd = corpus::Corpus::resources(vec![corpus::Row::new("\\style.css", b"p{color:maroon}")]).serialize().unwrap();
    let mut form = Form::new();
    for (name, bytes) in [("test.mdx",mdx),("test.mdd",mdd),("style.css",b"p{color:blue}".to_vec()),("extra.css",b"p{font-weight:600}".to_vec())] {
        form=form.part("files",Part::bytes(bytes).file_name(name));
    }
    let preview=json_response(client.post(format!("{base}/imports")).multipart(form).send().await.unwrap()).await;
    json_response(client.post(format!("{base}/imports/{}/commit",preview["id"].as_str().unwrap()))
        .json(&json!({"dictionaries":[{"name":"Style fixture","mdx":"test.mdx","resources":["test.mdd","style.css","extra.css"]}]})).send().await.unwrap()).await;
    let boot=json_response(client.get(format!("{base}/bootstrap")).send().await.unwrap()).await;
    let dictionary=&boot["dictionaries"][0]; let id=dictionary["id"].as_str().unwrap();
    let original_root=data.path().join("revisions").join(dictionary["revision"].as_str().unwrap());
    let html=lookup_html(&client,&base,id).await;
    assert!(html.contains("color:blue") && html.contains("font-weight:600"));
    assert!(!html.contains("color:maroon"),"MDD must not shadow an uploaded file");

    let mut resources=dictionary["resources"].clone();
    resources[1]["enabled"]=false.into();
    json_response(client.put(format!("{base}/dictionaries/{id}")).json(&json!({"resources":resources})).send().await.unwrap()).await;
    let disabled=lookup_html(&client,&base,id).await;
    assert!(!disabled.contains("color:blue") && !disabled.contains("color:maroon"));
    assert!(disabled.contains("font-weight:600"));
    assert!(!disabled.contains("<link rel=\"stylesheet\""));
    for path in ["style.css","STYLE.CSS"] {
        assert_eq!(client.get(asset_url(&disabled,path)).send().await.unwrap().status(),404,"CSS @import must not bypass the switch");
    }
    assert_eq!(client.get(asset_url(&html,"style.css")).send().await.unwrap().status(),410,"Old article capabilities must expire on a toggle");
    resources[1]["enabled"]=true.into(); resources[2]["enabled"]=false.into();
    resources[2]["key"]="theme/extra.css".into();
    json_response(client.put(format!("{base}/dictionaries/{id}")).json(&json!({"resources":resources})).send().await.unwrap()).await;
    let enabled=lookup_html(&client,&base,id).await;
    assert!(enabled.contains("color:blue") && !enabled.contains("font-weight:600"));

    // Small multipart replacements retain bindings, order and disabled states.
    let form=upload("style.css",b"p{color:red}").part("files",Part::bytes(b"p{font-weight:700}".to_vec()).file_name("extra.css"));
    let updated=json_response(client.post(format!("{base}/dictionaries/{id}/resources")).multipart(form).send().await.unwrap()).await;
    assert_eq!(updated["resources"].as_array().unwrap().len(),3);
    for index in 0..3 {
        for field in ["id","path","key","order","enabled"] { assert_eq!(updated["resources"][index][field],resources[index][field]); }
    }
    assert_eq!(std::fs::read(original_root.join("style.css")).unwrap(),b"p{color:blue}");
    let revised_root=data.path().join("revisions").join(updated["revision"].as_str().unwrap());
    assert_eq!(std::fs::read(revised_root.join("style.css")).unwrap(),b"p{color:red}");
    assert!(lookup_html(&client,&base,id).await.contains("color:red"));

    // Chunked replacements publish once and never alter an older mapped revision.
    let bytes=b"p{color:green}";
    let session=json_response(client.post(format!("{base}/uploads")).json(&json!({"kind":"resources","dictionary_id":id,"files":[{"path":"style.css","size":bytes.len()}]})).send().await.unwrap()).await;
    let upload_id=session["id"].as_str().unwrap();
    json_response(client.put(format!("{base}/uploads/{upload_id}/files/0?offset=0")).body(bytes.to_vec()).send().await.unwrap()).await;
    let published=json_response(client.post(format!("{base}/uploads/{upload_id}/complete")).send().await.unwrap()).await;
    let again=json_response(client.post(format!("{base}/uploads/{upload_id}/complete")).send().await.unwrap()).await;
    assert_eq!(published,again);
    assert_eq!(published["resources"][1]["id"],resources[1]["id"]);
    assert_eq!(published["resources"].as_array().unwrap().len(),3);
    assert_eq!(std::fs::read(revised_root.join("style.css")).unwrap(),b"p{color:red}");
    assert!(lookup_html(&client,&base,id).await.contains("color:green"));
    // Unbinding leaves the old immutable file on disk; uploading it again still works.
    let mut unbound=published["resources"].as_array().unwrap().clone();
    unbound.retain(|resource| resource["path"] != "extra.css");
    json_response(client.put(format!("{base}/dictionaries/{id}")).json(&json!({"resources":unbound})).send().await.unwrap()).await;
    let rebound=json_response(client.post(format!("{base}/dictionaries/{id}/resources")).multipart(upload("extra.css",b"p{font-weight:800}")).send().await.unwrap()).await;
    assert_eq!(rebound["resources"].as_array().unwrap().len(),3);
    assert!(lookup_html(&client,&base,id).await.contains("font-weight:800"));
    let rejected=client.post(format!("{base}/uploads")).json(&json!({"kind":"resources","dictionary_id":id,"files":[{"path":"test.mdx","size":1}]})).send().await.unwrap();
    assert!(!rejected.status().is_success());
    server.abort();
}

fn upload(name: &str, bytes: &[u8]) -> Form {
    Form::new().part("files", Part::bytes(bytes.to_vec()).file_name(name.to_owned()))
}

async fn json_response(response: reqwest::Response) -> Value {
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
    serde_json::from_str(&body).unwrap()
}

async fn wait_job(client: &Client, base: &str, id: &str) -> Value {
    for _ in 0..200 {
        let job = json_response(client.get(format!("{base}/jobs/{id}")).send().await.unwrap()).await;
        if !matches!(job["state"].as_str(), Some("queued" | "running")) { return job; }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("Job did not complete");
}

#[tokio::test]
async fn resource_edits_preserve_indexes_and_import_reports_completion() {
    let data = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = listener.local_addr().unwrap();
    drop(listener);
    let server = tokio::spawn(serve(ServeConfig {
        listen, data_dir: data.path().to_owned(), token: None,
        import_roots: vec![], static_dir: data.path().join("static"),
    }));
    let client = Client::new();
    let base = format!("http://{listen}/api/v1");
    for _ in 0..100 {
        if client.get(format!("{base}/health")).send().await.is_ok() { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mdx = include_bytes!("../../vendor/wodix/crates/wodix/tests/data/basic.mdx");
    let preview = json_response(client.post(format!("{base}/imports"))
        .multipart(upload("test.mdx", mdx)).send().await.unwrap()).await;
    let commit = format!("{base}/imports/{}/commit?background=true", preview["id"].as_str().unwrap());
    let selection = json!({"dictionaries": [{"name": "Test", "mdx": "test.mdx", "resources": []}]});
    let started = json_response(client.post(&commit).json(&selection).send().await.unwrap()).await;
    let job_id = started["job_id"].as_str().unwrap();
    let done = wait_job(&client, &base, job_id).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["stage"], "done");
    assert_eq!(done["progress"], 1);
    // A repeated commit must not register the dictionary twice.
    let repeated = json_response(client.post(&commit).json(&selection).send().await.unwrap()).await;
    assert_eq!(repeated, started);
    let bootstrap = json_response(client.get(format!("{base}/bootstrap")).send().await.unwrap()).await;
    assert_eq!(bootstrap["dictionaries"].as_array().unwrap().len(), 1);
    let dictionary = &bootstrap["dictionaries"][0];
    assert_eq!(dictionary["index_status"]["state"], "missing");
    let id = dictionary["id"].as_str().unwrap();
    let old_revision = dictionary["revision"].as_str().unwrap();
    assert_eq!(done["dictionary_id"], id);
    let maintenance = json_response(client.post(format!("{base}/maintenance"))
        .json(&json!({"target":"fulltext", "action":"rebuild", "dictionary_id":id}))
        .send().await.unwrap()).await;
    assert_eq!(wait_job(&client, &base, maintenance["job_id"].as_str().unwrap()).await["state"], "done");
    let hits = json_response(client.get(format!("{base}/suggest?q=fruit&mode=fulltext")).send().await.unwrap()).await;
    assert!(!hits["items"].as_array().unwrap().is_empty());
    json_response(client.get(format!("{base}/lookup?q=apple&dictionary={id}")).send().await.unwrap()).await;

    let updated = json_response(client.post(format!("{base}/dictionaries/{id}/resources"))
        .multipart(upload("style.css", b"p { color: inherit; }" )).send().await.unwrap()).await;
    assert_eq!(updated["index_status"]["state"], "ready");
    assert_eq!(updated["entries"], dictionary["entries"]);
    let revision = updated["revision"].as_str().unwrap();
    assert_ne!(revision, old_revision);
    let old_root = data.path().join("revisions").join(old_revision);
    let new_root = data.path().join("revisions").join(revision);
    assert!(!old_root.join("style.css").exists());
    assert!(new_root.join("style.css").exists());
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(old_root.join("test.mdx")).unwrap().ino(),
            std::fs::metadata(new_root.join("test.mdx")).unwrap().ino());
    }
    let mut resources = updated["resources"].clone();
    resources[0]["enabled"] = false.into();
    let renamed = json_response(client.put(format!("{base}/dictionaries/{id}"))
        .json(&json!({"name":"Renamed", "resources":resources, "font_family":"serif"}))
        .send().await.unwrap()).await;
    assert_eq!(renamed["revision"], revision);
    assert_eq!(renamed["index_status"]["state"], "ready");
    let new_hits = json_response(client.get(format!("{base}/suggest?q=fruit&mode=fulltext")).send().await.unwrap()).await;
    assert_eq!(new_hits["items"], hits["items"]);
    let rejected = client.post(format!("{base}/dictionaries/{id}/resources"))
        .multipart(upload("other.mdx", mdx)).send().await.unwrap();
    assert!(!rejected.status().is_success());
    let preserved = json_response(client.get(format!("{base}/bootstrap")).send().await.unwrap()).await;
    assert_eq!(preserved["dictionaries"][0]["index_status"]["state"], "ready");

    // Invalid content becomes a failed background job and leaves staging retryable.
    let invalid = json_response(client.post(format!("{base}/imports"))
        .multipart(upload("bad.mdx", b"invalid MDX")).send().await.unwrap()).await;
    let started = json_response(client.post(format!("{base}/imports/{}/commit?background=true", invalid["id"].as_str().unwrap()))
        .json(&json!({"dictionaries":[{"name":"Bad", "mdx":"bad.mdx", "resources":[]}]}))
        .send().await.unwrap()).await;
    let failed = wait_job(&client, &base, started["job_id"].as_str().unwrap()).await;
    assert_eq!(failed["state"], "error");
    assert!(failed["error"].is_string());
    server.abort();
}
