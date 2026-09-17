//! The `http` harvester: a declarative JSON API, turned into nodes.
//!
//! This is the T1 runner. It still executes no third-party code — the manifest
//! declares a URL, some headers and where the records live in the response, and
//! everything below is the engine doing the fetching. What makes it a higher
//! tier than `regex` or `sqlite` is not code, it is *reach*: this harvester can
//! talk to a machine that is not the user's, with the user's own credentials.
//!
//! So the security properties live here, not in the manifest's good intentions:
//!
//! * the rendered host is re-checked against the declared allowlist, because
//!   validation saw a template and this sees the actual URL;
//! * a value that came back from the API is percent-encoded before it is spliced
//!   into the *next* URL, so a crafted response cannot walk the path;
//! * secrets are read at request time, go only into headers, and a missing one
//!   is a reported error rather than a request sent without authentication;
//! * an expansion may only draw edges to nodes that already exist.

use super::template::{Vars, render, render_with};
use super::{Harvest, emit_node};
use aneural_core::NodeId;
use aneural_core::graph::Edge;
use aneural_core::net::{FetchError, Fetcher, Request, SecretStore};
use aneural_core::spore::{
    Emit, EmitEdge, Expand, HttpRequest, MAX_EXPAND_ITEMS, MAX_PAGES_CEILING, SporeManifest, vars,
};
use std::collections::BTreeMap;

/// A single response may not exceed this. Generous for a page of JSON, small
/// enough that a hostile endpoint cannot exhaust memory.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Everything the runner needs from the host to make a request.
pub struct Context<'a> {
    pub fetcher: &'a dyn Fetcher,
    pub secrets: &'a dyn SecretStore,
    /// The user's answers to the manifest's declared `settings`.
    pub settings: &'a BTreeMap<String, String>,
    /// Whether a node id is already in the graph. Expansion edges are dropped
    /// when the far end is not — a pull request touching a file this checkout
    /// does not have should not conjure the file.
    pub known: &'a dyn Fn(&str) -> bool,
}

/// The synthetic origin an HTTP harvester's nodes hang off. It is deliberately
/// not a path: `replace_origin` keys on this string, and no file can collide
/// with it.
pub fn origin(spore_id: &str, harvester_id: &str) -> String {
    format!("spore://{spore_id}/{harvester_id}")
}

#[derive(Debug, Default)]
pub struct Report {
    pub harvest: Harvest,
    /// Problems worth showing the user: a missing credential, a 401, an
    /// unanswered setting. Never a reason to fail the whole index.
    pub problems: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn harvest(
    manifest: &SporeManifest,
    harvester_id: &str,
    request: &HttpRequest,
    select: &str,
    max_pages: u32,
    emit: &Emit,
    expand: Option<&Expand>,
    source: &str,
    cx: &Context<'_>,
) -> Report {
    let mut report = Report::default();
    let org = origin(manifest.id().as_str(), harvester_id);

    let mut base = Vars::new();
    for (k, v) in cx.settings {
        base.insert(format!("setting.{k}"), v.clone());
    }
    base.insert("readAt".into(), now_rfc3339());

    // A setting the user never answered would render as an empty path segment
    // and quietly fetch the wrong thing, so say so instead.
    for def in manifest.settings.iter().filter(|s| s.required) {
        if cx
            .settings
            .get(&def.key)
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            report.problems.push(format!(
                "`{}` is not set; the marketplace panel or `aneural spores set {} {} <value>` can fill it in",
                def.key,
                manifest.id(),
                def.key
            ));
            return report;
        }
    }

    let items = match fetch_records(manifest, request, select, max_pages, &base, cx) {
        Ok(items) => items,
        Err(problem) => {
            report.problems.push(problem);
            return report;
        }
    };

