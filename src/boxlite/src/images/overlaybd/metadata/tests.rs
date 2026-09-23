use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::super::tests::image_fixture;
use super::*;

#[derive(Clone)]
struct Response {
    status: u16,
    headers: String,
    body: Vec<u8>,
    chunked: bool,
}

impl Response {
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: String::new(),
            body,
            chunked: false,
        }
    }
}

pub(in crate::images::overlaybd) struct Registry {
    host: String,
    routes: Arc<Mutex<BTreeMap<String, Response>>>,
    paths: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Registry {
    pub(in crate::images::overlaybd) async fn new(authorization: Option<String>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = listener.local_addr().unwrap().to_string();
        let routes = Arc::new(Mutex::new(BTreeMap::<String, Response>::new()));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let (responses, requests) = (routes.clone(), paths.clone());
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    headers.push(match stream.read_u8().await {
                        Ok(byte) => byte,
                        Err(_) => break,
                    });
                    assert!(headers.len() < 16384);
                }
                let headers = String::from_utf8(headers).unwrap();
                let path = headers.split_whitespace().nth(1).unwrap_or("").to_owned();
                requests.lock().push(path.clone());
                if path == "/stall" {
                    std::future::pending::<()>().await;
                }
                let route = path.split('?').next().unwrap();
                let mut response = responses.lock().get(route).cloned().unwrap_or_else(|| {
                    let mut response = Response::ok(Vec::new());
                    response.status = if path == "/v2/" { 200 } else { 404 };
                    response
                });
                if (path.contains("/manifests/") || path.contains("/blobs/"))
                    && let Some(expected) = &authorization
                    && !headers.lines().any(|line| {
                        line.split_once(':').is_some_and(|(name, value)| {
                            name.eq_ignore_ascii_case("authorization") && value.trim() == expected
                        })
                    })
                {
                    response.status = 401;
                }
                let framing = if response.headers.contains("Content-Length:") {
                    String::new()
                } else if response.chunked {
                    "Transfer-Encoding: chunked\r\n".to_owned()
                } else {
                    format!("Content-Length: {}\r\n", response.body.len())
                };
                let head = format!(
                    "HTTP/1.1 {} Status\r\n{}{}Connection: close\r\n\r\n",
                    response.status, response.headers, framing
                );
                let _ = stream.write_all(head.as_bytes()).await;
                if response.chunked {
                    let _ = stream
                        .write_all(format!("{:x}\r\n", response.body.len()).as_bytes())
                        .await;
                    let _ = stream.write_all(&response.body).await;
                    let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
                } else {
                    let _ = stream.write_all(&response.body).await;
                }
            }
        });
        Self {
            host,
            routes,
            paths,
            task,
        }
    }

    pub(in crate::images::overlaybd) fn image(&self, source: &Path, manifest: &Value) -> String {
        let raw = serde_json::to_vec(manifest).unwrap();
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&raw)));
        self.routes
            .lock()
            .insert(format!("/v2/image/manifests/{digest}"), Response::ok(raw));
        let config_digest = manifest["config"]["digest"].as_str().unwrap();
        self.routes.lock().insert(
            format!("/v2/image/blobs/{config_digest}"),
            Response::ok(fs::read(blob_path(source, config_digest).unwrap()).unwrap()),
        );
        format!("{}/image@{digest}", self.host)
    }

    fn manifest_path(reference: &str) -> String {
        format!(
            "/v2/image/manifests/{}",
            reference.split_once('@').unwrap().1
        )
    }

    fn redirect(&self, path: String, target: &str) {
        self.routes.lock().insert(
            path,
            Response {
                status: 307,
                headers: format!("Location: http://{}{target}\r\n", self.host),
                ..Response::ok(vec![])
            },
        );
    }

    async fn failure(&self, store: &OverlaybdImages, reference: &str) -> String {
        let failure = store
            .pull_metadata(reference, &self.options())
            .await
            .unwrap_err();
        assert!(!store.root.join("metadata").exists());
        failure.to_string()
    }

    pub(in crate::images::overlaybd) fn options(&self) -> Vec<ImageRegistry> {
        vec![ImageRegistry::http(&self.host)]
    }
}

