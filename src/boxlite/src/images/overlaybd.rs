//! Verified local OverlayBD blobs, isolated from the ordinary OCI image cache.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use boxlite_shared::{BoxliteError, BoxliteResult};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::ContainerImageConfig;
use crate::disk::{BackingFormat, Disk, DiskFormat, Qcow2Helper};

const SOCKET: &str = "/var/run/overlaybd-ublk/ublkd.sock";

const MAX_JSON: u64 = 4 * 1024 * 1024;

fn error(message: impl std::fmt::Display) -> BoxliteError {
    BoxliteError::Storage(format!("OverlayBD: {message}"))
}

pub(crate) fn image_digest(reference: &str) -> BoxliteResult<&str> {
    let (_, digest) = reference.rsplit_once('@').ok_or_else(|| {
        error("image reference must pin a converted single-platform manifest with @sha256:...")
    })?;
    digest_hex(digest)
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

fn read_bounded(path: &Path) -> BoxliteResult<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(MAX_JSON + 1).read_to_end(&mut bytes))
        .map_err(|e| error(format!("read {}: {e}", path.display())))?;
    if bytes.len() as u64 > MAX_JSON {
        return Err(error("JSON exceeds 4 MiB"));
    }
    Ok(bytes)
}

fn read_json(path: &Path, digest: &str) -> BoxliteResult<Vec<u8>> {
    let bytes = read_bounded(path)?;
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
        self.import_image(source, reference)
            .map(|(config, _)| config)
    }

    fn import_image(
        &self,
        source: &Path,
        reference: &str,
    ) -> BoxliteResult<(ContainerImageConfig, Vec<PathBuf>)> {
        if !source.is_absolute() || !source.is_dir() {
            return Err(error(
                "source must be an existing absolute OCI layout directory",
            ));
        }
        let digest = format!("sha256:{}", image_digest(reference)?);
        let bytes = read_json(&blob_path(source, &digest)?, &digest)?;
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
                digest,
                size: bytes.len() as u64,
                annotations: BTreeMap::new(),
            },
        )?;
        let layers = manifest
            .layers
            .iter()
            .map(|layer| blob_path(&self.root, &layer.digest))
            .collect::<BoxliteResult<_>>()?;
        Ok((config, layers))
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

#[derive(Debug, Deserialize)]
struct Device {
    dev_id: u32,
    dev: PathBuf,
    config: PathBuf,
    writable: bool,
    state: String,
}

pub(crate) struct Overlaybd {
    images: OverlaybdImages,
    source: Option<PathBuf>,
    socket: PathBuf,
    // ponytail: serialize device transitions; split per image if startup throughput requires it.
    users: Mutex<BTreeMap<String, BTreeSet<String>>>,
}

pub(crate) struct Lease {
    owner: Arc<Overlaybd>,
    box_id: String,
    armed: bool,
}

impl Lease {
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.armed
            && let Err(e) = self.owner.release(&self.box_id)
        {
            tracing::warn!(box_id = %self.box_id, error = %e, "Failed to release OverlayBD device; recovery will retry");
        }
    }
}

impl Overlaybd {
    pub(crate) fn new(home: &Path, source: Option<PathBuf>) -> BoxliteResult<Arc<Self>> {
        if source.is_some() && !cfg!(target_os = "linux") {
            return Err(BoxliteError::Unsupported(
                "OverlayBD requires a Linux cloud runner".into(),
            ));
        }
        if let Some(path) = &source
            && !path.is_absolute()
        {
            return Err(error(
                "image directory must be an absolute OCI layout directory",
            ));
        }
        Ok(Arc::new(Self {
            images: OverlaybdImages::new(home),
            source,
            socket: SOCKET.into(),
            users: Mutex::new(BTreeMap::new()),
        }))
    }

    pub(crate) fn enabled(&self) -> bool {
        self.source.is_some()
    }

    pub(crate) fn import(&self, reference: &str) -> BoxliteResult<()> {
        let source = self
            .source
            .as_ref()
            .ok_or_else(|| error("backend is disabled"))?;
        self.images.import(source, reference).map(|_| ())
    }

    fn config_path(&self, digest: &str) -> PathBuf {
        self.images
            .root
            .join("devices")
            .join(format!("{digest}.json"))
    }

