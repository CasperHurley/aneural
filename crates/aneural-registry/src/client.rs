//! Fetching and merging registry indexes.
//!
//! [`RegistryClient`] is the semantic seam: today the only implementation
//! downloads a static JSON index and searches it locally, but a real HTTP search
//! API can replace it without the GUI noticing.

use crate::index::{Entry, Index, Revocation};
use crate::transport::Transport;
use crate::{Error, Result};
use aneural_core::config::{RegistrySource, SporesConfig};
use std::path::PathBuf;
use std::sync::Mutex;

/// An index is JSON; 8 MiB is generous for a catalogue and still bounded.
pub const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;
/// No single package file is anywhere near this; it bounds a hostile one.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

pub trait RegistryClient: Send + Sync {
    fn name(&self) -> &str;
    fn url(&self) -> &str;
    /// The index, from cache unless `force` or nothing is cached yet.
    fn index(&self, force: bool) -> Result<Index>;
    /// One file of a package, verified against the hash the index pinned.
    fn fetch_file(&self, entry: &Entry, file: &str) -> Result<Vec<u8>>;
}

/// Whether a configured registry could be reached, reported per registry so one
/// dead private index never hides the official one.
#[derive(Clone, Debug, PartialEq)]
pub struct RegistryStatus {
    pub name: String,
    pub url: String,
    pub ok: bool,
    pub spore_count: usize,
    pub error: Option<String>,
}

pub struct StaticIndex<T: Transport> {
    name: String,
    /// The URL as configured. Reported and recorded in the lockfile, so
    /// provenance always names what the user actually pointed at.
    url: String,
    /// What to ask the transport for, which for a directory registry is a path
    /// relative to its root rather than the configured URL.
    fetch_path: String,
    transport: T,
    cache_dir: Option<PathBuf>,
    cached: Mutex<Option<(Index, Option<String>)>>,
}

impl<T: Transport> StaticIndex<T> {
    pub fn new(name: impl Into<String>, url: impl Into<String>, transport: T) -> Self {
        let url = url.into();
        StaticIndex {
            name: name.into(),
            fetch_path: url.clone(),
            url,
            transport,
            cache_dir: None,
            cached: Mutex::new(None),
        }
    }

    /// Fetch from `path` while still reporting the configured `url`.
    pub fn fetching(mut self, path: impl Into<String>) -> Self {
        self.fetch_path = path.into();
        self
    }

    /// Persist the downloaded index here, so the marketplace opens instantly and
    /// still works offline.
    pub fn with_cache(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    fn cache_path(&self) -> Option<PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        Some(dir.join(format!("{}.json", crate::sha256_hex(self.url.as_bytes()))))
    }

    fn read_cache(&self) -> Option<Index> {
        let text = std::fs::read_to_string(self.cache_path()?).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_cache(&self, index: &Index) {
        let Some(path) = self.cache_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string(index) {
            let _ = crate::lock::write_atomic(&path, text.as_bytes());
        }
    }
}

impl<T: Transport> RegistryClient for StaticIndex<T> {
    fn name(&self) -> &str {
        &self.name
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn index(&self, force: bool) -> Result<Index> {
        {
            let cached = self.cached.lock().unwrap();
            if let Some((index, _)) = cached.as_ref()
                && !force
            {
                return Ok(index.clone());
            }
        }
        let etag = self
            .cached
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|(_, tag)| tag.clone());

        let fetched = match self
            .transport
            .get(&self.fetch_path, etag.as_deref(), MAX_INDEX_BYTES)
        {
            Ok(f) => f,
            Err(e) => {
                // A stale index beats a blank marketplace.
                if let Some(index) = self.read_cache() {
                    tracing::warn!(registry = %self.name, "using cached index: {e}");
                    return Ok(index);
                }
                return Err(e);
            }
        };

        if fetched.not_modified
            && let Some((index, _)) = self.cached.lock().unwrap().as_ref()
        {
            return Ok(index.clone());
        }

        let index: Index = serde_json::from_slice(&fetched.body)?;
        self.write_cache(&index);
        *self.cached.lock().unwrap() = Some((index.clone(), fetched.etag));
        Ok(index)
    }

