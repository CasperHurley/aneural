//! Byte-level fetching, and the seam every test swaps out.
//!
//! Splitting transport from [`crate::client`] means the whole marketplace can be
//! exercised against a directory of fixture files: no test in this workspace
//! touches the network except `tests/http.rs`, which serves itself on loopback.

use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;
use ureq::ResponseExt;

/// A response body plus the little bit of cache metadata we care about.
#[derive(Clone, Debug, Default)]
pub struct Fetched {
    pub body: Vec<u8>,
    pub etag: Option<String>,
    /// The server answered 304: `body` is empty and the cached copy is current.
    pub not_modified: bool,
}

pub trait Transport: Send + Sync {
    /// GET `url`, optionally conditional on `etag`, refusing bodies over `max_bytes`.
    fn get(&self, url: &str, etag: Option<&str>, max_bytes: usize) -> Result<Fetched>;
}

/// 20s is long enough for a cold CDN and short enough that a hung index does
/// not look like a frozen marketplace.
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REDIRECTS: u32 = 3;

pub struct HttpTransport {
    agent: ureq::Agent,
}

impl HttpTransport {
    pub fn new() -> Self {
        Self::with_timeout(TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .max_redirects(MAX_REDIRECTS)
            .user_agent(concat!("aneural/", env!("CARGO_PKG_VERSION")))
            // We inspect statuses ourselves so 304 is a normal answer.
            .http_status_as_error(false)
            .build();
        HttpTransport {
            agent: config.into(),
        }
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for HttpTransport {
    fn get(&self, url: &str, etag: Option<&str>, max_bytes: usize) -> Result<Fetched> {
        let mut req = self.agent.get(url);
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }
        let mut res = req.call().map_err(|source| Error::Transport {
            url: url.to_string(),
            source: Box::new(source),
        })?;

        let status = res.status().as_u16();
        if status == 304 {
            return Ok(Fetched {
                not_modified: true,
                ..Default::default()
            });
        }
        if !(200..300).contains(&status) {
            return Err(Error::Status {
                url: url.to_string(),
                status,
            });
        }

        // A redirect must not carry us onto another host: the index pins hosts,
        // and following a redirect off them would defeat that.
        if let Some(final_uri) = res.get_uri().host()
            && let Some(asked) = host_of(url)
            && final_uri != asked
        {
            return Err(Error::Invalid(format!(
                "{url} redirected to a different host ({final_uri}); refusing to follow"
            )));
        }

        let etag = res
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        // `limit` makes an oversized or decompression-bombed body an error
        // rather than an allocation.
        let body = res
            .body_mut()
            .with_config()
            .limit(max_bytes as u64)
            .read_to_vec()
            .map_err(|_| Error::TooLarge {
                url: url.to_string(),
                limit: max_bytes,
            })?;

        Ok(Fetched {
            body,
            etag,
            not_modified: false,
        })
    }
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    Some(host.split(':').next().unwrap_or(host))
}

/// Serves URLs as paths beneath `root`. Backs private file-system registries and
/// every fixture test.
pub struct DirTransport {
    root: PathBuf,
}

impl DirTransport {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        DirTransport { root: root.into() }
    }

    /// Map a URL or path onto a file under `root`, refusing to escape it.
    ///
    /// Absolute paths are honoured — a private registry naturally records them —
    /// but only when they stay inside the root, so a hostile index still cannot
    /// reach into the rest of the filesystem.
    fn resolve(&self, url: &str) -> Result<PathBuf> {
        let raw = url.strip_prefix("file://").unwrap_or(url);
        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.root.join(raw)
        };

        let normalized = normalize(&candidate);
        let root = normalize(&self.root);
        if !normalized.starts_with(&root) {
            return Err(Error::Invalid(format!("`{url}` escapes the registry root")));
        }
        Ok(normalized)
    }
}

/// Resolve `.` and `..` lexically. We cannot canonicalize, because the file may
/// legitimately not exist yet, and a 404 is a better answer than an IO error.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        use std::path::Component::*;
        match comp {
            CurDir => {}
            ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl Transport for DirTransport {
    fn get(&self, url: &str, _etag: Option<&str>, max_bytes: usize) -> Result<Fetched> {
        let path = self.resolve(url)?;
        let body = std::fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::Status {
                url: url.to_string(),
                status: 404,
            },
            _ => Error::Io(e),
        })?;
        if body.len() > max_bytes {
            return Err(Error::TooLarge {
                url: url.to_string(),
                limit: max_bytes,
            });
        }
        Ok(Fetched {
            body,
            etag: None,
            not_modified: false,
        })
    }
}

/// Routes by scheme: absolute URLs over HTTP, everything else off disk.
///
/// A registry index can list some entries in real repos and others beside it in a
/// checkout, and validation has to reach both.
pub struct MixedTransport {
    http: HttpTransport,
    dir: DirTransport,
}

impl MixedTransport {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        MixedTransport {
            http: HttpTransport::new(),
            dir: DirTransport::new(root),
        }
    }
}

impl Transport for MixedTransport {
    fn get(&self, url: &str, etag: Option<&str>, max_bytes: usize) -> Result<Fetched> {
        if url.starts_with("http://") || url.starts_with("https://") {
            self.http.get(url, etag, max_bytes)
        } else {
            self.dir.get(url, etag, max_bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_extracted_for_the_redirect_guard() {
        assert_eq!(
            host_of("https://aneural.dev/registry/index.json"),
            Some("aneural.dev")
        );
        assert_eq!(
            host_of("https://user:pw@acme.example:8443/x"),
            Some("acme.example")
        );
        assert_eq!(host_of("https://aneural.dev"), Some("aneural.dev"));
    }

    #[test]
    fn dir_transport_reads_and_refuses_to_escape() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.json"), b"{}").unwrap();
        let t = DirTransport::new(tmp.path());

        // Relative to the registry root...
        assert_eq!(t.get("index.json", None, 1024).unwrap().body, b"{}");
        // ...and an absolute path inside it, which is what a private registry
        // recording its own location looks like.
        let abs = tmp.path().join("index.json");
        assert_eq!(
            t.get(abs.to_str().unwrap(), None, 1024).unwrap().body,
            b"{}"
        );
        assert_eq!(
            t.get(&format!("file://{}", abs.display()), None, 1024)
                .unwrap()
                .body,
            b"{}"
        );

        // An absolute path outside the root is still refused.
        let outside = t.get("/etc/passwd", None, 1024).unwrap_err();
        assert!(matches!(outside, Error::Invalid(_)), "{outside:?}");

        let escaped = t.get("../../etc/passwd", None, 1024).unwrap_err();
        assert!(matches!(escaped, Error::Invalid(_)), "{escaped:?}");

        let missing = t.get("nope.json", None, 1024).unwrap_err();
        assert!(
            matches!(missing, Error::Status { status: 404, .. }),
            "{missing:?}"
        );

        let too_big = t.get("index.json", None, 1).unwrap_err();
        assert!(matches!(too_big, Error::TooLarge { .. }), "{too_big:?}");
    }
}
