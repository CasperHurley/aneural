//! The seam between a spore that wants to make a web request and whatever is
//! actually willing to make one.
//!
//! Nothing here opens a socket. `aneural-core` and `aneural-engine` deliberately
//! do not link a TLS stack — indexing a repository should not drag one in — so
//! the engine takes a `&dyn Fetcher` and the host binary supplies the
//! implementation (`aneural_registry::UreqFetcher`). Tests supply a fake, which
//! is why the whole T1 harvester is exercised without touching the network.

use std::collections::BTreeMap;
use std::fmt;

/// One outbound request. Built by the engine, never by a manifest directly:
/// the URL has already been rendered and checked against the host allowlist by
/// the time a `Fetcher` sees it.
#[derive(Clone, Default)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn new(url: impl Into<String>) -> Self {
        Request {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// Header values routinely carry a bearer token, so the derived `Debug` would
/// be a credential leak into any log line that formatted a request.
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("url", &self.url)
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    /// The `rel="next"` URL from an RFC 8288 `Link` header, if the server sent
    /// one. This is how GitHub and most JSON APIs paginate.
    pub next: Option<String>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum FetchError {
    #[error("{url}: {message}")]
    Transport { url: String, message: String },
    #[error("{url} answered {status}")]
    Status { url: String, status: u16 },
    #[error("{url} returned more than {limit} bytes")]
    TooLarge { url: String, limit: usize },
    /// No host wired a fetcher in. A spore that needs the network is installed,
    /// but this build of the surrounding program cannot make requests.
    #[error("this program cannot make web requests")]
    Unavailable,
}

pub trait Fetcher: Send + Sync {
    fn get(&self, req: &Request, max_bytes: usize) -> Result<Response, FetchError>;
}

/// The default when nothing is wired in: every request fails cleanly rather
/// than the engine pretending a harvester ran and found nothing.
pub struct NoFetcher;

impl Fetcher for NoFetcher {
    fn get(&self, _req: &Request, _max_bytes: usize) -> Result<Response, FetchError> {
        Err(FetchError::Unavailable)
    }
}

/// Parse `rel="next"` out of a `Link` header.
pub fn link_next(header: &str) -> Option<String> {
    for part in header.split(',') {
        let (url, params) = part.split_once('>')?;
        let url = url.trim().strip_prefix('<')?;
        if params.contains("rel=\"next\"") || params.contains("rel=next") {
            return Some(url.to_string());
        }
    }
    None
}

// ---- secrets --------------------------------------------------------------

/// Where a `{secret.name}` comes from.
///
/// This build reads the process environment. That is deliberately the least
/// magical option: the value lives wherever the user already keeps credentials
/// (a shell profile, direnv, a CI secret) and Aneural never stores a copy.
/// An OS keychain backend can implement this same trait later without any
/// manifest changing.
pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Option<String>;

    /// Which of `names` are resolvable right now. Used to tell a user *which*
    /// credential is missing instead of just failing the request.
    fn missing(&self, names: &[String]) -> Vec<String> {
        names
            .iter()
            .filter(|n| self.get(n).is_none())
            .cloned()
            .collect()
    }
}

/// `githubToken` is read from `ANEURAL_SECRET_GITHUB_TOKEN`.
pub struct EnvSecrets;

impl EnvSecrets {
    pub fn var_name(secret: &str) -> String {
        let mut out = String::from("ANEURAL_SECRET_");
        let mut prev_lower = false;
        for ch in secret.chars() {
            if ch.is_ascii_uppercase() && prev_lower {
                out.push('_');
            }
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_uppercase());
            } else {
                out.push('_');
            }
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
        out
    }
}

impl SecretStore for EnvSecrets {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(Self::var_name(name))
            .ok()
            .filter(|v| !v.is_empty())
    }
}

/// An in-memory store, for tests and for a host that resolves secrets itself.
#[derive(Clone, Debug, Default)]
pub struct MapSecrets(pub BTreeMap<String, String>);

impl MapSecrets {
    pub fn new<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        MapSecrets(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

impl SecretStore for MapSecrets {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names_become_env_vars() {
        assert_eq!(
            EnvSecrets::var_name("githubToken"),
            "ANEURAL_SECRET_GITHUB_TOKEN"
        );
        assert_eq!(EnvSecrets::var_name("jira"), "ANEURAL_SECRET_JIRA");
        assert_eq!(
            EnvSecrets::var_name("my-api.key"),
            "ANEURAL_SECRET_MY_API_KEY"
        );
    }

    #[test]
    fn link_headers_yield_the_next_page() {
        let h = "<https://api.example/x?page=2>; rel=\"next\", \
                 <https://api.example/x?page=9>; rel=\"last\"";
        assert_eq!(
            link_next(h).as_deref(),
            Some("https://api.example/x?page=2")
        );
        assert_eq!(
            link_next("<https://api.example/x?page=9>; rel=\"last\""),
            None
        );
        assert_eq!(link_next("garbage"), None);
    }

    #[test]
    fn a_request_never_debug_prints_its_header_values() {
        let req = Request::new("https://api.example/x").header("Authorization", "Bearer hunter2");
        let shown = format!("{req:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("Authorization"), "{shown}");
    }
}