    fn fetch_file(&self, entry: &Entry, file: &str) -> Result<Vec<u8>> {
        let expected = entry
            .files
            .get(file)
            .ok_or_else(|| Error::Invalid(format!("`{}` does not publish a `{file}`", entry.id)))?;
        let url = entry.file_url(file);
        let fetched = self.transport.get(&url, None, MAX_FILE_BYTES)?;
        let actual = crate::sha256_hex(&fetched.body);
        if &actual != expected {
            return Err(Error::Hash {
                id: entry.id.clone(),
                file: file.to_string(),
                expected: expected.clone(),
                actual,
            });
        }
        Ok(fetched.body)
    }
}

/// The configured registries, in order. Resolution is first-match, and
/// revocations are the deliberate exception: they are unioned across every
/// index, so a private registry can withdraw an official spore for its team.
pub struct Federation {
    clients: Vec<Box<dyn RegistryClient>>,
}

impl Federation {
    pub fn new(clients: Vec<Box<dyn RegistryClient>>) -> Self {
        Federation { clients }
    }

    /// Build from config, skipping disabled entries.
    pub fn from_config(
        config: &SporesConfig,
        make: impl Fn(&RegistrySource) -> Result<Box<dyn RegistryClient>>,
    ) -> Result<Self> {
        let mut clients = Vec::new();
        for source in config.registries.iter().filter(|r| !r.disabled) {
            clients.push(make(source)?);
        }
        Ok(Federation::new(clients))
    }

    pub fn clients(&self) -> &[Box<dyn RegistryClient>] {
        &self.clients
    }

    /// Refresh every index. Never fails as a whole: one unreachable registry is
    /// reported in its own status, the rest still load.
    pub fn refresh(&self, force: bool) -> Vec<RegistryStatus> {
        self.clients
            .iter()
            .map(|c| match c.index(force) {
                Ok(index) => RegistryStatus {
                    name: c.name().to_string(),
                    url: c.url().to_string(),
                    ok: true,
                    spore_count: index.spores.len(),
                    error: None,
                },
                Err(e) => RegistryStatus {
                    name: c.name().to_string(),
                    url: c.url().to_string(),
                    ok: false,
                    spore_count: 0,
                    error: Some(e.to_string()),
                },
            })
            .collect()
    }

    /// Every index that loaded, paired with its registry name.
    pub fn indexes(&self) -> Vec<(String, Index)> {
        self.clients
            .iter()
            .filter_map(|c| c.index(false).ok().map(|i| (c.name().to_string(), i)))
            .collect()
    }

    /// The first registry listing `id` provides it.
    pub fn resolve(&self, id: &str) -> Result<(&dyn RegistryClient, Entry)> {
        for client in &self.clients {
            if let Ok(index) = client.index(false)
                && let Some(entry) = index.get(id)
            {
                return Ok((client.as_ref(), entry.clone()));
            }
        }
        Err(Error::NotFound(id.to_string()))
    }

    /// Revocations from every index, not just the providing one.
    pub fn revocation(&self, id: &str, version: &str) -> Option<Revocation> {
        self.clients.iter().find_map(|c| {
            c.index(false)
                .ok()
                .and_then(|i| i.revocation(id, version).cloned())
        })
    }
}

/// Where downloaded indexes are cached: `$XDG_CACHE_HOME/aneural/registry`,
/// falling back to `~/.cache`. Same convention as the GUI's recent-workspaces
/// list, which lives outside any one workspace for the same reason.
pub fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("aneural").join("registry"))
}