    for item in &items {
        let mut item_vars = base.clone();
        flatten(item, "item", &mut item_vars);

        let Some(node_id) = emit_node(emit, source, &org, &item_vars, &mut report.harvest) else {
            continue;
        };

        let Some(x) = expand else { continue };
        if report.harvest.nodes.len() > MAX_EXPAND_ITEMS {
            continue;
        }

        // `{parent.*}` is the record that produced `node_id`; `{item.*}` below
        // is rebound to each record of the follow-up response.
        let mut parent_vars = base.clone();
        flatten(item, "parent", &mut parent_vars);
        let mut url_vars = parent_vars.clone();
        flatten(item, "item", &mut url_vars);

        let inner = match fetch_records(manifest, &x.request, &x.select, x.max_pages, &url_vars, cx)
        {
            Ok(inner) => inner,
            Err(problem) => {
                if !report.problems.contains(&problem) {
                    report.problems.push(problem);
                }
                continue;
            }
        };

        for record in &inner {
            let mut v = parent_vars.clone();
            flatten(record, "item", &mut v);
            join(
                &x.edges,
                &node_id,
                &v,
                source,
                &org,
                cx,
                &mut report.harvest,
            );
        }
    }

    report
}

/// Draw the expansion's edges, dropping any whose far end is not in the graph.
fn join(
    edges: &[EmitEdge],
    parent: &NodeId,
    v: &Vars,
    source: &str,
    org: &str,
    cx: &Context<'_>,
    out: &mut Harvest,
) {
    for e in edges {
        let resolve = |slot: &str| -> Option<NodeId> {
            if slot == "$parent" || slot == "$node" {
                return Some(parent.clone());
            }
            NodeId::parse(&render(slot, v)).ok()
        };
        let (Some(src), Some(dst)) = (resolve(&e.src), resolve(&e.dst)) else {
            continue;
        };
        if src == dst {
            continue;
        }
        // The endpoint the manifest did not control is the one to check.
        let far = if src == *parent { &dst } else { &src };
        if !(cx.known)(far.as_str()) {
            continue;
        }
        let mut edge = Edge::new(e.kind.clone(), src, dst, source).with_origin(org);
        if let serde_json::Value::Object(map) = &mut edge.props {
            for (k, tpl) in &e.props {
                let rendered = render(tpl, v);
                if !rendered.is_empty() {
                    map.insert(k.clone(), super::template::coerce(&rendered));
                }
            }
        }
        out.edges.push(edge);
    }
}

/// Fetch one request, following `Link: rel="next"` up to `max_pages`, and
/// return the concatenated records.
fn fetch_records(
    manifest: &SporeManifest,
    req: &HttpRequest,
    select: &str,
    max_pages: u32,
    vars_in: &Vars,
    cx: &Context<'_>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut url = render_url(&req.url, vars_in);
    let mut out = Vec::new();

    for _ in 0..max_pages.min(MAX_PAGES_CEILING) {
        // Validation checked the *template's* host. This checks the string that
        // is actually about to be dialled, which is the one that matters — and
        // it runs again on every `next`, because the server chooses that one.
        let host = host_of(&url)
            .ok_or_else(|| format!("`{url}` is not an https url with a host"))?
            .to_string();
        if !manifest.host_allowed(&host) {
            return Err(format!(
                "refused to call `{host}`: not in this spore's allowed hosts"
            ));
        }

        let mut request = Request::new(url.clone());
        // Headers are resolved per hop for the same reason: a secret is scoped
        // to hosts, and the host is only known once the URL is.
        request.headers = resolve_headers(manifest, req, &host, vars_in, cx)?;
        let res = cx
            .fetcher
            .get(&request, MAX_RESPONSE_BYTES)
            .map_err(|e| describe(&e, &url))?;

        let body: serde_json::Value = serde_json::from_slice(&res.body)
            .map_err(|e| format!("`{url}` did not answer with JSON: {e}"))?;
        match pointed(&body, select) {
            Some(serde_json::Value::Array(items)) => out.extend(items.iter().cloned()),
            Some(one @ serde_json::Value::Object(_)) => out.push(one.clone()),
            _ => {
                return Err(format!(
                    "`{url}`: nothing at `{}` in the response",
                    if select.is_empty() { "/" } else { select }
                ));
            }
        }

        match res.next {
            Some(next) => url = next,
            None => break,
        }
    }
    Ok(out)
}