    // curl is the upstream documented UDS client, also installed in the runner image.
    // Disable user curl configuration/proxies and bound both time and response size.
    fn request(&self, operation: &str, body: Option<Value>) -> BoxliteResult<Value> {
        let output = tempfile::NamedTempFile::new().map_err(error)?;
        let mut cmd = Command::new("/usr/bin/curl");
        cmd.args([
            "--disable",
            "--silent",
            "--show-error",
            "--fail",
            "--noproxy",
            "*",
            "--connect-timeout",
            "5",
            "--max-time",
            "30",
            "--max-filesize",
            "1048576",
            "--unix-socket",
        ])
        .arg(&self.socket)
        .arg("--output")
        .arg(output.path());
        if let Some(body) = body {
            cmd.args(["--request", "POST", "--data-binary", &body.to_string()]);
        }
        let status = cmd
            .arg(format!("http://localhost/v1/{operation}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(error)?;
        if !status.success() {
            return Err(error(format!(
                "daemon {operation} at {} failed ({status}); check daemon log",
                self.socket.display()
            )));
        }
        let response: Value =
            serde_json::from_slice(&read_bounded(output.path())?).map_err(error)?;
        if response.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(error(format!("daemon {operation} returned failure")));
        }
        Ok(response)
    }

    fn devices(&self) -> BoxliteResult<Vec<Device>> {
        serde_json::from_value(
            self.request("list", None)?
                .get("devices")
                .cloned()
                .ok_or_else(|| error("missing device list"))?,
        )
        .map_err(error)
    }

    fn matching_device(&self, digest: &str) -> BoxliteResult<Option<Device>> {
        let matches: Vec<_> = self
            .devices()?
            .into_iter()
            .filter(|d| d.config == self.config_path(digest))
            .collect();
        if matches.len() > 1 {
            return Err(error(
                "duplicate devices for one image; operator reconciliation required",
            ));
        }
        let device = matches.into_iter().next();
        if let Some(d) = &device
            && (d.writable
                || d.state != "running"
                || d.dev != Path::new(&format!("/dev/ublkb{}", d.dev_id)))
        {
            return Err(error("expected a running read-only UBLK device"));
        }
        Ok(device)
    }

    fn acquire(self: &Arc<Self>, box_id: &str, digest: &str) -> BoxliteResult<(PathBuf, Lease)> {
        let mut users = self.users.lock();
        let device = self.matching_device(digest)?;
        if device.is_none() && users.get(digest).is_some_and(|ids| !ids.is_empty()) {
            return Err(error(
                "device disappeared while boxes still hold it; stop those boxes before restarting",
            ));
        }
        // Register cleanup before add: a lost response can still leave a live device.
        users
            .entry(digest.into())
            .or_default()
            .insert(box_id.into());
        let lease = Lease {
            owner: self.clone(),
            box_id: box_id.into(),
            armed: true,
        };
        let device = match device {
            Some(device) => Ok(device),
            None => self
                .request("add", Some(json!({"config": self.config_path(digest)})))
                .and_then(|_| {
                    self.matching_device(digest)?
                        .ok_or_else(|| error("created device missing from daemon list"))
                }),
        };
        // Lease rollback calls release(), which takes this same lock.
        drop(users);
        device.map(|device| (device.dev, lease))
    }

    pub(crate) fn prepare(
        self: &Arc<Self>,
        box_id: &str,
        reference: &str,
        disk_path: &Path,
        size_gb: Option<u64>,
    ) -> BoxliteResult<(ContainerImageConfig, Disk, Lease)> {
        if !self.enabled() {
            return Err(BoxliteError::Unsupported(
                "OverlayBD is disabled for this runner".into(),
            ));
        }
        let digest = image_digest(reference)?;
        let (config, layers) = self.images.import_image(&self.images.root, reference)?;
        let lowers: Vec<_> = layers.iter().map(|path| json!({"file": path})).collect();
        let path = self.config_path(digest);
        fs::create_dir_all(path.parent().unwrap()).map_err(error)?;
        if !path.exists() {
            let mut staged =
                tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(error)?;
            serde_json::to_writer(&mut staged, &json!({"lowers": lowers})).map_err(error)?;
            staged.as_file().sync_all().map_err(error)?;
            match staged.persist_noclobber(&path) {
                Ok(_) => {}
                Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(error(e)),
            }
        } else {
            let stored: Value = serde_json::from_slice(&read_bounded(&path)?).map_err(error)?;
            if stored != json!({"lowers": lowers}) {
                return Err(error("persisted device config differs from pinned image"));
            }
        }
        let (device, lease) = self.acquire(box_id, digest)?;
        let capacity = device_capacity(&device)?;
        let disk = prepare_cow(disk_path, &device, capacity, size_gb)?;
        Ok((config, disk, lease))
    }

    /// Called only after a shim is stopped; failed releases retain ownership for retry.
    pub(crate) fn release(&self, box_id: &str) -> BoxliteResult<()> {
        let mut users = self.users.lock();
        let Some(digest) = users
            .iter()
            .find(|(_, ids)| ids.contains(box_id))
            .map(|(d, _)| d.clone())
        else {
            return Ok(());
        };
        if users[&digest].len() == 1 {
            if let Some(device) = self.matching_device(&digest)? {
                self.request("del", Some(json!({"dev_id": device.dev_id})))?;
            }
            users.remove(&digest);
        } else {
            users.get_mut(&digest).unwrap().remove(box_id);
        }
        Ok(())
    }

    /// Rebuild ownership from surviving shims, then reclaim only this runtime's orphans.
    pub(crate) fn recover(&self, active: BTreeMap<String, BTreeSet<String>>) -> BoxliteResult<()> {
        *self.users.lock() = active;
        if !self.images.root.join("devices").exists() {
            return Ok(());
        }
        for device in self.devices()? {
            let Some(name) = device.config.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if digest_hex(&format!("sha256:{name}")).is_err()
                || device.config != self.config_path(name)
            {
                continue;
            }
            if !self.users.lock().contains_key(name) {
                self.request("del", Some(json!({"dev_id": device.dev_id})))?;
            }
        }
        Ok(())
    }
}

fn device_capacity(path: &Path) -> BoxliteResult<u64> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::FileTypeExt;
        let file = File::open(path).map_err(error)?;
        if !file
            .metadata()
            .map_err(error)?
            .file_type()
            .is_block_device()
        {
            return Err(error("backing is not a block device"));
        }
        let mut readonly: libc::c_int = 0;
        // BLKROGET reports the kernel-enforced read-only flag, not daemon metadata.
        if unsafe { libc::ioctl(file.as_raw_fd(), 0x125e as libc::c_ulong, &mut readonly) } < 0 {
            return Err(error(std::io::Error::last_os_error()));
        }
        if readonly != 1 {
            return Err(error("backing block device is writable"));
        }
        let mut size: u64 = 0;
        // BLKGETSIZE64 writes one u64 to a valid output pointer; metadata.len() is zero for devices.
        if unsafe { libc::ioctl(file.as_raw_fd(), 0x80081272 as libc::c_ulong, &mut size) } < 0 {
            return Err(error(std::io::Error::last_os_error()));
        }
        if size == 0 {
            return Err(error("block device has zero capacity"));
        }
        Ok(size)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(BoxliteError::Unsupported(
            "OverlayBD devices require Linux".into(),
        ))
    }
}

