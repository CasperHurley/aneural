//! The one test that actually drives `HttpTransport`. Everything else in the
//! crate swaps in `DirTransport`, so without this nothing proves the real
//! client sends the headers or honours the limits it claims to.
//!
//! It serves itself on loopback: no network, no dependency, no fixture server.

use aneural_registry::transport::{HttpTransport, Transport};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// Accept one request, hand the request head back, and reply with `response`.
fn serve_once(response: &'static str) -> (String, mpsc::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if line == "\r\n" {
                break;
            }
            head.push(line.trim_end().to_string());
        }
        let _ = tx.send(head);
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    (format!("http://{addr}/index.json"), rx)
}

fn ok_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"abc123\"\r\nContent-Type: application/json\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn sends_a_user_agent_and_returns_the_body_and_etag() {
    let body = r#"{"version":1,"spores":[]}"#;
    let response: &'static str = Box::leak(ok_response(body).into_boxed_str());
    let (url, head_rx) = serve_once(response);

    let fetched = HttpTransport::new().get(&url, None, 64 * 1024).unwrap();
    assert_eq!(fetched.body, body.as_bytes());
    assert_eq!(fetched.etag.as_deref(), Some("\"abc123\""));
    assert!(!fetched.not_modified);

    let head = head_rx.recv().unwrap();
    let agent = head
        .iter()
        .find(|h| h.to_ascii_lowercase().starts_with("user-agent:"))
        .expect("a User-Agent must be sent so registries can identify the client");
    assert!(agent.contains("aneural/"), "{agent}");
}

#[test]
fn sends_if_none_match_and_understands_304() {
    let (url, head_rx) = serve_once("HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n");

    let fetched = HttpTransport::new()
        .get(&url, Some("\"abc123\""), 64 * 1024)
        .unwrap();
    assert!(fetched.not_modified, "304 must not be an error");
    assert!(fetched.body.is_empty());

    let head = head_rx.recv().unwrap();
    assert!(
        head.iter()
            .any(|h| h.to_ascii_lowercase().starts_with("if-none-match:")),
        "a cached index must revalidate conditionally: {head:?}"
    );
}

#[test]
fn a_non_2xx_status_is_an_error_with_the_code() {
    let (url, _rx) = serve_once("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    let err = HttpTransport::new().get(&url, None, 1024).unwrap_err();
    assert!(
        matches!(err, aneural_registry::Error::Status { status: 404, .. }),
        "{err:?}"
    );
}

#[test]
fn an_oversized_body_is_refused_rather_than_allocated() {
    let body = "x".repeat(4096);
    let response: &'static str = Box::leak(ok_response(&body).into_boxed_str());
    let (url, _rx) = serve_once(response);

    let err = HttpTransport::new().get(&url, None, 128).unwrap_err();
    assert!(
        matches!(err, aneural_registry::Error::TooLarge { limit: 128, .. }),
        "{err:?}"
    );
}

// ---- UreqFetcher -----------------------------------------------------------
//
// The spore-facing fetcher. Every other test of the tier-1 harvester injects a
// fake, so without these nothing proves the real client sends a spore's headers,
// reads a `Link` header, or refuses to follow a redirect off the host the user
// consented to.

use aneural_core::net::{FetchError, Fetcher, Request};
use aneural_registry::UreqFetcher;

#[test]
fn carries_a_spores_headers_and_reads_the_next_page_link() {
    let body = r#"[{"number":1}]"#;
    let response: &'static str = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\
             Link: <https://api.example/x?page=2>; rel=\"next\"\r\n\
             Content-Type: application/json\r\n\r\n{body}",
            body.len()
        )
        .into_boxed_str(),
    );
    let (url, head_rx) = serve_once(response);

    let req = Request::new(url)
        .header("Authorization", "Bearer s3cret")
        .header("Accept", "application/vnd.github+json");
    let res = UreqFetcher::new().get(&req, 64 * 1024).unwrap();

    assert_eq!(res.status, 200);
    assert_eq!(res.body, body.as_bytes());
    assert_eq!(res.next.as_deref(), Some("https://api.example/x?page=2"));

    // ureq normalises header names to lowercase on the wire.
    let head: Vec<String> = head_rx
        .recv()
        .unwrap()
        .iter()
        .map(|l| l.to_ascii_lowercase())
        .collect();
    assert!(
        head.iter().any(|l| l == "authorization: bearer s3cret"),
        "{head:?}"
    );
    assert!(
        head.iter()
            .any(|l| l == "accept: application/vnd.github+json"),
        "{head:?}"
    );
}

#[test]
fn refuses_to_carry_a_token_through_a_redirect() {
    // A redirect is where a bearer token would leak to whoever the first host
    // points at, so the fetcher does not follow one at all.
    let response: &'static str =
        "HTTP/1.1 302 Found\r\nLocation: https://elsewhere.example/x\r\nContent-Length: 0\r\n\r\n";
    let (url, _head) = serve_once(response);

    let req = Request::new(url).header("Authorization", "Bearer s3cret");
    let err = UreqFetcher::new().get(&req, 64 * 1024).unwrap_err();
    assert!(
        matches!(err, FetchError::Status { status: 302, .. }),
        "{err:?}"
    );
}

#[test]
fn an_oversized_body_is_an_error_not_an_allocation() {
    let body = "x".repeat(4096);
    let response: &'static str = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_boxed_str(),
    );
    let (url, _head) = serve_once(response);

    let err = UreqFetcher::new().get(&Request::new(url), 128).unwrap_err();
    assert!(matches!(err, FetchError::TooLarge { .. }), "{err:?}");
}
