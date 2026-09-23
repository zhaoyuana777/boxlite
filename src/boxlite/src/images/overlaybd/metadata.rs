use std::time::Duration;

use oci_client::{Reference, client::ClientProtocol, secrets::RegistryAuth};

use super::*;
use crate::images::store::{
    client_config_for_registry, registry_auth_for, validate_image_registries,
};
use crate::runtime::options::ImageRegistry;

/// Verified metadata only: layer contents have not been fetched or verified.
#[derive(Debug)]
pub struct OverlaybdMetadata {
    pub reference: Reference,
    /// Stable candidate origin for the daemon; contains no credentials or signed URL.
    /// The device manager, not metadata fetching, owns source binding.
    pub repo_blob_url: String,
    pub manifest: Manifest,
    pub config: ContainerImageConfig,
}

impl OverlaybdImages {
    /// Fetch only a pinned manifest and config, retaining native layer descriptors.
    ///
    /// Publishes raw verified metadata under `overlaybd/metadata/<digest>/` atomically.
    /// Does not create a Box, bind a source, fetch layers or mark an OCI image complete.
    /// Registry credentials here authorize metadata only, not the external daemon.
    pub async fn pull_metadata(
        &self,
        reference: &str,
        registries: &[ImageRegistry],
    ) -> BoxliteResult<OverlaybdMetadata> {
        validate_image_registries(registries)?;
        let reference: Reference = reference
            .parse()
            .map_err(|_| error("invalid OCI image reference"))?;
        let digest = reference
            .digest()
            .ok_or_else(|| error("image reference must pin a manifest with @sha256"))?;
        digest_hex(digest)?;
        let (metadata, manifest_bytes, config_bytes) = tokio::time::timeout(
            Duration::from_secs(30),
            fetch_metadata(reference, registries),
        )
        .await
        .map_err(|_| error("registry metadata request timed out after 30s"))??;
        let root = self.root.join("metadata");
        let digest = metadata.reference.digest().unwrap().to_owned();
        tokio::task::spawn_blocking(move || {
            cache_metadata(&root, &digest, &manifest_bytes, &config_bytes)
        })
        .await
        .map_err(|e| error(format!("metadata cache task failed: {e}")))??;
        Ok(metadata)
    }
}

async fn fetch_metadata(
    reference: Reference,
    registries: &[ImageRegistry],
) -> BoxliteResult<(OverlaybdMetadata, Vec<u8>, Vec<u8>)> {
    let mut settings = client_config_for_registry(reference.registry(), registries);
    settings.connect_timeout = Some(Duration::from_secs(5));
    settings.read_timeout = Some(Duration::from_secs(10));
    let scheme = if matches!(settings.protocol, ClientProtocol::Http) {
        "http"
    } else {
        "https"
    };
    // oci-client 0.15 buffers manifest responses without a size limit. Reuse its
    // authentication, but stream metadata through our existing HTTP dependency.
    let client = reqwest::Client::builder()
        .https_only(scheme == "https")
        .danger_accept_invalid_certs(settings.accept_invalid_certificates)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| error(e.without_url()))?;
    let auth = registry_auth_for(reference.registry(), registries);
    let token = oci_client::Client::try_from(settings)
        .map_err(|_| error("could not configure registry authentication"))?
        .auth(&reference, &auth, oci_client::RegistryOperation::Pull)
        .await
        // Auth errors can contain server bodies, tokens or signed URLs.
        .map_err(|_| error(format!("registry authentication failed for {}", reference.registry())))?;
    let base = format!(
        "{scheme}://{}/v2/{}",
        reference.resolve_registry(),
        reference.repository()
    );
    let request = |url: String| {
        let request = client.get(url);
        if let Some(token) = &token {
            request.bearer_auth(token)
        } else if let RegistryAuth::Basic(user, password) = &auth {
            request.basic_auth(user, Some(password))
        } else {
            request
        }
    };
    let digest = reference.digest().unwrap();
    let manifest_bytes = read_response(
        request(format!("{base}/manifests/{digest}")).header(
            reqwest::header::ACCEPT,
            "application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json",
        ),
    )
    .await?;
    let manifest = parse_manifest(&manifest_bytes, digest)?;
    if manifest.config.size as u64 > MAX_JSON {
        return Err(error("config JSON exceeds 4 MiB"));
    }
    let config_bytes =
        read_response(request(format!("{base}/blobs/{}", manifest.config.digest))).await?;
    let config = parse_config(&config_bytes, &manifest.config)?;
    Ok((
        OverlaybdMetadata {
            reference,
            repo_blob_url: format!("{base}/blobs"),
            manifest,
            config,
        },
        manifest_bytes,
        config_bytes,
    ))
}

async fn read_response(request: reqwest::RequestBuilder) -> BoxliteResult<Vec<u8>> {
    let mut response = request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| error(e.without_url()))?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_JSON)
    {
        return Err(error("registry JSON exceeds 4 MiB"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| error(e.without_url()))? {
        if bytes.len() + chunk.len() > MAX_JSON as usize {
            return Err(error("registry JSON exceeds 4 MiB"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn cache_metadata(root: &Path, digest: &str, manifest: &[u8], config: &[u8]) -> BoxliteResult<()> {
    fs::create_dir_all(root).map_err(error)?;
    let staged = tempfile::tempdir_in(root).map_err(error)?;
    for (name, bytes) in [("manifest.json", manifest), ("config.json", config)] {
        let mut file = File::create(staged.path().join(name)).map_err(error)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(error)?;
    }
    File::open(staged.path())
        .and_then(|f| f.sync_all())
        .map_err(error)?;
    let path = root.join(digest_hex(digest)?);
    if let Err(cause) = fs::rename(staged.path(), &path) {
        // A concurrent pull can win publication. Never trust existence alone.
        if !path.is_dir() {
            return Err(error(cause));
        }
        for (name, expected) in [("manifest.json", manifest), ("config.json", config)] {
            if read_bounded(&path.join(name))? != expected {
                return Err(error("cached metadata differs from pinned image"));
            }
        }
    }
    File::open(root).and_then(|f| f.sync_all()).map_err(error)
}

#[cfg(test)]
pub(super) mod tests;
