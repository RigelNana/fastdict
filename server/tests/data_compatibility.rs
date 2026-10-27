use fastdict::store::Store;

#[test]
fn existing_database_preserves_settings_and_new_installs_use_fastdict() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    {
        let store = Store::open(root.path())?;
        assert_eq!(store.path, root.path().join("fastdict.sqlite3"));
        store.update(|snapshot| {
            snapshot.settings["appearance"]["theme"] = "dark".into();
            Ok(())
        })?;
        store.log("info", "rename", "existing installation");
    }
    std::fs::rename(root.path().join("fastdict.sqlite3"), root.path().join("mydict.sqlite3"))?;
    let store = Store::open(root.path())?;
    assert_eq!(store.path, root.path().join("mydict.sqlite3"));
    assert_eq!(store.logs(10)?[0].message, "existing installation");
    assert_eq!(store.load()?.settings["appearance"]["theme"], "dark");
    assert!(!root.path().join("fastdict.sqlite3").exists());
    Ok(())
}