/// Build the configured federation, choosing a transport per source: anything
/// with a scheme goes over HTTP, a bare path is read from disk so private and
/// fixture registries need no server.
pub fn federation(config: &SporesConfig) -> Result<Federation> {
    Federation::from_config(config, |source| {
        let client: Box<dyn RegistryClient> =
            if source.url.starts_with("http://") || source.url.starts_with("https://") {
                let mut index = StaticIndex::new(
                    source.name.clone(),
                    source.url.clone(),
                    crate::transport::HttpTransport::new(),
                );
                if let Some(dir) = cache_dir() {
                    index = index.with_cache(dir);
                }
                Box::new(index)
            } else {
                // A filesystem registry: the index path is the root, and files are
                // resolved relative to the directory holding it.
                let path = PathBuf::from(source.url.trim_start_matches("file://"));
                let root = path.parent().map(PathBuf::from).unwrap_or_default();
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "index.json".into());
                Box::new(
                    StaticIndex::new(
                        source.name.clone(),
                        source.url.clone(),
                        crate::transport::DirTransport::new(root),
                    )
                    .fetching(name),
                )
            };
        Ok(client)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::DirTransport;
    use std::collections::BTreeMap;

    fn write_index(root: &std::path::Path, name: &str, index: &Index) {
        std::fs::write(
            root.join(name),
            serde_json::to_string_pretty(index).unwrap(),
        )
        .unwrap();
    }

    fn entry(id: &str, body: &[u8]) -> Entry {
        Entry {
            id: id.into(),
            version: "1.0.0".into(),
            repo: "pkg".into(),
            path: String::new(),
            files: BTreeMap::from([(crate::MANIFEST_FILE.into(), crate::sha256_hex(body))]),
            ..Default::default()
        }
    }

    #[test]
    fn fetches_verifies_and_rejects_tampering() {
        let tmp = tempfile::tempdir().unwrap();
        let body = br#"{"name":"adr","version":"1.0.0"}"#;
        std::fs::create_dir_all(tmp.path().join("pkg")).unwrap();
        std::fs::write(tmp.path().join("pkg").join(crate::MANIFEST_FILE), body).unwrap();

        let e = entry("acme.adr", body);
        write_index(
            tmp.path(),
            "index.json",
            &Index {
                version: 1,
                spores: vec![e.clone()],
                ..Default::default()
            },
        );

        let client = StaticIndex::new("official", "index.json", DirTransport::new(tmp.path()));
        assert_eq!(client.index(false).unwrap().spores.len(), 1);
        assert_eq!(client.fetch_file(&e, crate::MANIFEST_FILE).unwrap(), body);

        // The bytes move under a pinned hash: that is the attack, and it must fail.
        std::fs::write(tmp.path().join("pkg").join(crate::MANIFEST_FILE), b"evil").unwrap();
        let err = client.fetch_file(&e, crate::MANIFEST_FILE).unwrap_err();
        assert!(matches!(err, Error::Hash { .. }), "{err:?}");
    }

    #[test]
    fn federation_resolves_in_order_and_unions_revocations() {
        let tmp = tempfile::tempdir().unwrap();
        write_index(
            tmp.path(),
            "official.json",
            &Index {
                version: 1,
                spores: vec![entry("acme.adr", b"official")],
                ..Default::default()
            },
        );
        write_index(
            tmp.path(),
            "team.json",
            &Index {
                version: 1,
                spores: vec![entry("acme.adr", b"team")],
                revoked: vec![Revocation {
                    id: "bad.thing".into(),
                    versions: vec!["*".into()],
                    reason: "policy".into(),
                }],
                ..Default::default()
            },
        );

        let fed = Federation::new(vec![
            Box::new(StaticIndex::new(
                "team",
                "team.json",
                DirTransport::new(tmp.path()),
            )),
            Box::new(StaticIndex::new(
                "official",
                "official.json",
                DirTransport::new(tmp.path()),
            )),
        ]);

        let (client, _) = fed.resolve("acme.adr").unwrap();
        assert_eq!(client.name(), "team");

        // The revocation lives only in the team index but applies federation-wide.
        assert!(fed.revocation("bad.thing", "1.0.0").is_some());
        assert!(fed.revocation("acme.adr", "1.0.0").is_none());
        assert!(matches!(fed.resolve("nope.nope"), Err(Error::NotFound(_))));
    }

    #[test]
    fn one_dead_registry_does_not_hide_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        write_index(
            tmp.path(),
            "official.json",
            &Index {
                version: 1,
                spores: vec![entry("a.b", b"x")],
                ..Default::default()
            },
        );
        let fed = Federation::new(vec![
            Box::new(StaticIndex::new(
                "gone",
                "missing.json",
                DirTransport::new(tmp.path()),
            )),
            Box::new(StaticIndex::new(
                "official",
                "official.json",
                DirTransport::new(tmp.path()),
            )),
        ]);

        let statuses = fed.refresh(true);
        assert!(!statuses[0].ok);
        assert!(statuses[0].error.is_some());
        assert!(statuses[1].ok);
        assert_eq!(statuses[1].spore_count, 1);
        assert!(fed.resolve("a.b").is_ok());
    }
}
