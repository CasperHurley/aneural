//! The registry index format.
//!
//! An index is a single JSON document served over HTTPS (or read from disk for
//! private and development registries). It is deliberately static: the client
//! downloads it once, caches it, and searches locally, so browsing the
//! marketplace costs one request and works offline afterwards.

use aneural_core::spore::{Capability, Tier};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub version: u32,
    /// Human label for the registry itself, shown in the marketplace.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub spores: Vec<Entry>,
    /// Spores pulled from circulation. Checked on install and on update.
    #[serde(default)]
    pub revoked: Vec<Revocation>,
}

impl Index {
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.spores.iter().find(|e| e.id == id)
    }

    /// The revocation covering this exact version, if any.
    pub fn revocation(&self, id: &str, version: &str) -> Option<&Revocation> {
        self.revoked
            .iter()
            .find(|r| r.id == id && r.covers(version))
    }
}

/// One listing. `files` is the whole package: every path the client may
/// download, pinned by sha256. Anything not listed here is never fetched.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// `publisher.name`.
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    /// `github:owner/repo[#ref]`, or an https base URL.
    pub repo: String,
    /// Directory inside the repo holding `spore.json`.
    #[serde(default)]
    pub path: String,
    /// Published file name -> lowercase hex sha256. Must contain `spore.json`.
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub node_kinds: Vec<String>,
    /// What the listing claims the spore will ask for. Verified against the
    /// manifest after download; a mismatch aborts the install.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// Whether this listing is published by the Aneural project itself.
    #[serde(default)]
    pub first_party: bool,
}

impl Entry {
    pub fn publisher(&self) -> &str {
        self.id.split_once('.').map(|(p, _)| p).unwrap_or(&self.id)
    }

    pub fn name(&self) -> &str {
        self.id.split_once('.').map(|(_, n)| n).unwrap_or(&self.id)
    }

    /// Derived the same way as on the manifest, from the advertised capabilities.
    pub fn tier(&self) -> Tier {
        self.capabilities
            .iter()
            .map(Capability::tier)
            .max()
            .unwrap_or(Tier::Declarative)
    }

    /// The base URL for this entry's files.
    pub fn base_url(&self) -> String {
        let base = match parse_github(&self.repo) {
            Some((owner, repo, git_ref)) => {
                format!("https://raw.githubusercontent.com/{owner}/{repo}/{git_ref}")
            }
            None => self.repo.trim_end_matches('/').to_string(),
        };
        let path = self.path.trim_matches('/');
        if path.is_empty() {
            base
        } else {
            format!("{base}/{path}")
        }
    }

    pub fn file_url(&self, file: &str) -> String {
        format!("{}/{}", self.base_url(), file)
    }

    /// Structural checks run before anything is downloaded.
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        if !self.id.contains('.') {
            errs.push(format!("id `{}` must be `publisher.name`", self.id));
        }
        if semver::Version::parse(&self.version).is_err() {
            errs.push(format!("version `{}` is not valid semver", self.version));
        }
        if !self.files.contains_key(crate::MANIFEST_FILE) {
            errs.push(format!("no `{}` in files", crate::MANIFEST_FILE));
        }
        for (file, hash) in &self.files {
            if !crate::ALLOWED_FILES.contains(&file.as_str()) {
                errs.push(format!("file `{file}` is not an allowed package file"));
            }
            if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                errs.push(format!("file `{file}` has no valid sha256"));
            }
        }
        if self.repo.is_empty() {
            errs.push("repo must not be empty".into());
        }
        // The ceiling for anything listed anywhere is the WASM sandbox. Native
        // code only ever arrives by a direct URL the user typed themselves, so a
        // registry cannot become a distribution channel for it.
        if self.tier() == Tier::Native {
            errs.push(format!(
                "{}: native spores are never listed; they can only be installed from a direct URL",
                self.id
            ));
        }
        errs
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Revocation {
    pub id: String,
    /// Exact versions withdrawn, or `["*"]` for the whole spore.
    #[serde(default)]
    pub versions: Vec<String>,
    #[serde(default)]
    pub reason: String,
}