/// Render headers, resolving `{secret.*}` from the store. A secret the manifest
/// did not declare is refused even if it happens to be set, and so is one that
/// was declared for some other host than the one about to be dialled.
fn resolve_headers(
    manifest: &SporeManifest,
    req: &HttpRequest,
    host: &str,
    vars_in: &Vars,
    cx: &Context<'_>,
) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (name, template) in &req.headers {
        let mut with_secrets = vars_in.clone();
        for secret in vars(template, "secret.") {
            if !manifest.secret_declared(&secret) {
                return Err(format!(
                    "header `{name}` wants undeclared secret `{secret}`"
                ));
            }
            if !manifest.secret_allowed(&secret, host) {
                return Err(format!(
                    "refused to send secret `{secret}` to `{host}`: not among that secret's hosts"
                ));
            }
            let Some(value) = cx.secrets.get(&secret) else {
                return Err(format!(
                    "secret `{secret}` is not set; export {}",
                    aneural_core::net::EnvSecrets::var_name(&secret)
                ));
            };
            with_secrets.insert(format!("secret.{secret}"), value);
        }
        out.push((name.clone(), render(template, &with_secrets)));
    }
    Ok(out)
}

fn describe(e: &FetchError, url: &str) -> String {
    match e {
        FetchError::Status { status: 401, .. } | FetchError::Status { status: 403, .. } => {
            format!("`{url}` rejected the credentials ({e})")
        }
        FetchError::Status { status: 404, .. } => {
            format!("`{url}` was not found — check this spore's settings")
        }
        FetchError::Unavailable => {
            "this program cannot make web requests; use the GUI or the CLI".into()
        }
        other => other.to_string(),
    }
}

/// Substitute into a URL, escaping by provenance.
///
/// A `{setting.*}` is something the user typed into their own config, so a `/`
/// in it is meant as a path separator (`acme/widget`). Everything else — above
/// all `{item.*}`, which is whatever a remote server just sent — is escaped in
/// full, so a title of `../../admin` stays a single segment.
fn render_url(template: &str, vars_in: &Vars) -> String {
    render_with(template, vars_in, |name, value| {
        if name.starts_with("setting.") {
            encode(value, b"/")
        } else {
            encode(value, b"")
        }
    })
}

fn encode(value: &str, extra_safe: &[u8]) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~')
            || extra_safe.contains(&b)
        {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    (!host.is_empty()).then_some(host)
}

/// RFC 6901 JSON pointer, with the empty string meaning the document itself.
fn pointed<'a>(body: &'a serde_json::Value, pointer: &str) -> Option<&'a serde_json::Value> {
    if pointer.is_empty() {
        return Some(body);
    }
    body.pointer(pointer)
}

/// Flatten a record into template vars: `{item.title}`, `{item.user.login}`,
/// `{item.labels.0.name}`. An array of scalars also gets a joined form at its
/// own path, so `{item.labels}` is usable without indexing.
fn flatten(value: &serde_json::Value, prefix: &str, out: &mut Vars) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                flatten(v, &format!("{prefix}.{k}"), out);
            }
        }
        serde_json::Value::Array(items) => {
            let mut scalars = Vec::new();
            // An array of objects gets a joined column per field, so a list of
            // GitHub labels or JIRA components is usable as `{item.labels.name}`
            // rather than only as `{item.labels.0.name}`.
            let mut columns: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (i, v) in items.iter().enumerate() {
                flatten(v, &format!("{prefix}.{i}"), out);
                match v {
                    serde_json::Value::Object(map) => {
                        for (k, field) in map {
                            if let Some(s) = scalar(field) {
                                columns.entry(k.clone()).or_default().push(s);
                            }
                        }
                    }
                    other => {
                        if let Some(s) = scalar(other) {
                            scalars.push(s);
                        }
                    }
                }
            }
            for (k, values) in columns {
                out.insert(format!("{prefix}.{k}"), values.join(", "));
            }
            out.insert(prefix.to_string(), scalars.join(", "));
            out.insert(format!("{prefix}.count"), items.len().to_string());
        }
        other => {
            if let Some(s) = scalar(other) {
                out.insert(prefix.to_string(), s);
            }
        }
    }
}

