//! `.aneural/spores.lock` — what is installed, from where, and at what hash.
//!
//! Install records nothing today, so `spores update` cannot show a diff and a
//! changed hash is invisible. The lockfile is meant to be committed: a team then
//! gets byte-identical spores, and `verify` can tell drift from tampering.

use crate::{Error, Result, sha256_hex};
use aneural_core::spore::Capability;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const LOCKFILE_VERSION: u32 = 1;
pub const LOCKFILE_NAME: &str = "spores.lock";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lockfile {
    pub lockfile_version: u32,
    /// Spore id -> what we installed. `BTreeMap` so the file has a stable order.
    #[serde(default)]
    pub spores: BTreeMap<String, LockEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockEntry {
    pub version: String,
    /// The configured registry name this came from, or `direct` for a URL install.
    pub registry: String,
    pub registry_url: String,
    /// The `repo` and `path` the files were fetched from, for provenance.
    pub repo: String,
    #[serde(default)]
    pub path: String,
    pub tier: String,
    /// What the user consented to at install time. Re-prompt when this grows.
    #[serde(default)]
    pub granted: Vec<Capability>,
    /// Published file name -> sha256 of what we wrote.
    pub files: BTreeMap<String, String>,
    /// One sha256 over the whole file set, so `verify` compares one value.
    pub integrity: String,
    pub installed_at: String,
}

impl LockEntry {
    /// sha256 over `"<file>\0<hash>\n"` for every file, in sorted order.
    pub fn compute_integrity(files: &BTreeMap<String, String>) -> String {
        let mut joined = String::new();
        for (name, hash) in files {
            joined.push_str(name);
            joined.push('\0');
            joined.push_str(hash);
            joined.push('\n');
        }
        sha256_hex(joined.as_bytes())
    }
}

/// How the installed files differ from what the lockfile recorded.
#[derive(Clone, Debug, PartialEq)]
pub enum Drift {
    /// The file is there but its bytes changed. People do legitimately hand-edit
    /// an installed spore, so this is a warning, not a failure.
    Modified {
        id: String,
        file: String,
    },
    Missing {
        id: String,
        file: String,
    },
    /// Installed on disk but absent from the lockfile — a hand-authored spore.
    NotInLock {
        id: String,
    },
    /// In the lockfile but not on disk.
    NotInstalled {
        id: String,
    },
}

impl Lockfile {
    pub fn path(root: &Path) -> std::path::PathBuf {
        root.join(".aneural").join(LOCKFILE_NAME)
    }

    /// A missing lockfile is an empty one, not an error.
    pub fn load(root: &Path) -> Result<Self> {
        let path = Self::path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lockfile {
                lockfile_version: LOCKFILE_VERSION,
                spores: BTreeMap::new(),
            }),
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = format!("{}\n", serde_json::to_string_pretty(self)?);
        write_atomic(&path, text.as_bytes())
    }

    pub fn get(&self, id: &str) -> Option<&LockEntry> {
        self.spores.get(id)
    }

    /// Compare the lockfile against what is actually in `.aneural/spores`.
    pub fn verify(&self, root: &Path) -> Result<Vec<Drift>> {
        let mut out = Vec::new();
        let spores_dir = root.join(".aneural").join("spores");

        for (id, entry) in &self.spores {
            let dir = spores_dir.join(id);
            if !dir.is_dir() {
                out.push(Drift::NotInstalled { id: id.clone() });
                continue;
            }
            for (file, expected) in &entry.files {
                match std::fs::read(dir.join(file)) {
                    Ok(bytes) if &sha256_hex(&bytes) != expected => out.push(Drift::Modified {
                        id: id.clone(),
                        file: file.clone(),
                    }),
                    Ok(_) => {}
                    Err(_) => out.push(Drift::Missing {
                        id: id.clone(),
                        file: file.clone(),
                    }),
                }
            }
        }

        if let Ok(entries) = std::fs::read_dir(&spores_dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if e.path().is_dir() && !self.spores.contains_key(&name) {
                    out.push(Drift::NotInLock { id: name });
                }
            }
        }
        out.sort_by_key(|d| format!("{d:?}"));
        Ok(out)
    }
}

/// Write via a sibling temp file and rename, so a crash never leaves a half file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> LockEntry {
        let files = BTreeMap::from([(crate::MANIFEST_FILE.to_string(), sha256_hex(b"{}"))]);
        LockEntry {
            version: "1.0.0".into(),
            registry: "official".into(),
            registry_url: "https://aneural.dev/registry/index.json".into(),
            repo: "github:acme/adr".into(),
            path: "spore".into(),
            tier: "declarative".into(),
            granted: Vec::new(),
            integrity: LockEntry::compute_integrity(&files),
            files,
            installed_at: aneural_core::now_rfc3339(),
        }
    }

    #[test]
    fn missing_lockfile_loads_as_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = Lockfile::load(tmp.path()).unwrap();
        assert!(lock.spores.is_empty());
    }

    #[test]
    fn round_trips_and_keeps_a_stable_order() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lock = Lockfile::load(tmp.path()).unwrap();
        lock.spores.insert("zed.z".into(), entry());
        lock.spores.insert("acme.adr".into(), entry());
        lock.save(tmp.path()).unwrap();

        let text = std::fs::read_to_string(Lockfile::path(tmp.path())).unwrap();
        assert!(text.find("acme.adr").unwrap() < text.find("zed.z").unwrap());
        assert_eq!(Lockfile::load(tmp.path()).unwrap(), lock);
    }

    #[test]
    fn verify_spots_modified_missing_and_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".aneural/spores/acme.adr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::MANIFEST_FILE), b"{}").unwrap();

        let mut lock = Lockfile::load(tmp.path()).unwrap();
        lock.spores.insert("acme.adr".into(), entry());
        assert!(lock.verify(tmp.path()).unwrap().is_empty());

        std::fs::write(dir.join(crate::MANIFEST_FILE), b"{\"tampered\":1}").unwrap();
        assert_eq!(
            lock.verify(tmp.path()).unwrap(),
            vec![Drift::Modified {
                id: "acme.adr".into(),
                file: crate::MANIFEST_FILE.into()
            }]
        );

        std::fs::remove_file(dir.join(crate::MANIFEST_FILE)).unwrap();
        assert_eq!(
            lock.verify(tmp.path()).unwrap(),
            vec![Drift::Missing {
                id: "acme.adr".into(),
                file: crate::MANIFEST_FILE.into()
            }]
        );

        // A hand-authored spore is reported, not treated as an error.
        std::fs::create_dir_all(tmp.path().join(".aneural/spores/mine.local")).unwrap();
        assert!(
            lock.verify(tmp.path())
                .unwrap()
                .contains(&Drift::NotInLock {
                    id: "mine.local".into()
                })
        );
    }

    #[test]
    fn integrity_covers_every_file() {
        let a = BTreeMap::from([("spore.json".to_string(), "aa".to_string())]);
        let b = BTreeMap::from([
            ("spore.json".to_string(), "aa".to_string()),
            ("README.md".to_string(), "bb".to_string()),
        ]);
        assert_ne!(
            LockEntry::compute_integrity(&a),
            LockEntry::compute_integrity(&b)
        );
    }
}