#[tokio::test]
async fn pulls_only_metadata_and_atomically_reuses_cache() {
    let (_dir, store, source, mut manifest) = image_fixture();
    manifest["mediaType"] = json!("application/vnd.oci.image.manifest.v1+json");
    let server = Registry::new(None).await;
    let reference = server.image(&source, &manifest);
    for _ in 0..2 {
        let result = store
            .pull_metadata(&reference, &server.options())
            .await
            .unwrap();
        assert_eq!(result.config.final_cmd(), ["/bin/sh", "-c", "echo hello"]);
        assert_eq!(
            result.repo_blob_url,
            format!("http://{}/v2/image/blobs", server.host)
        );
        let expected: Manifest = serde_json::from_value(manifest.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&result.manifest).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        let path = store
            .root
            .join("metadata")
            .join(image_digest(&reference).unwrap());
        assert_eq!(
            fs::read(path.join("manifest.json")).unwrap(),
            serde_json::to_vec(&manifest).unwrap()
        );
        assert_eq!(
            fs::read(path.join("config.json")).unwrap(),
            fs::read(blob_path(&source, &result.manifest.config.digest).unwrap()).unwrap()
        );
    }
    assert_eq!(
        fs::read_dir(store.root.join("metadata")).unwrap().count(),
        1
    );
    assert!(!store.root.join("blobs").exists());
    assert!(!store.root.join("devices").exists());
    assert!(!store.root.parent().unwrap().join("images").exists());
    let config = manifest["config"]["digest"].as_str().unwrap();
    assert_eq!(server.paths.lock().len(), 6);
    assert!(server.paths.lock().iter().all(|p| p == "/v2/"
        || p.contains("/manifests/")
        || p == &format!("/v2/image/blobs/{config}")));
}

#[tokio::test]
async fn reuses_basic_bearer_and_token_exchange_authentication() {
    use base64::Engine;
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("test-user:TEST_ONLY_PASSWORD")
    );
    for mode in ["basic", "bearer", "exchange"] {
        let expected = if mode == "basic" {
            basic.clone()
        } else {
            "Bearer TEST_ONLY_TOKEN".into()
        };
        let server = Registry::new(Some(expected)).await;
        let (_dir, store, source, manifest) = image_fixture();
        let reference = server.image(&source, &manifest);
        let mut probe = Response::ok(Vec::new());
        probe.status = 401;
        probe.headers = if mode == "exchange" {
            format!(
                "WWW-Authenticate: Bearer realm=\"http://{}/token\",service=\"test\"\r\n",
                server.host
            )
        } else {
            "WWW-Authenticate: Basic realm=\"test\"\r\n".into()
        };
        server.routes.lock().insert("/v2/".into(), probe);
        server.routes.lock().insert(
            "/token".into(),
            Response::ok(br#"{"token":"TEST_ONLY_TOKEN"}"#.to_vec()),
        );
        let registry = if mode == "bearer" {
            ImageRegistry::http(&server.host).with_bearer_auth("TEST_ONLY_TOKEN")
        } else {
            ImageRegistry::http(&server.host).with_basic_auth("test-user", "TEST_ONLY_PASSWORD")
        };
        store
            .pull_metadata(&reference, &[registry])
            .await
            .unwrap_or_else(|error| panic!("{mode}: {error}"));
        if mode == "exchange" {
            assert!(server.paths.lock().iter().any(|p| p.starts_with("/token?")));
        }
    }
}

#[tokio::test]
async fn rejects_bad_references_and_registry_settings_without_network() {
    let (_dir, store, _source, _manifest) = image_fixture();
    let server = Registry::new(None).await;
    for reference in [
        "https://bad/image",
        "image:latest",
        "image@sha512:abcd",
        "image@sha256:../escape",
    ] {
        assert!(
            store
                .pull_metadata(reference, &server.options())
                .await
                .is_err()
        );
    }
    assert!(
        store
            .pull_metadata(
                &format!("{}/image@sha256:{}", server.host, "0".repeat(64)),
                &[ImageRegistry::http("https://invalid")]
            )
            .await
            .is_err()
    );
    assert!(server.paths.lock().is_empty());
    assert!(!store.root.exists());
}

