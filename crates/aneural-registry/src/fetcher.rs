//! The real [`Fetcher`]: the only place in the workspace that makes a web
//! request on a spore's behalf.
//!
//! It lives here rather than in `aneural-engine` on purpose. The engine is
//! linked by the MCP server and by every indexing path, and none of those should
//! pull in a TLS stack; `aneural-registry` already had to, so the socket stays
//! quarantined behind `aneural_core::net::Fetcher`.

use aneural_core::net::{FetchError, Fetcher, Request, Response, link_next};
use std::time::Duration;
use ureq::Agent;

/// Long enough for a cold API, short enough that a wedged endpoint does not look
/// like a wedged graph.
const TIMEOUT: Duration = Duration::from_secs(20);

pub struct UreqFetcher {
    agent: Agent,
}

impl UreqFetcher {
    pub fn new() -> Self {
        Self::with_timeout(TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        let config = Agent::config_builder()
            .timeout_global(Some(timeout))
            // Redirects are refused outright rather than followed carefully.
            //
            // A spore request usually carries a bearer token, and the user
            // consented to one host. Following a redirect means deciding
            // whether to forward that token to wherever the first host points,
            // and the only answer that is obviously right is "don't go".
            .max_redirects(0)
            .user_agent(concat!("aneural/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .build();
        UreqFetcher {
            agent: config.into(),
        }
    }
}

impl Default for UreqFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher for UreqFetcher {
    fn get(&self, req: &Request, max_bytes: usize) -> Result<Response, FetchError> {
        let mut call = self.agent.get(&req.url);
        for (name, value) in &req.headers {
            call = call.header(name, value);
        }
        let mut res = call.call().map_err(|e| FetchError::Transport {
            url: req.url.clone(),
            // `e` can render the URL but never the request headers, so this
            // cannot spill a token into a log line.
            message: e.to_string(),
        })?;

        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(FetchError::Status {
                url: req.url.clone(),
                status,
            });
        }

        let next = res
            .headers()
            .get("link")
            .and_then(|v| v.to_str().ok())
            .and_then(link_next);

        let body = res
            .body_mut()
            .with_config()
            .limit(max_bytes as u64)
            .read_to_vec()
            .map_err(|_| FetchError::TooLarge {
                url: req.url.clone(),
                limit: max_bytes,
            })?;

        Ok(Response { status, body, next })
    }
}
