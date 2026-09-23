//! Persistent image origins are independent of each runtime's creation default.
use super::*;
use crate::db::Database;
use crate::runtime::options::ImageRegistry;
use rusqlite::OptionalExtension;
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Origin {
    Local,
    Registry(String),
}

impl Overlaybd {
    pub(crate) fn new_registry(
        home: &Path,
        registries: Vec<ImageRegistry>,
    ) -> BoxliteResult<Arc<Self>> {
        super::super::store::validate_image_registries(&registries)?;
        if registries.iter().any(|r| r.skip_verify) {
            return Err(error(
                "remote layers require verified TLS; daemon credentials are configured separately",
            ));
        }
        let mut manager = Self::new(home, Some(home.to_owned()))?;
        Arc::get_mut(&mut manager).unwrap().source = Some(ImageSource::Registry(registries));
        Ok(manager)
    }

    fn database(&self) -> BoxliteResult<Database> {
        self.db
            .lock()
            .clone()
            .ok_or_else(|| error("source database is not initialized"))
    }

    pub(super) fn bind_source(&self, digest: &str, origin: &Origin) -> BoxliteResult<()> {
        let db = self.database()?;
        let conn = db.conn();
        let encoded = serde_json::to_string(origin).map_err(error)?;
        // The runtime home lock excludes other runtimes; SQLite serializes concurrent creates.
        conn.execute("INSERT INTO overlaybd_source(digest, origin) VALUES (?1, ?2) ON CONFLICT(digest) DO NOTHING",
            rusqlite::params![digest, encoded]).map_err(error)?;
        let stored: Option<String> = conn
            .query_row(
                "SELECT origin FROM overlaybd_source WHERE digest = ?1",
                [digest],
                |row| row.get(0),
            )
            .map_err(error)?;
        if stored.as_deref() != Some(&encoded) {
            return Err(error(format!(
                "source conflict for sha256:{digest}; existing boxes retain their origin"
            )));
        }
        Ok(())
    }

    pub(super) fn read_source(&self, digest: &str) -> BoxliteResult<Origin> {
        let db = self.database()?;
        let stored: Option<Option<String>> = db
            .conn()
            .query_row(
                "SELECT origin FROM overlaybd_source WHERE digest = ?1",
                [digest],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        let encoded = stored
            .flatten()
            .ok_or_else(|| error(format!("missing or unverified source for sha256:{digest}")))?;
        serde_json::from_str(&encoded).map_err(error)
    }

    pub(super) fn lower(
        &self,
        reference: &str,
        origin: &Origin,
    ) -> BoxliteResult<(ContainerImageConfig, Value)> {
        match origin {
            Origin::Local => {
                let (config, layers) = self.images.import_image(&self.images.root, reference)?;
                let lowers: Vec<_> = layers.iter().map(|path| json!({"file": path})).collect();
                Ok((config, json!({"lowers": lowers})))
            }
            Origin::Registry(url) => {
                let parsed = reqwest::Url::parse(url)
                    .map_err(|_| error("invalid persisted registry origin"))?;
                if !matches!(parsed.scheme(), "http" | "https")
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.query().is_some()
                    || parsed.fragment().is_some()
                    || !parsed.path().starts_with("/v2/")
                    || !parsed.path().ends_with("/blobs")
                {
                    return Err(error("invalid persisted registry origin"));
                }
                let digest = image_digest(reference)?;
                let path = self.images.root.join("metadata").join(digest);
                let manifest = parse_manifest(
                    &read_bounded(&path.join("manifest.json"))?,
                    &format!("sha256:{digest}"),
                )?;
                let config =
                    parse_config(&read_bounded(&path.join("config.json"))?, &manifest.config)?;
                let lowers: Vec<_> = manifest
                    .layers
                    .iter()
                    .map(|blob| json!({"digest": blob.digest, "size": blob.size}))
                    .collect();
                Ok((config, json!({"repoBlobUrl": url, "lowers": lowers})))
            }
        }
    }

    pub(super) fn check_lower(&self, path: &Path, expected: &Value) -> BoxliteResult<()> {
        let stored: Value = serde_json::from_slice(&read_bounded(path)?).map_err(error)?;
        if &stored != expected {
            return Err(error("persisted device config differs from pinned image"));
        }
        Ok(())
    }

    /// Only schema migration can create a pending local origin. Missing rows never trigger repair.
    pub(crate) fn initialize_sources(
        &self,
        db: Database,
        references: &[String],
    ) -> BoxliteResult<()> {
        *self.db.lock() = Some(db.clone());
        let pending = {
            let conn = db.conn();
            let mut query = conn
                .prepare("SELECT digest FROM overlaybd_source WHERE origin IS NULL")
                .map_err(error)?;
            query
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(error)?
        };
        for digest in pending {
            let (_, lower) = self.lower(&format!("legacy@sha256:{digest}"), &Origin::Local)?;
            let path = self.config_path(&digest);
            if path.exists() {
                self.check_lower(&path, &lower)?;
            }
            db.conn()
                .execute(
                    "UPDATE overlaybd_source SET origin = ?2 WHERE digest = ?1 AND origin IS NULL",
                    rusqlite::params![
                        digest,
                        serde_json::to_string(&Origin::Local).map_err(error)?
                    ],
                )
                .map_err(error)?;
        }
        let mut checked = BTreeSet::new();
        for reference in references {
            let digest = image_digest(reference)?;
            if !checked.insert(digest) {
                continue;
            }
            let (_, lower) = self.lower(reference, &self.read_source(digest)?)?;
            let path = self.config_path(digest);
            if path.exists() {
                self.check_lower(&path, &lower)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
