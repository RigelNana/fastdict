mod support;

use std::fs;
use support::{Corpus, Row};
use wodix::{Error, Limits, ResourceMount, Resources};

fn redirect(target: &str) -> Vec<u8> {
    format!("@@@LINK={target}\0")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[test]
fn arbitrary_mounts_skip_unrelated_damage_and_preserve_binary_payloads() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("unrelated.mdd"), b"broken").unwrap();
    fs::write(
        root.path().join("images-part-seven.mdd"),
        Corpus::resources(vec![Row::new("\\image.bin", vec![0, 255, 17, 0, 10])])
            .serialize()
            .unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("fonts-other-name.mdd"),
        Corpus::resources(vec![Row::new("\\font.bin", vec![31, 0, 253])])
            .serialize()
            .unwrap(),
    )
    .unwrap();
    let mounts = [
        ResourceMount {
            path: "unrelated.mdd".into(),
            key: String::new(),
        },
        ResourceMount {
            path: "images-part-seven.mdd".into(),
            key: String::new(),
        },
        ResourceMount {
            path: "fonts-other-name.mdd".into(),
            key: String::new(),
        },
    ];
    // SAFETY: All test source files are written before opening and never mutated.
    let resources =
        unsafe { Resources::mount_with(root.path(), &mounts, Limits::default()) }.unwrap();
    assert_eq!(resources.cache_bytes(), 0);
    assert_eq!(
        resources.load("image.bin").unwrap().as_ref(),
        &[0, 255, 17, 0, 10]
    );
    assert_eq!(resources.load("font.bin").unwrap().as_ref(), &[31, 0, 253]);
    assert!(matches!(
        resources.load("absent"),
        Err(Error::Resource { .. })
    ));
    assert_eq!(
        resources
            .entries(std::path::Path::new("fonts-other-name.mdd"), "", 10)
            .unwrap(),
        vec![("/font.bin".to_owned(), 3)]
    );
    assert!(resources.cache_bytes() > 0);
}

#[test]
fn resource_namespaces_follow_cross_volume_links_with_strict_errors() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("aliases.mdd"),
        Corpus::resources(vec![
            Row::new("\\alias", redirect("\\target")),
            Row::new("\\cycle", redirect("./CYCLE")),
            Row::new("\\missing", redirect("absent")),
        ])
        .serialize()
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("payload.mdd"),
        Corpus::resources(vec![Row::new("\\target", vec![0, 255, 0])])
            .serialize()
            .unwrap(),
    )
    .unwrap();
    let mounts = [
        ResourceMount {
            path: "aliases.mdd".into(),
            key: "assets".into(),
        },
        ResourceMount {
            path: "payload.mdd".into(),
            key: "assets".into(),
        },
    ];
    // SAFETY: Files and path bindings remain unchanged throughout the test.
    let resources =
        unsafe { Resources::mount_with(root.path(), &mounts, Limits::default()) }.unwrap();
    assert_eq!(
        resources.load("/assets/alias").unwrap().as_ref(),
        &[0, 255, 0]
    );
    assert!(matches!(
        resources.load("assets/cycle"),
        Err(Error::RedirectCycle(_))
    ));
    assert!(matches!(
        resources.load("assets/missing"),
        Err(Error::RedirectMissing(_))
    ));
    assert!(matches!(
        resources.load("target"),
        Err(Error::KeyNotFound(_))
    ));
    assert_eq!(
        resources
            .entries(std::path::Path::new("payload.mdd"), "assets/t", 1)
            .unwrap(),
        vec![("/assets/target".to_owned(), 3)]
    );
}

#[test]
fn explicit_loose_keys_and_precedence_never_expose_unregistered_files() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("actual-name.css"), b"first").unwrap();
    fs::write(root.path().join("other.css"), b"second").unwrap();
    fs::write(root.path().join("private.txt"), b"private").unwrap();
    let mounts = [
        ResourceMount {
            path: "actual-name.css".into(),
            key: "css/style.css".into(),
        },
        ResourceMount {
            path: "other.css".into(),
            key: "css/style.css".into(),
        },
    ];
    // SAFETY: No files or bindings are modified while the collection exists.
    let resources =
        unsafe { Resources::mount_with(root.path(), &mounts, Limits::default()) }.unwrap();
    assert_eq!(
        resources.load("\\CSS\\STYLE.CSS").unwrap().as_ref(),
        b"first"
    );
    assert!(matches!(
        resources.load("actual-name.css"),
        Err(Error::KeyNotFound(_))
    ));
    assert!(matches!(
        resources.load("private.txt"),
        Err(Error::KeyNotFound(_))
    ));
    assert!(matches!(
        resources.load("../private.txt"),
        Err(Error::Corrupt(_))
    ));
    assert!(matches!(
        resources.load("C:/private.txt"),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn discovered_numbered_volumes_do_not_require_contiguous_numbers() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("dict.7.mdd"),
        Corpus::basic_mdd().serialize().unwrap(),
    )
    .unwrap();
    // SAFETY: Sources and paths are immutable after construction.
    let resources = unsafe { Resources::discover(&root.path().join("dict.mdx")) }.unwrap();
    assert_eq!(
        resources.load("hello.txt").unwrap().as_ref(),
        b"hello resource"
    );
}

#[cfg(unix)]
#[test]
fn explicit_symlinks_cannot_escape_managed_root() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("revision");
    fs::create_dir(&root).unwrap();
    fs::write(parent.path().join("secret"), b"secret").unwrap();
    std::os::unix::fs::symlink(parent.path().join("secret"), root.join("linked.css")).unwrap();
    let mounts = [ResourceMount {
        path: "linked.css".into(),
        key: "style.css".into(),
    }];
    // SAFETY: Files and symlink bindings remain unchanged throughout this test.
    let resources = unsafe { Resources::mount_with(&root, &mounts, Limits::default()) }.unwrap();
    assert!(matches!(
        resources.load("style.css"),
        Err(Error::Corrupt(_))
    ));
}