fn prepare_cow(
    path: &Path,
    device: &Path,
    capacity: u64,
    size_gb: Option<u64>,
) -> BoxliteResult<Disk> {
    if path.exists() {
        // The immutable manifest identifies the contents; device numbers are deliberately transient.
        let size = Qcow2Helper::qcow2_virtual_size(path)?;
        if size < capacity {
            return Err(error("existing qcow2 is smaller than its backing device"));
        }
        if crate::disk::read_backing_file_path(path)?.as_deref() != device.to_str() {
            // Never leave a torn qcow2 header after a crash during device rebinding.
            let staged = tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(error)?;
            if reflink_copy::reflink(path, staged.path()).is_err() {
                fs::copy(path, staged.path()).map_err(error)?;
            }
            crate::disk::qcow2::set_backing_file_path(staged.path(), device)?;
            staged.as_file().sync_all().map_err(error)?;
            staged.persist(path).map_err(error)?;
            File::open(path.parent().unwrap())
                .and_then(|f| f.sync_all())
                .map_err(error)?;
        }
    } else {
        let requested = size_gb
            .unwrap_or(0)
            .checked_mul(1024 * 1024 * 1024)
            .ok_or_else(|| error("disk size overflow"))?;
        Qcow2Helper::create_cow_child_disk(
            device,
            BackingFormat::Raw,
            path,
            capacity.max(requested),
        )?
        .leak();
    }
    Ok(Disk::new(path.to_path_buf(), DiskFormat::Qcow2, true))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixListener;

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

    fn image_fixture() -> (tempfile::TempDir, OverlaybdImages, PathBuf, Value) {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "amd64"
        };
        let mut config = write_blob(&source, json!({
            "architecture": arch, "os": "linux", "rootfs": {"type": "layers", "diff_ids": []},
            "config": {"Entrypoint": ["/bin/sh"], "Cmd": ["-c", "echo hello"], "Env": ["MODE=test"], "WorkingDir": "/app", "User": "1000"}
        }).to_string().as_bytes());
        config["mediaType"] = json!("application/vnd.oci.image.config.v1+json");
        let mut layer = write_blob(&source, &vec![42; 65537]);
        layer["mediaType"] = json!("application/vnd.oci.image.layer.v1.tar");
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
        let (dir, store, source, manifest) = image_fixture();
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
        let (_dir, store, source, manifest) = image_fixture();
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
        let (_dir, store, source, mut manifest) = image_fixture();
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
                let (_dir, store, source, manifest) = image_fixture();
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
        let (_dir, store, source, manifest) = image_fixture();
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
        let (_dir, store, source, manifest) = image_fixture();
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

    pub(crate) fn fixture() -> (tempfile::TempDir, Arc<Overlaybd>, String) {
        let (dir, images, source, manifest) = image_fixture();
        let reference = pin(&source, &manifest);
        let manager = Arc::new(Overlaybd {
            images,
            source: Some(source),
            socket: dir.path().join("daemon.sock"),
            users: Mutex::new(BTreeMap::new()),
        });
        (dir, manager, reference)
    }

    #[test]
    fn overlaybd_disabled_backend_and_invalid_runner_config_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let disabled = Overlaybd::new(dir.path(), None).unwrap();
        assert!(!disabled.enabled());
        assert!(
            disabled
                .import("image:latest")
                .unwrap_err()
                .to_string()
                .contains("disabled")
        );
        assert!(matches!(
            disabled.prepare("box", "image:latest", &dir.path().join("disk"), None),
            Err(BoxliteError::Unsupported(_))
        ));
        let result = crate::BoxliteRuntime::new_cloud_runner(
            crate::BoxliteOptions {
                home_dir: dir.path().into(),
                image_registries: vec![],
            },
            Some("relative".into()),
        );
        let Err(error) = result else {
            panic!("invalid runner configuration was accepted");
        };
        assert!(error.to_string().contains(if cfg!(target_os = "linux") {
            "absolute"
        } else {
            "Linux cloud runner"
        }));
        assert!(!dir.path().join("overlaybd").exists());
    }
    #[test]
    fn overlaybd_rebind_preserves_existing_cow_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("device-1");
        let second = dir.path().join("device-2");
        fs::write(&first, vec![0; 65536]).unwrap();
        fs::write(&second, vec![0; 65536]).unwrap();
        let cow = dir.path().join("container.qcow2");
        prepare_cow(&cow, &first, 65536, None).unwrap();
        let before = fs::read(&cow).unwrap();
        prepare_cow(&cow, &second, 65536, None).unwrap();
        let after = fs::read(&cow).unwrap();
        assert_eq!(&before[4096..], &after[4096..]);
        assert_eq!(
            crate::disk::read_backing_file_path(&cow).unwrap().unwrap(),
            second.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(Qcow2Helper::qcow2_virtual_size(&cow).unwrap(), 65536);
        assert!(prepare_cow(&cow, &second, 131072, None).is_err());
        prepare_cow(&cow, &second, 65536, None).unwrap();
        assert_eq!(fs::read(&cow).unwrap(), after);
        assert!(
            matches!(prepare_cow(&dir.path().join("overflow.qcow2"), &first, 65536, Some(u64::MAX)), Err(e) if e.to_string().contains("overflow"))
        );
    }

    #[test]
    fn overlaybd_prepare_rejects_stale_config_and_rolls_back_device_errors() {
        let (dir, manager, reference) = fixture();
        manager.import(&reference).unwrap();
        let path = manager.config_path(image_digest(&reference).unwrap());
        let disk = dir.path().join("disk.qcow2");
        let server = daemon(&manager, 10);
        for _ in 0..2 {
            assert!(manager.prepare("box", &reference, &disk, None).is_err());
            assert!(manager.users.lock().is_empty());
            assert!(!disk.exists());
        }
        assert_eq!(
            server
                .join()
                .unwrap()
                .iter()
                .filter(|op| *op == "/v1/del")
                .count(),
            2
        );
        fs::write(path, b"{}").unwrap();
        assert!(
            matches!(manager.prepare("box", &reference, &disk, None), Err(e) if e.to_string().contains("persisted device config differs"))
        );
        let file = dir.path().join("ordinary-file");
        fs::write(&file, b"not a block device").unwrap();
        assert!(device_capacity(&file).is_err());
    }

    // Exercise the real curl/UDS/JSON boundary; no device or VM is pretended to exist.
    pub(crate) fn daemon(
        manager: &Overlaybd,
        requests: usize,
    ) -> std::thread::JoinHandle<Vec<String>> {
        daemon_with_dropped_responses(manager, requests, &[])
    }

    fn daemon_with_dropped_responses(
        manager: &Overlaybd,
        requests: usize,
        dropped: &[usize],
    ) -> std::thread::JoinHandle<Vec<String>> {
        daemon_with_gate(manager, requests, dropped, || {})
    }

    pub(crate) fn daemon_with_gate(
        manager: &Overlaybd,
        requests: usize,
        dropped: &[usize],
        before_first_response: impl FnOnce() + Send + 'static,
    ) -> std::thread::JoinHandle<Vec<String>> {
        let listener = UnixListener::bind(&manager.socket).unwrap();
        let dropped = dropped.to_vec();
        std::thread::spawn(move || {
            let mut config = None;
            let mut operations = Vec::new();
            let mut gate = Some(before_first_response);
            for index in 0..requests {
                let mut poll = libc::pollfd {
                    fd: listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ready = unsafe { libc::poll(&mut poll, 1, 5000) };
                if ready == 0 && !dropped.is_empty() {
                    break;
                }
                assert_eq!(ready, 1);
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let path = first.split_whitespace().nth(1).unwrap().to_owned();
                let response = match path.as_str() {
                    "/v1/list" => json!({"ok": true, "devices": config.as_ref().map(|c| vec![json!({"dev_id": u32::MAX, "dev": "/dev/ublkb4294967295", "config": c, "writable": false, "state": "running"})]).unwrap_or_default()}),
                    "/v1/add" => {
                        let body: Value = serde_json::from_slice(&body).unwrap();
                        assert!(config.is_none());
                        config = Some(body["config"].as_str().unwrap().to_owned());
                        json!({"ok": true, "dev_id": u32::MAX, "dev": "/dev/ublkb4294967295"})
                    },
                    "/v1/del" => {
                        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["dev_id"], u32::MAX);
                        config = None;
                        json!({"ok": true})
                    },
                    _ => panic!("unexpected operation {path}"),
                }.to_string();
                operations.push(path);
                if let Some(gate) = gate.take() {
                    gate();
                }
                if dropped.contains(&index) {
                    continue;
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            }
            operations
        })
    }

    #[test]
    fn overlaybd_lost_add_response_keeps_device_reclaimable() {
        for dropped in [&[1][..], &[1, 2][..]] {
            let (_dir, manager, reference) = fixture();
            let server = daemon_with_dropped_responses(&manager, dropped.len() + 3, dropped);
            assert!(
                manager
                    .acquire("box-a", image_digest(&reference).unwrap())
                    .is_err()
            );
            // A failed rollback must still be reclaimable when the box is removed.
            manager.release("box-a").unwrap();
            let operations = server.join().unwrap();
            assert_eq!(
                operations.last().map(String::as_str),
                Some("/v1/del"),
                "failed box left its device behind: {operations:?}"
            );
            assert!(manager.users.lock().is_empty());
        }
    }

    #[test]
    fn overlaybd_shares_device_until_last_lease_and_recovers_ownership() {
        let (_dir, manager, reference) = fixture();
        let digest = image_digest(&reference).unwrap();
        fs::create_dir_all(manager.config_path(digest).parent().unwrap()).unwrap();
        let server = daemon(&manager, 7);
        let (_, mut first) = manager.acquire("box-a", digest).unwrap();
        let (_, mut second) = manager.acquire("box-b", digest).unwrap();
        first.disarm();
        second.disarm();
        // Process restart reconstructs leases from surviving shims, without adding another device.
        manager
            .recover(BTreeMap::from([(
                digest.into(),
                BTreeSet::from(["box-a".into(), "box-b".into()]),
            )]))
            .unwrap();
        manager.release("box-a").unwrap();
        assert_eq!(manager.users.lock()[digest].len(), 1);
        manager.release("box-b").unwrap();
        assert!(manager.users.lock().is_empty());
        let ops = server.join().unwrap();
        assert_eq!(ops.iter().filter(|p| *p == "/v1/add").count(), 1);
        assert_eq!(ops.iter().filter(|p| *p == "/v1/del").count(), 1);
    }

    #[test]
    fn overlaybd_failed_preparation_lease_releases_device() {
        let (_dir, manager, reference) = fixture();
        let server = daemon(&manager, 5);
        let (_, lease) = manager
            .acquire("box-a", image_digest(&reference).unwrap())
            .unwrap();
        drop(lease);
        assert!(manager.users.lock().is_empty());
        assert_eq!(server.join().unwrap().last().unwrap(), "/v1/del");
    }
}
