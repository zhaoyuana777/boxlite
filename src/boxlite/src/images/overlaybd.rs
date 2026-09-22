//! Verified local OverlayBD blobs, isolated from the ordinary OCI image cache.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use boxlite_shared::{BoxliteError, BoxliteResult};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::ContainerImageConfig;

const MAX_JSON: u64 = 4 * 1024 * 1024;

fn error(message: impl std::fmt::Display) -> BoxliteError {
    BoxliteError::Storage(format!("OverlayBD: {message}"))
}

fn digest_hex(digest: &str) -> BoxliteResult<&str> {
    let hex = digest.strip_prefix("sha256:").unwrap_or_default();
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(error("expected a lowercase sha256 digest"));
    }
    Ok(hex)
}

fn blob_path(root: &Path, digest: &str) -> BoxliteResult<PathBuf> {
    Ok(root.join("blobs/sha256").join(digest_hex(digest)?))
}

fn read_json(path: &Path, digest: &str) -> BoxliteResult<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(MAX_JSON + 1).read_to_end(&mut bytes))
        .map_err(|e| error(format!("read {}: {e}", path.display())))?;
    if bytes.len() as u64 > MAX_JSON {
        return Err(error("JSON exceeds 4 MiB"));
    }
    if hex::encode(Sha256::digest(&bytes)) != digest_hex(digest)? {
        return Err(error(format!("digest mismatch for {digest}")));
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct Blob {
    digest: String,
    size: u64,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    config: Blob,
    layers: Vec<Blob>,
}

pub(crate) struct OverlaybdImages {
    root: PathBuf,
}

impl OverlaybdImages {
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            root: home.join("overlaybd"),
        }
    }

    /// Import a pinned single-platform manifest from a local OCI layout.
    /// Blocking file I/O: runtime callers must use their blocking pool.
    pub(crate) fn import(
        &self,
        source: &Path,
        reference: &str,
    ) -> BoxliteResult<ContainerImageConfig> {
        if !source.is_absolute() || !source.is_dir() {
            return Err(error(
                "source must be an existing absolute OCI layout directory",
            ));
        }
        let (_, digest) = reference
            .rsplit_once('@')
            .ok_or_else(|| error("image reference must pin a manifest with @sha256:..."))?;
        let bytes = read_json(&blob_path(source, digest)?, digest)?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(error)?;
        if manifest.schema_version != 2 || !(1..=128).contains(&manifest.layers.len()) {
            return Err(error(
                "expected a single-platform manifest with 1..128 layers",
            ));
        }
        for layer in &manifest.layers {
            if layer
                .annotations
                .get("containerd.io/snapshot/overlaybd/version")
                .map(String::as_str)
                != Some("0.1.0")
            {
                return Err(error("only native OverlayBD 0.1.0 layers are supported"));
            }
            if layer
                .annotations
                .get("containerd.io/snapshot/overlaybd/blob-digest")
                != Some(&layer.digest)
            {
                return Err(error(
                    "layer blob digest annotation must match its descriptor",
                ));
            }
        }
        let config_bytes = read_json(
            &blob_path(source, &manifest.config.digest)?,
            &manifest.config.digest,
        )?;
        let config: oci_spec::image::ImageConfiguration =
            serde_json::from_slice(&config_bytes).map_err(error)?;
        let arch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            other => other,
        };
        if config.os().to_string() != "linux" || config.architecture().to_string() != arch {
            return Err(error(format!("image must target linux/{arch}")));
        }
        let config = ContainerImageConfig::from_oci_config(&config)?;
        for blob in std::iter::once(&manifest.config).chain(&manifest.layers) {
            self.copy_blob(source, blob)?;
        }
        // Publish the manifest last: its presence must not advertise a partial import.
        self.copy_blob(
            source,
            &Blob {
                digest: digest.into(),
                size: bytes.len() as u64,
                annotations: BTreeMap::new(),
            },
        )?;
        Ok(config)
    }

    fn copy_blob(&self, source: &Path, blob: &Blob) -> BoxliteResult<()> {
        let dst = blob_path(&self.root, &blob.digest)?;
        let cached = dst.exists();
        let src = if cached {
            dst.clone()
        } else {
            blob_path(source, &blob.digest)?
        };
        let mut input =
            File::open(&src).map_err(|e| error(format!("open {}: {e}", src.display())))?;
        let metadata = input.metadata().map_err(error)?;
        if !metadata.is_file() || metadata.len() != blob.size {
            return Err(error(format!("invalid blob size/type: {}", src.display())));
        }
        fs::create_dir_all(dst.parent().unwrap()).map_err(error)?;
        let mut staged = if cached {
            None
        } else {
            Some(tempfile::NamedTempFile::new_in(dst.parent().unwrap()).map_err(error)?)
        };
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        // Bound reads even if the source is concurrently growing.
        let mut input = (&mut input).take(blob.size.saturating_add(1));
        loop {
            let n = input.read(&mut buffer).map_err(error)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
            if let Some(file) = &mut staged {
                file.write_all(&buffer[..n]).map_err(error)?;
            }
        }
        if hex::encode(hash.finalize()) != digest_hex(&blob.digest)? {
            return Err(error(format!("digest mismatch for {}", blob.digest)));
        }
        if let Some(file) = staged {
            file.as_file().sync_all().map_err(error)?;
            match file.persist_noclobber(&dst) {
                Ok(_) => {}
                // A concurrent importer won; verify its bytes instead of trusting existence.
                Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return self.copy_blob(source, blob);
                }
                Err(e) => return Err(error(e)),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn write_blob(source: &Path, bytes: &[u8]) -> Value {
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(bytes)));
        let path = blob_path(source, &digest).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        json!({"digest": digest, "size": bytes.len()})
    }

    fn pin(source: &Path, manifest: &Value) -> String {
        let blob = write_blob(source, manifest.to_string().as_bytes());
        format!("example.test/image@{}", blob["digest"].as_str().unwrap())
    }

    fn fixture() -> (tempfile::TempDir, OverlaybdImages, PathBuf, Value) {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "amd64"
        };
        let config = write_blob(&source, json!({
            "architecture": arch, "os": "linux", "rootfs": {"type": "layers", "diff_ids": []},
            "config": {"Entrypoint": ["/bin/sh"], "Cmd": ["-c", "echo hello"], "Env": ["MODE=test"], "WorkingDir": "/app", "User": "1000"}
        }).to_string().as_bytes());
        let mut layer = write_blob(&source, &vec![42; 65537]);
        // Import verifies the OCI envelope; device-format validation belongs to the daemon.
        layer["annotations"] = json!({
            "containerd.io/snapshot/overlaybd/version": "0.1.0",
            "containerd.io/snapshot/overlaybd/blob-digest": layer["digest"],
        });
        let store = OverlaybdImages::new(&dir.path().join("runtime"));
        (
            dir,
            store,
            source,
            json!({"schemaVersion": 2, "config": config, "layers": [layer]}),
        )
    }

    #[test]
    fn import_pins_blobs_and_reopens_without_source() {
        let (dir, store, source, manifest) = fixture();
        let reference = pin(&source, &manifest);
        let config = store.import(&source, &reference).unwrap();
        assert_eq!(config.final_cmd(), ["/bin/sh", "-c", "echo hello"]);
        assert_eq!(config.env, ["MODE=test"]);
        assert_eq!(config.working_dir, "/app");
        assert_eq!(config.user, "1000");
        for entry in fs::read_dir(source.join("blobs/sha256")).unwrap() {
            let entry = entry.unwrap();
            assert_eq!(
                fs::read(entry.path()).unwrap(),
                fs::read(store.root.join("blobs/sha256").join(entry.file_name())).unwrap()
            );
        }
        fs::remove_dir_all(&source).unwrap();
        let reopened = OverlaybdImages::new(&dir.path().join("runtime"));
        assert_eq!(
            reopened
                .import(&reopened.root, &reference)
                .unwrap()
                .final_cmd(),
            config.final_cmd()
        );
        assert!(!dir.path().join("runtime/images").exists());
        assert!(!store.root.join("devices").exists());
    }

    #[test]
    fn import_rejects_invalid_references_and_manifests() {
        let (_dir, store, source, manifest) = fixture();
        for reference in [
            "image:latest",
            "image@sha512:abcd",
            "image@sha256:../escape",
            &format!("image@sha256:{}", "A".repeat(64)),
        ] {
            assert!(store.import(&source, reference).is_err(), "{reference}");
        }
        let reference = pin(&source, &manifest);
        for path in [Path::new("relative"), source.join("missing").as_path()] {
            assert!(
                store
                    .import(path, &reference)
                    .unwrap_err()
                    .to_string()
                    .contains("absolute OCI")
            );
        }
        for (pointer, value, message) in [
            ("/schemaVersion", json!(1), "single-platform"),
            ("/layers", json!([]), "single-platform"),
            (
                "/layers",
                json!(vec![manifest["layers"][0].clone(); 129]),
                "single-platform",
            ),
            ("/layers/0/annotations", json!({}), "native OverlayBD"),
            (
                "/layers/0/annotations/containerd.io~1snapshot~1overlaybd~1version",
                json!("2"),
                "native OverlayBD",
            ),
            (
                "/layers/0/annotations/containerd.io~1snapshot~1overlaybd~1blob-digest",
                json!("sha256:wrong"),
                "annotation",
            ),
            ("/config/size", json!(0), "size/type"),
            ("/layers/0/size", json!(0), "size/type"),
            (
                "/config/digest",
                json!("sha256:../../escape"),
                "lowercase sha256",
            ),
        ] {
            let mut invalid = manifest.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            let reference = pin(&source, &invalid);
            let error = store.import(&source, &reference).unwrap_err();
            assert!(error.to_string().contains(message), "{pointer}: {error}");
            assert!(
                !blob_path(&store.root, reference.split_once('@').unwrap().1)
                    .unwrap()
                    .exists()
            );
        }
        for bytes in [b"{".as_slice(), br#"{"schemaVersion":2,"manifests":[]}"#] {
            let blob = write_blob(&source, bytes);
            assert!(
                store
                    .import(
                        &source,
                        &format!("image@{}", blob["digest"].as_str().unwrap())
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn import_rejects_incompatible_or_invalid_config() {
        let (_dir, store, source, mut manifest) = fixture();
        let bytes =
            fs::read(blob_path(&source, manifest["config"]["digest"].as_str().unwrap()).unwrap())
                .unwrap();
        let original: Value = serde_json::from_slice(&bytes).unwrap();
        for (field, value) in [("os", "windows"), ("architecture", "s390x")] {
            let mut config = original.clone();
            config[field] = json!(value);
            manifest["config"] = write_blob(&source, config.to_string().as_bytes());
            assert!(
                store
                    .import(&source, &pin(&source, &manifest))
                    .unwrap_err()
                    .to_string()
                    .contains("linux/")
            );
        }
        manifest["config"] = write_blob(&source, b"{}");
        assert!(store.import(&source, &pin(&source, &manifest)).is_err());
        assert!(!store.root.exists());
    }

    #[test]
    fn import_detects_corruption_and_keeps_failed_import_unpublished() {
        for corrupt_cached in [false, true] {
            for contents in [vec![0; 65537], vec![0; 1]] {
                let (_dir, store, source, manifest) = fixture();
                let reference = pin(&source, &manifest);
                let root = if corrupt_cached {
                    store.import(&source, &reference).unwrap();
                    &store.root
                } else {
                    &source
                };
                let layer =
                    blob_path(root, manifest["layers"][0]["digest"].as_str().unwrap()).unwrap();
                fs::write(layer, contents).unwrap();
                let error = store.import(&source, &reference).unwrap_err().to_string();
                assert!(
                    error.contains("digest mismatch") || error.contains("size/type"),
                    "{error}"
                );
                if !corrupt_cached {
                    let cached = fs::read_dir(store.root.join("blobs/sha256"))
                        .unwrap()
                        .collect::<Vec<_>>();
                    assert_eq!(cached.len(), 1, "only the verified config may remain");
                    assert!(
                        !blob_path(&store.root, reference.split_once('@').unwrap().1)
                            .unwrap()
                            .exists()
                    );
                    fs::write(
                        blob_path(&source, manifest["layers"][0]["digest"].as_str().unwrap())
                            .unwrap(),
                        vec![42; 65537],
                    )
                    .unwrap();
                    store.import(&source, &reference).unwrap();
                }
            }
        }
    }

    #[test]
    fn import_bounds_json_and_reports_missing_or_non_file_blobs() {
        let (_dir, store, source, manifest) = fixture();
        let reference = pin(&source, &manifest);
        let manifest_path = blob_path(&source, reference.split_once('@').unwrap().1).unwrap();
        for bytes in [vec![0; MAX_JSON as usize + 1], b"corrupt".to_vec()] {
            fs::write(&manifest_path, &bytes).unwrap();
            let error = store.import(&source, &reference).unwrap_err().to_string();
            assert!(error.contains(if bytes.len() as u64 > MAX_JSON {
                "4 MiB"
            } else {
                "digest mismatch"
            }));
        }
        pin(&source, &manifest);
        let layer = blob_path(&source, manifest["layers"][0]["digest"].as_str().unwrap()).unwrap();
        fs::remove_file(&layer).unwrap();
        assert!(
            store
                .import(&source, &reference)
                .unwrap_err()
                .to_string()
                .contains("open")
        );
        fs::create_dir(&layer).unwrap();
        assert!(
            store
                .import(&source, &reference)
                .unwrap_err()
                .to_string()
                .contains("size/type")
        );
        fs::remove_file(manifest_path).unwrap();
        assert!(
            store
                .import(&source, &reference)
                .unwrap_err()
                .to_string()
                .contains("read")
        );
    }

    #[test]
    fn concurrent_imports_share_verified_cache() {
        let (_dir, store, source, manifest) = fixture();
        let reference = pin(&source, &manifest);
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    let barrier = &barrier;
                    let source = &source;
                    let reference = &reference;
                    let store = &store;
                    scope.spawn(move || {
                        barrier.wait();
                        store.import(source, reference).unwrap();
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
        });
        assert_eq!(
            fs::read_dir(store.root.join("blobs/sha256"))
                .unwrap()
                .count(),
            3
        );
        store.import(&store.root, &reference).unwrap();
    }
}