fn scalar(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aneural_core::net::{MapSecrets, Response};
    use std::cell::RefCell;

    /// Answers from a canned table and records what it was asked for, so a test
    /// can assert on the URL and headers that were actually built.
    #[derive(Default)]
    struct FakeApi {
        routes: BTreeMap<String, serde_json::Value>,
        seen: RefCell<Vec<Request>>,
        fail: Option<FetchError>,
    }

    impl FakeApi {
        fn route(mut self, url: &str, body: serde_json::Value) -> Self {
            self.routes.insert(url.into(), body);
            self
        }
        fn urls(&self) -> Vec<String> {
            self.seen.borrow().iter().map(|r| r.url.clone()).collect()
        }
        fn header(&self, name: &str) -> Option<String> {
            self.seen.borrow().first().and_then(|r| {
                r.headers
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
            })
        }
    }

    // The fake is only ever used from the test thread.
    unsafe impl Sync for FakeApi {}

    impl Fetcher for FakeApi {
        fn get(&self, req: &Request, _max: usize) -> Result<Response, FetchError> {
            self.seen.borrow_mut().push(req.clone());
            if let Some(e) = &self.fail {
                return Err(e.clone());
            }
            match self.routes.get(&req.url) {
                Some(body) => Ok(Response {
                    status: 200,
                    body: serde_json::to_vec(body).unwrap(),
                    next: None,
                }),
                None => Err(FetchError::Status {
                    url: req.url.clone(),
                    status: 404,
                }),
            }
        }
    }

    fn manifest(extra: serde_json::Value) -> SporeManifest {
        let mut base = serde_json::json!({
            "publisher": "acme",
            "name": "gh",
            "version": "0.1.0",
            "capabilities": [
                { "kind": "http", "hosts": ["api.example.com", "cdn.example.com"] },
                { "kind": "secret", "names": ["token"], "hosts": ["api.example.com"] }
            ],
            "settings": [{ "key": "repo", "required": true }],
            "harvesters": []
        });
        let serde_json::Value::Object(map) = &mut base else {
            unreachable!()
        };
        let serde_json::Value::Object(more) = extra else {
            unreachable!()
        };
        map.extend(more);
        serde_json::from_value(base).unwrap()
    }

    fn pull_harvester(expand: Option<serde_json::Value>) -> serde_json::Value {
        let mut h = serde_json::json!({
            "kind": "http",
            "id": "pulls",
            "request": {
                "url": "https://api.example.com/repos/{setting.repo}/pulls",
                "headers": { "Authorization": "Bearer {secret.token}" }
            },
            "emit": {
                "node": {
                    "kind": "Pull",
                    "id": "acme.gh.pull:{item.number}",
                    "label": "{item.title}",
                    "props": { "author": "{item.user.login}", "readAt": "{readAt}" }
                },
                "edges": []
            }
        });
        if let Some(x) = expand {
            h.as_object_mut().unwrap().insert("expand".into(), x);
        }
        h
    }

    struct Fixture {
        api: FakeApi,
        settings: BTreeMap<String, String>,
        secrets: MapSecrets,
        known: Vec<String>,
    }

    impl Fixture {
        fn with(api: FakeApi) -> Self {
            Fixture {
                api,
                ..Default::default()
            }
        }
    }

    impl Default for Fixture {
        fn default() -> Self {
            Fixture {
                api: FakeApi::default(),
                settings: [("repo".to_string(), "acme/widget".to_string())]
                    .into_iter()
                    .collect(),
                secrets: MapSecrets::new([("token", "s3cret")]),
                known: vec!["file:src/a.ts".into()],
            }
        }
    }

    fn run(f: &Fixture, m: &SporeManifest) -> Report {
        let Harvester::Http {
            id,
            request,
            select,
            max_pages,
            emit,
            expand,
            ..
        } = &m.harvesters[0]
        else {
            panic!("http harvester")
        };
        let known = |id: &str| f.known.iter().any(|k| k == id);
        let cx = Context {
            fetcher: &f.api,
            secrets: &f.secrets,
            settings: &f.settings,
            known: &known,
        };
        harvest(
            m,
            id,
            request,
            select,
            *max_pages,
            emit,
            expand.as_ref(),
            "spore:acme.gh",
            &cx,
        )
    }

    use aneural_core::spore::Harvester;

    #[test]
    fn records_become_nodes_and_the_token_travels_in_a_header() {
        let f = Fixture::with(FakeApi::default().route(
            "https://api.example.com/repos/acme/widget/pulls",
            serde_json::json!([
                { "number": 7, "title": "Add a thing", "user": { "login": "ada" } },
                { "number": 9, "title": "Fix a thing", "user": { "login": "grace" } },
            ]),
        ));
        let m = manifest(serde_json::json!({ "harvesters": [pull_harvester(None)] }));
        let report = run(&f, &m);

        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(report.harvest.nodes.len(), 2);
        assert_eq!(report.harvest.nodes[0].id.as_str(), "acme.gh.pull:7");
        assert_eq!(report.harvest.nodes[0].label, "Add a thing");
        assert_eq!(report.harvest.nodes[0].props["author"], "ada");
        // The synthetic origin is not a path, so no file can collide with it.
        assert_eq!(
            report.harvest.nodes[0].origin.as_deref(),
            Some("spore://acme.gh/pulls")
        );
        assert_eq!(
            f.api.header("Authorization").as_deref(),
            Some("Bearer s3cret")
        );
    }

    #[test]
    fn an_expansion_joins_a_record_to_files_that_exist_and_no_others() {
        let f = Fixture::with(FakeApi::default()
            .route(
                "https://api.example.com/repos/acme/widget/pulls",
                serde_json::json!([{ "number": 7, "title": "Add", "user": { "login": "ada" } }]),
            )
            .route(
                "https://api.example.com/repos/acme/widget/pulls/7/files",
                serde_json::json!([
                    { "filename": "src/a.ts", "additions": 12 },
                    // Not in this checkout: the edge must be dropped rather
                    // than the file conjured into the graph.
                    { "filename": "vendor/elsewhere.ts", "additions": 3 },
                ]),
            ));
        let m = manifest(serde_json::json!({
            "harvesters": [pull_harvester(Some(serde_json::json!({
                "request": { "url": "https://api.example.com/repos/{setting.repo}/pulls/{item.number}/files" },
                "edges": [{
                    "kind": "TOUCHES", "src": "$parent", "dst": "file:{item.filename}",
                    "props": { "additions": "{item.additions}" }
                }]
            })))]
        }));
        let report = run(&f, &m);

        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(report.harvest.edges.len(), 1, "{:?}", report.harvest.edges);
        let e = &report.harvest.edges[0];
        assert_eq!(e.kind, "TOUCHES");
        assert_eq!(e.src.as_str(), "acme.gh.pull:7");
        assert_eq!(e.dst.as_str(), "file:src/a.ts");
        assert_eq!(e.props["additions"], 12);
    }

    #[test]
    fn a_value_from_the_api_cannot_walk_the_url() {
        // A pull request whose number is a path traversal. If it were spliced
        // in raw the second call would leave the repo entirely.
        let f = Fixture::with(
            FakeApi::default()
                .route(
                    "https://api.example.com/repos/acme/widget/pulls",
                    serde_json::json!([{ "number": "../../../admin", "title": "x" }]),
                )
                .route(
                    "https://api.example.com/repos/acme/widget/pulls/..%2F..%2F..%2Fadmin/files",
                    serde_json::json!([]),
                ),
        );
        let m = manifest(serde_json::json!({
            "harvesters": [pull_harvester(Some(serde_json::json!({
                "request": { "url": "https://api.example.com/repos/{setting.repo}/pulls/{item.number}/files" },
                "edges": [{ "kind": "TOUCHES", "src": "$parent", "dst": "file:{item.filename}" }]
            })))]
        }));
        run(&f, &m);

        let urls = f.api.urls();
        assert!(
            urls.iter().all(|u| !u.contains("../")),
            "a response walked the path: {urls:?}"
        );
        assert!(
            urls.iter()
                .any(|u| u.ends_with("/pulls/..%2F..%2F..%2Fadmin/files")),
            "{urls:?}"
        );
    }

    #[test]
    fn a_host_outside_the_allowlist_is_refused_at_request_time() {
        let f = Fixture::default();
        let mut m = manifest(serde_json::json!({ "harvesters": [pull_harvester(None)] }));
        // Rewrite the URL past validation, the way a tampered install would.
        let Harvester::Http { request, .. } = &mut m.harvesters[0] else {
            panic!()
        };
        request.url = "https://evil.example/repos/{setting.repo}/pulls".into();

        let report = run(&f, &m);
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("not in this spore's allowed hosts")),
            "{:?}",
            report.problems
        );
        assert!(report.harvest.nodes.is_empty());
        assert!(f.api.urls().is_empty(), "it must not even be dialled");
    }

    #[test]
    fn a_secret_stays_on_the_hosts_it_was_scoped_to() {
        // cdn.example.com is an allowed host, so the request itself is fine;
        // the token is what may not go there.
        let f = Fixture::default();
        let mut m = manifest(serde_json::json!({ "harvesters": [pull_harvester(None)] }));
        let Harvester::Http { request, .. } = &mut m.harvesters[0] else {
            panic!()
        };
        request.url = "https://cdn.example.com/repos/{setting.repo}/pulls".into();

        let report = run(&f, &m);
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("refused to send secret `token` to `cdn.example.com`")),
            "{:?}",
            report.problems
        );
        assert!(f.api.urls().is_empty(), "it must not even be dialled");
    }

    #[test]
    fn a_missing_secret_is_reported_instead_of_being_sent_unauthenticated() {
        let f = Fixture {
            secrets: MapSecrets::default(),
            api: FakeApi::default().route(
                "https://api.example.com/repos/acme/widget/pulls",
                serde_json::json!([]),
            ),
            ..Default::default()
        };
        let m = manifest(serde_json::json!({ "harvesters": [pull_harvester(None)] }));

        let report = run(&f, &m);
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("ANEURAL_SECRET_TOKEN")),
            "{:?}",
            report.problems
        );
        assert!(f.api.urls().is_empty(), "no request without the credential");
    }

    #[test]
    fn a_required_setting_is_asked_for_before_anything_is_fetched() {
        let f = Fixture {
            settings: BTreeMap::new(),
            ..Default::default()
        };
        let m = manifest(serde_json::json!({ "harvesters": [pull_harvester(None)] }));

        let report = run(&f, &m);
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("`repo` is not set")),
            "{:?}",
            report.problems
        );
        assert!(f.api.urls().is_empty());
    }

    #[test]
    fn records_are_selected_by_json_pointer() {
        let f = Fixture::with(FakeApi::default().route(
            "https://api.example.com/repos/acme/widget/pulls",
            serde_json::json!({ "data": { "issues": [{ "number": 1, "title": "One" }] } }),
        ));
        let mut m = manifest(serde_json::json!({ "harvesters": [pull_harvester(None)] }));
        let Harvester::Http { select, .. } = &mut m.harvesters[0] else {
            panic!()
        };
        *select = "/data/issues".into();

        let report = run(&f, &m);
        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(report.harvest.nodes.len(), 1);
        assert_eq!(report.harvest.nodes[0].label, "One");
    }

    #[test]
    fn nested_and_repeated_fields_are_addressable() {
        let mut vars = Vars::new();
        flatten(
            &serde_json::json!({
                "number": 7,
                "user": { "login": "ada" },
                "labels": [{ "name": "bug" }, { "name": "ui" }],
                "draft": false
            }),
            "item",
            &mut vars,
        );
        assert_eq!(vars["item.number"], "7");
        assert_eq!(vars["item.user.login"], "ada");
        assert_eq!(vars["item.labels.0.name"], "bug");
        // ...and the whole column, which is what a manifest actually wants.
        assert_eq!(vars["item.labels.name"], "bug, ui");
        assert_eq!(vars["item.labels.count"], "2");
        assert_eq!(vars["item.draft"], "false");
    }
}