#[tokio::test]
async fn rejects_invalid_metadata_without_publishing_or_fetching_layers() {
    for (pointer, value, message) in [
        (
            "/mediaType",
            json!("application/vnd.oci.image.index.v1+json"),
            "single-platform",
        ),
        ("/config/size", json!(MAX_JSON + 1), "4 MiB"),
        ("/config/size", json!(1), "size/type"),
    ] {
        let (_dir, store, source, mut manifest) = image_fixture();
        manifest["mediaType"] = json!("application/vnd.oci.image.manifest.v1+json");
        *manifest.pointer_mut(pointer).unwrap() = value;
        let server = Registry::new(None).await;
        let reference = server.image(&source, &manifest);
        let failure = server.failure(&store, &reference).await;
        assert!(failure.contains(message), "{pointer}: {failure}");
        assert!(!server.paths.lock().iter().any(|p| {
            p.ends_with(manifest["layers"][0]["digest"].as_str().unwrap_or("never"))
                && p.contains("/blobs/")
        }));
    }
}

#[tokio::test]
async fn rejects_tampered_manifest_config_and_external_urls() {
    for mode in ["manifest", "config", "urls"] {
        let (_dir, store, source, mut manifest) = image_fixture();
        if mode == "urls" {
            manifest["layers"][0]["urls"] = json!(["https://other.invalid/blob"]);
        }
        let server = Registry::new(None).await;
        let reference = server.image(&source, &manifest);
        if mode == "manifest" || mode == "config" {
            let path = if mode == "manifest" {
                Registry::manifest_path(&reference)
            } else {
                format!(
                    "/v2/image/blobs/{}",
                    manifest["config"]["digest"].as_str().unwrap()
                )
            };
            server.routes.lock().get_mut(&path).unwrap().body[0] ^= 1;
        }
        let failure = server.failure(&store, &reference).await;
        assert!(
            failure.contains(if mode == "urls" {
                "external URLs"
            } else {
                "digest mismatch"
            }),
            "{mode}: {failure}"
        );
    }
}

#[tokio::test]
async fn bounds_streams_and_does_not_echo_error_bodies() {
    for mode in ["length", "chunked", "truncated", "401", "403", "404"] {
        let (_dir, store, source, manifest) = image_fixture();
        let server = Registry::new(None).await;
        let reference = server.image(&source, &manifest);
        let path = Registry::manifest_path(&reference);
        let mut response = Response::ok(if mode == "length" || mode == "chunked" {
            vec![b' '; MAX_JSON as usize + 1]
        } else {
            b"TEST_ONLY_SENSITIVE_BODY".to_vec()
        });
        if mode == "truncated" {
            response.headers = "Content-Length: 100\r\n".into();
        }
        response.chunked = mode == "chunked";
        response.status = mode.parse().unwrap_or(200);
        server.routes.lock().insert(path, response);
        let failure = server.failure(&store, &reference).await;
        assert!(!failure.contains("TEST_ONLY_SENSITIVE_BODY"));
        assert!(
            failure.contains(if mode == "length" || mode == "chunked" {
                "4 MiB"
            } else if mode == "truncated" {
                "body"
            } else {
                mode
            }),
            "{failure}"
        );
    }
}

