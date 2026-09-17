//! The official index is generated from `spores/*`, never hand-edited, so a
//! pinned hash cannot drift from the file it pins.
//!
//! Regenerate with `UPDATE_INDEX=1 cargo test -p aneural-registry official_index`.

use aneural_core::spore::SporeManifest;
use aneural_registry::index::{Index, entry_for};
use std::path::{Path, PathBuf};

/// Where the first-party spores are served from: this repo, at a tag per spore
/// per version. A tag rather than a branch because a branch can move under a
/// hash the index has already published, which the client treats as
/// tampering. A tag per spore because bumping one spore must not re-pin the
/// others. Cut the tag on the commit that bumps the manifest's `version`.
fn repo_for(name: &str, version: &str) -> String {
    format!("github:Parnassix/aneural#spores/{name}/v{version}")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build() -> Index {
    let root = repo_root();
    let spores_dir = root.join("spores");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&spores_dir)
        .expect("spores/ exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut spores = Vec::new();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let manifest_bytes = std::fs::read(dir.join("spore.json")).unwrap();
        let manifest: SporeManifest = serde_json::from_slice(&manifest_bytes).unwrap();
        assert!(
            manifest.validate().is_empty(),
            "{name} is invalid: {:?}",
            manifest.validate()
        );

        let readme = std::fs::read(dir.join("README.md")).ok();
        let extra: Vec<(&str, &[u8])> = match &readme {
            Some(bytes) => vec![(aneural_registry::README_FILE, bytes.as_slice())],
            None => Vec::new(),
        };
        spores.push(entry_for(
            &manifest,
            &manifest_bytes,
            &repo_for(&name, &manifest.version),
            &format!("spores/{name}"),
            &extra,
        ));
    }

    Index {
        version: 1,
        name: "Aneural Official Spores".into(),
        spores,
        revoked: Vec::new(),
    }
}

#[test]
fn official_index_matches_the_first_party_spores() {
    let index = build();
    let generated = format!("{}\n", serde_json::to_string_pretty(&index).unwrap());
    let path = repo_root().join("registry/index.json");

    if std::env::var("UPDATE_INDEX").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let checked_in = std::fs::read_to_string(&path).expect("registry/index.json is missing");
    assert_eq!(
        checked_in, generated,
        "registry/index.json is stale; rerun with UPDATE_INDEX=1"
    );
}

#[test]
fn every_first_party_spore_has_a_readme() {
    // The README is what the marketplace detail pane shows. A listing without
    // one looks abandoned.
    for entry in build().spores {
        assert!(
            entry.files.contains_key(aneural_registry::README_FILE),
            "{} has no README.md",
            entry.id
        );
        assert!(entry.first_party, "{} is in the official index", entry.id);
        assert!(
            !entry.description.is_empty(),
            "{} has no description",
            entry.id
        );
    }
}