impl Revocation {
    pub fn covers(&self, version: &str) -> bool {
        self.versions.is_empty() || self.versions.iter().any(|v| v == "*" || v == version)
    }
}

/// `github:owner/repo[#ref]` -> `(owner, repo, ref)`, defaulting to `main`.
fn parse_github(repo: &str) -> Option<(&str, &str, &str)> {
    let rest = repo.strip_prefix("github:")?;
    let (owner, rest) = rest.split_once('/')?;
    let (name, git_ref) = match rest.split_once('#') {
        Some((n, r)) => (n, r),
        None => (rest, "main"),
    };
    if owner.is_empty() || name.is_empty() || git_ref.is_empty() {
        return None;
    }
    Some((owner, name, git_ref))
}

/// Build an index entry from a manifest that lives at `path` inside `repo`.
///
/// The official index is generated from `spores/*` rather than hand-written, so
/// a hash can never drift from the file it pins.
pub fn entry_for(
    manifest: &aneural_core::spore::SporeManifest,
    manifest_bytes: &[u8],
    repo: &str,
    path: &str,
    extra_files: &[(&str, &[u8])],
) -> Entry {
    let mut files = BTreeMap::from([(
        crate::MANIFEST_FILE.to_string(),
        crate::sha256_hex(manifest_bytes),
    )]);
    for (name, bytes) in extra_files {
        files.insert((*name).to_string(), crate::sha256_hex(bytes));
    }
    Entry {
        id: manifest.id(),
        version: manifest.version.clone(),
        display_name: manifest.display_name.clone(),
        description: manifest.description.clone(),
        license: manifest.license.clone(),
        repo: repo.to_string(),
        path: path.to_string(),
        files,
        keywords: manifest.keywords.clone(),
        categories: manifest.categories.clone(),
        node_kinds: manifest.node_types.iter().map(|n| n.kind.clone()).collect(),
        capabilities: manifest.capabilities.clone(),
        first_party: manifest.is_first_party(),
    }
}