#[tokio::test]
async fn cache_failure_preserves_existing_metadata() {
    let (dir, store, source, manifest) = image_fixture();
    let server = Registry::new(None).await;
    let reference = server.image(&source, &manifest);
    store
        .pull_metadata(&reference, &server.options())
        .await
        .unwrap();
    let path = store
        .root
        .join("metadata")
        .join(image_digest(&reference).unwrap())
        .join("config.json");
    fs::write(&path, "corrupt").unwrap();
    let failure = store
        .pull_metadata(&reference, &server.options())
        .await
        .unwrap_err();
    assert!(failure.to_string().contains("differs"));
    assert_eq!(fs::read(&path).unwrap(), b"corrupt");
    let cache_dir = path.parent().unwrap();
    fs::remove_dir_all(cache_dir).unwrap();
    fs::write(cache_dir, "not a directory").unwrap();
    assert!(
        store
            .pull_metadata(&reference, &server.options())
            .await
            .is_err()
    );
    assert_eq!(fs::read(cache_dir).unwrap(), b"not a directory");
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, "file").unwrap();
    assert!(
        OverlaybdImages::new(&blocked)
            .pull_metadata(&reference, &server.options())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn follows_blob_redirect_without_persisting_redirect_url() {
    let (_dir, store, source, manifest) = image_fixture();
    let server = Registry::new(None).await;
    let reference = server.image(&source, &manifest);
    let path = format!(
        "/v2/image/blobs/{}",
        manifest["config"]["digest"].as_str().unwrap()
    );
    let original = server.routes.lock().remove(&path).unwrap();
    server.routes.lock().insert("/redirected".into(), original);
    server.redirect(path, "/redirected?test-signature=TEST_ONLY");
    let result = store
        .pull_metadata(&reference, &server.options())
        .await
        .unwrap();
    assert!(!result.repo_blob_url.contains("signature"));
    assert!(
        server
            .paths
            .lock()
            .iter()
            .any(|p| p.starts_with("/redirected?"))
    );
}

#[tokio::test]
async fn times_out_stalled_metadata_without_publishing() {
    let (_dir, store, source, manifest) = image_fixture();
    let server = Registry::new(None).await;
    let reference = server.image(&source, &manifest);
    let path = Registry::manifest_path(&reference);
    server.redirect(path, "/stall");
    assert!(
        server
            .failure(&store, &reference)
            .await
            .contains("timed out after 30s")
    );
}

#[tokio::test]
async fn authentication_and_connection_errors_do_not_publish() {
    let (_dir, store, source, manifest) = image_fixture();
    let mut server = Registry::new(None).await;
    let reference = server.image(&source, &manifest);
    server.routes.lock().insert(
        "/v2/".into(),
        Response {
            status: 401,
            headers: format!(
                "WWW-Authenticate: Bearer realm=\"http://{}/token\"\r\n",
                server.host
            ),
            body: vec![],
            chunked: false,
        },
    );
    server.routes.lock().insert(
        "/token".into(),
        Response {
            status: 403,
            headers: String::new(),
            body: b"TEST_ONLY_SENSITIVE_BODY".to_vec(),
            chunked: false,
        },
    );
    let failure = store
        .pull_metadata(&reference, &server.options())
        .await
        .unwrap_err()
        .to_string();
    assert!(failure.contains("authentication failed"));
    assert!(!failure.contains("TEST_ONLY_SENSITIVE_BODY"));
    let options = vec![ImageRegistry::https(&server.host).with_bearer_auth("TEST_ONLY_TOKEN")];
    server.task.abort();
    assert!((&mut server.task).await.unwrap_err().is_cancelled());
    assert!(store.pull_metadata(&reference, &options).await.is_err());
    assert!(!store.root.exists());
}

#[tokio::test]
async fn concurrent_pulls_publish_one_complete_directory() {
    let (_dir, store, source, manifest) = image_fixture();
    let server = Registry::new(None).await;
    let reference = server.image(&source, &manifest);
    let options = server.options();
    let (first, second) = tokio::join!(
        store.pull_metadata(&reference, &options),
        store.pull_metadata(&reference, &options)
    );
    assert_eq!(
        first.unwrap().manifest.config.digest,
        second.unwrap().manifest.config.digest
    );
    let root = store.root.join("metadata");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    let cached = root.join(image_digest(&reference).unwrap());
    for (name, expected) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        (
            "config.json",
            fs::read(blob_path(&source, manifest["config"]["digest"].as_str().unwrap()).unwrap())
                .unwrap(),
        ),
    ] {
        assert_eq!(fs::read(cached.join(name)).unwrap(), expected);
    }
}
