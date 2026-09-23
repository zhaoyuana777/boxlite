use super::super::tests::{fixture, image_fixture};
use super::*;

#[test]
fn origin_binding_is_atomic_and_survives_restart() {
    let (_dir, manager, reference) = fixture();
    let digest = image_digest(&reference).unwrap();
    let origins = [
        Origin::Local,
        Origin::Registry("https://registry.test/v2/image/blobs".into()),
    ];
    let barrier = std::sync::Barrier::new(2);
    let wins = std::thread::scope(|scope| {
        let jobs: Vec<_> = origins
            .iter()
            .map(|origin| {
                let (manager, barrier) = (&manager, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    manager.bind_source(digest, origin).is_ok()
                })
            })
            .collect();
        jobs.into_iter()
            .map(|job| usize::from(job.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(wins, 1);
    let stored = manager.read_source(digest).unwrap();
    manager.bind_source(digest, &stored).unwrap();
    assert!(
        manager
            .bind_source(digest, origins.iter().find(|o| **o != stored).unwrap())
            .is_err()
    );
    let db = manager.database().unwrap();
    *manager.db.lock() = None;
    assert!(manager.read_source(digest).is_err());
    *manager.db.lock() = Some(db);
    assert_eq!(manager.read_source(digest).unwrap(), stored);
    manager
        .database()
        .unwrap()
        .conn()
        .execute("UPDATE overlaybd_source SET origin = '{}'", [])
        .unwrap();
    assert!(manager.read_source(digest).is_err());
}

#[test]
fn migration_verifies_local_cache_and_never_repairs_lost_bindings() {
    let (_dir, manager, reference) = fixture();
    let ImageSource::Local(source) = manager.source.as_ref().unwrap() else {
        panic!()
    };
    manager.images.import(source, &reference).unwrap();
    let digest = image_digest(&reference).unwrap();
    let db = manager.database().unwrap();
    db.conn()
        .execute("INSERT INTO overlaybd_source(digest) VALUES (?1)", [digest])
        .unwrap();
    let path = manager.config_path(digest);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{}").unwrap();
    assert!(
        manager
            .initialize_sources(db.clone(), std::slice::from_ref(&reference))
            .is_err()
    );
    assert!(manager.read_source(digest).is_err());
    fs::remove_file(&path).unwrap();
    manager
        .initialize_sources(db.clone(), &[reference.clone(), reference.clone()])
        .unwrap();
    assert_eq!(manager.read_source(digest).unwrap(), Origin::Local);
    fs::remove_dir_all(source).unwrap();
    manager
        .initialize_sources(db.clone(), std::slice::from_ref(&reference))
        .unwrap();
    db.conn()
        .execute("DELETE FROM overlaybd_source", [])
        .unwrap();
    assert!(
        manager
            .initialize_sources(db.clone(), std::slice::from_ref(&reference))
            .unwrap_err()
            .to_string()
            .contains("missing")
    );
    db.conn()
        .execute("INSERT INTO overlaybd_source(digest) VALUES (?1)", [digest])
        .unwrap();
    fs::remove_dir_all(manager.images.root.join("blobs")).unwrap();
    assert!(manager.initialize_sources(db, &[reference]).is_err());
}

#[tokio::test]
async fn remote_create_binds_origin_and_prepare_uses_cached_metadata() {
    use super::super::metadata::tests::Registry;
    let (_image, _, source, manifest) = image_fixture();
    let server = Registry::new(None).await;
    let reference = server.image(&source, &manifest);
    let (dir, mut manager, _) = fixture();
    Arc::get_mut(&mut manager).unwrap().source = Some(ImageSource::Registry(server.options()));
    manager.import_async(&reference).await.unwrap();
    manager.import_async(&reference).await.unwrap();
    let digest = image_digest(&reference).unwrap();
    let origin = manager.read_source(digest).unwrap();
    drop(server);
    let (_, lower) = manager.lower(&reference, &origin).unwrap();
    assert_eq!(
        lower["lowers"][0],
        json!({"digest": manifest["layers"][0]["digest"], "size": manifest["layers"][0]["size"]})
    );
    assert!(
        lower["repoBlobUrl"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    assert!(!manager.images.root.join("blobs").exists());
    let db = manager.database().unwrap();
    manager
        .initialize_sources(db.clone(), std::slice::from_ref(&reference))
        .unwrap();
    assert!(manager.bind_source(digest, &Origin::Local).is_err());
    assert!(
        manager
            .bind_source(
                digest,
                &Origin::Registry("https://other.test/v2/image/blobs".into())
            )
            .is_err()
    );
    let daemon = super::super::tests::daemon(&manager, 5);
    assert!(
        manager
            .prepare("remote", &reference, &dir.path().join("disk"), None)
            .is_err()
    );
    assert!(manager.users.lock().is_empty());
    assert_eq!(daemon.join().unwrap().last().unwrap(), "/v1/del");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(manager.config_path(digest)).unwrap()).unwrap(),
        lower
    );
    manager
        .initialize_sources(db, std::slice::from_ref(&reference))
        .unwrap();
    for url in [
        "bad",
        "ftp://host/v2/image/blobs",
        "https://user@host/v2/image/blobs",
        "https://host/v2/image/blobs?token=TEST_ONLY",
        "https://host/wrong",
    ] {
        assert!(
            manager
                .lower(&reference, &Origin::Registry(url.into()))
                .is_err()
        );
    }
    fs::write(
        manager
            .images
            .root
            .join("metadata")
            .join(digest)
            .join("config.json"),
        "corrupt",
    )
    .unwrap();
    assert!(manager.lower(&reference, &origin).is_err());
}

#[test]
fn registry_constructor_rejects_unsupported_transport_and_platform() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Overlaybd::new_registry(dir.path(), vec![ImageRegistry::https("bad/host")]).is_err());
    assert!(
        Overlaybd::new_registry(
            dir.path(),
            vec![ImageRegistry::https("registry.test").with_skip_verify(true)]
        )
        .is_err()
    );
    let result = Overlaybd::new_registry(dir.path(), vec![]);
    assert_eq!(result.is_ok(), cfg!(target_os = "linux"));
}
