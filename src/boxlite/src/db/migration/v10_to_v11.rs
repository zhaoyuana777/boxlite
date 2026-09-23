//! Preserve legacy OverlayBD identity before remote sources become available.
use super::{Migration, db_err};
use boxlite_shared::errors::{BoxliteError, BoxliteResult};
use rusqlite::Connection;
use std::path::Path;

pub(crate) struct BindOverlaybdSources;

impl Migration for BindOverlaybdSources {
    fn source_version(&self) -> i32 {
        10
    }
    fn target_version(&self) -> i32 {
        11
    }
    fn description(&self) -> &str {
        "Record legacy OverlayBD sources for local verification"
    }
    fn run(&self, conn: &Connection, _home: Option<&Path>) -> BoxliteResult<()> {
        let tx = db_err!(conn.unchecked_transaction())?;
        db_err!(tx.execute_batch(crate::db::schema::OVERLAYBD_SOURCE_TABLE))?;
        let mut query = db_err!(tx.prepare("SELECT json FROM box_config"))?;
        let rows = db_err!(query.query_map([], |row| row.get::<_, String>(0)))?;
        for row in rows {
            let config: serde_json::Value = serde_json::from_str(&db_err!(row)?)
                .map_err(|e| BoxliteError::Database(e.to_string()))?;
            if config["rootfs_backend"] != "overlaybd" {
                continue;
            }
            let digest = config
                .pointer("/options/rootfs/Image")
                .and_then(|v| v.as_str())
                .and_then(|s| s.rsplit_once("@sha256:").map(|(_, d)| d))
                .filter(|d| {
                    d.len() == 64
                        && d.bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                })
                .ok_or_else(|| {
                    BoxliteError::Database("invalid legacy OverlayBD reference".into())
                })?;
            db_err!(tx.execute(
                "INSERT OR IGNORE INTO overlaybd_source(digest) VALUES (?1)",
                [digest]
            ))?;
        }
        drop(query);
        db_err!(tx.commit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlaybd_upgrade_marks_all_legacy_images_pending_once() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE box_config(json TEXT)")
            .unwrap();
        let digest = "a".repeat(64);
        for backend in ["legacy", "overlaybd", "overlaybd"] {
            conn.execute("INSERT INTO box_config VALUES (?1)", [serde_json::json!({
                "rootfs_backend": backend, "options": {"rootfs": {"Image": format!("image@sha256:{digest}")}}
            }).to_string()]).unwrap();
        }
        BindOverlaybdSources.run(&conn, None).unwrap();
        let rows: (i64, Option<String>) = conn
            .query_row("SELECT count(*), origin FROM overlaybd_source", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(rows, (1, None));
        conn.execute("UPDATE overlaybd_source SET origin = '\"Local\"'", [])
            .unwrap();
        BindOverlaybdSources.run(&conn, None).unwrap();
        assert_eq!(
            conn.query_row("SELECT origin FROM overlaybd_source", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "\"Local\""
        );
        conn.execute(
            "INSERT INTO box_config VALUES (?1)",
            [r#"{"rootfs_backend":"overlaybd","options":{"rootfs":{"Image":"image:latest"}}}"#],
        )
        .unwrap();
        assert!(BindOverlaybdSources.run(&conn, None).is_err());
    }
}