/// Check a whole index: structure, then fetch and verify every listed file.
///
/// This is what registry CI runs on a submission. The capability cross-check is
/// the important one — a listing that advertises less than its manifest asks for
/// is how a spore would slip past the consent sheet.
pub fn validate_index(index: &Index, client: &dyn crate::RegistryClient) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for entry in &index.spores {
        let id = &entry.id;
        if !seen.insert(id.clone()) {
            problems.push(format!("{id}: listed more than once"));
            continue;
        }
        for p in entry.validate() {
            problems.push(format!("{id}: {p}"));
        }
        if entry.repo.starts_with("github:") {
            let git_ref = entry.repo.split_once('#').map(|(_, r)| r);
            match git_ref {
                None => problems.push(format!(
                    "{id}: repo must name a ref (`github:owner/repo#v1.2.0`)"
                )),
                Some("main" | "master") => problems.push(format!(
                    "{id}: repo is pinned to a branch, which can move under a published hash; \
                     use a tag or a commit"
                )),
                Some(_) => {}
            }
        }

        // Fetching also verifies each file's sha256.
        let manifest_bytes = match client.fetch_file(entry, crate::MANIFEST_FILE) {
            Ok(bytes) => bytes,
            Err(e) => {
                problems.push(format!("{id}: {e}"));
                continue;
            }
        };
        for file in entry.files.keys().filter(|f| *f != crate::MANIFEST_FILE) {
            if let Err(e) = client.fetch_file(entry, file) {
                problems.push(format!("{id}: {e}"));
            }
        }

        let manifest: aneural_core::spore::SporeManifest =
            match serde_json::from_slice(&manifest_bytes) {
                Ok(m) => m,
                Err(e) => {
                    problems.push(format!("{id}: manifest is not valid JSON: {e}"));
                    continue;
                }
            };
        if manifest.id() != *id {
            problems.push(format!("{id}: manifest calls itself `{}`", manifest.id()));
        }
        if manifest.version != entry.version {
            problems.push(format!(
                "{id}: listing says v{} but the manifest says v{}",
                entry.version, manifest.version
            ));
        }
        for p in manifest.validate() {
            problems.push(format!("{id}: {p}"));
        }
        if manifest.capabilities != entry.capabilities {
            problems.push(format!(
                "{id}: the listing's capabilities do not match the manifest's — a listing \
                 may not advertise less than the spore asks for"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Entry {
        Entry {
            id: "acme.adr".into(),
            version: "1.2.0".into(),
            repo: "github:acme/adr-spore".into(),
            path: "spore".into(),
            files: BTreeMap::from([(crate::MANIFEST_FILE.into(), "a".repeat(64))]),
            ..Default::default()
        }
    }

    #[test]
    fn github_and_https_urls() {
        let e = entry();
        assert_eq!(
            e.file_url("spore.json"),
            "https://raw.githubusercontent.com/acme/adr-spore/main/spore/spore.json"
        );

        let mut pinned = entry();
        pinned.repo = "github:acme/adr-spore#v1.2.0".into();
        assert_eq!(
            pinned.file_url("README.md"),
            "https://raw.githubusercontent.com/acme/adr-spore/v1.2.0/spore/README.md"
        );

        let mut https = entry();
        https.repo = "https://acme.example/spores/".into();
        https.path = "/adr/".into();
        assert_eq!(
            https.file_url("spore.json"),
            "https://acme.example/spores/adr/spore.json"
        );

        let mut rooted = entry();
        rooted.path = String::new();
        assert_eq!(
            rooted.file_url("spore.json"),
            "https://raw.githubusercontent.com/acme/adr-spore/main/spore.json"
        );
    }

    #[test]
    fn rejects_unlisted_files_and_bad_hashes() {
        let mut e = entry();
        e.files.insert("evil.sh".into(), "b".repeat(64));
        e.files.insert(crate::README_FILE.into(), "nope".into());
        let errs = e.validate();
        assert!(
            errs.iter()
                .any(|x| x.contains("not an allowed package file")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|x| x.contains("no valid sha256")),
            "{errs:?}"
        );
    }

    #[test]
    fn tier_comes_from_advertised_capabilities() {
        let mut e = entry();
        assert_eq!(e.tier(), Tier::Declarative);
        e.capabilities.push(Capability::Http {
            hosts: vec!["api.github.com".into()],
        });
        assert_eq!(e.tier(), Tier::Http);
        assert_eq!(e.publisher(), "acme");
        assert_eq!(e.name(), "adr");
    }

    #[test]
    fn native_listings_are_refused_but_sandboxed_ones_are_not() {
        // A sandboxed listing may sit in an index before its runner ships;
        // the client says "not in this version". Native never gets that far.
        let mut wasm = entry();
        wasm.capabilities.push(Capability::Tcp {
            endpoints: vec!["redis.internal:6379".into()],
        });
        assert!(wasm.validate().is_empty(), "{:?}", wasm.validate());

        let mut native = entry();
        native.capabilities.push(Capability::FsWrite {
            paths: vec!["docs/".into()],
        });
        let errs = native.validate();
        assert!(errs.iter().any(|x| x.contains("never listed")), "{errs:?}");
    }

    #[test]
    fn revocation_matches_exact_versions_or_all() {
        let all = Revocation {
            id: "a.b".into(),
            versions: vec!["*".into()],
            reason: String::new(),
        };
        assert!(all.covers("9.9.9"));
        let one = Revocation {
            id: "a.b".into(),
            versions: vec!["1.0.0".into()],
            reason: String::new(),
        };
        assert!(one.covers("1.0.0"));
        assert!(!one.covers("1.0.1"));
    }
}
