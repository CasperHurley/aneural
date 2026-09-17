//! End-to-end for the tier-1 (HTTP) runner: index a real workspace, enable the
//! shipped `aneural.github` spore against a fake API, and check that pull
//! requests land in the graph attached to the files they change.
//!
//! No network. The fetcher is injected, which is the whole reason
//! `aneural_core::net::Fetcher` exists.

use aneural_core::net::{FetchError, Fetcher, MapSecrets, Request, Response};
use aneural_core::{NodeId, Workspace};
use aneural_engine::Engine;
use aneural_store::{EdgeQuery, NodeQuery};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap().flatten() {
        let p = entry.path();
        let target = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir(&p, &target);
        } else {
            std::fs::copy(&p, &target).unwrap();
        }
    }
}

fn sample() -> (tempfile::TempDir, PathBuf) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-workspace");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("sample-workspace");
    copy_dir(&src, &root);
    for local in [".aneural/cache", ".aneural/state"] {
        let _ = std::fs::remove_dir_all(root.join(local));
    }
    let root = root.canonicalize().unwrap();
    (tmp, root)
}

#[derive(Default)]
struct FakeGitHub {
    routes: BTreeMap<String, serde_json::Value>,
    seen: Mutex<Vec<String>>,
}

impl Fetcher for FakeGitHub {
    fn get(&self, req: &Request, _max: usize) -> Result<Response, FetchError> {
        self.seen.lock().unwrap().push(req.url.clone());
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

/// Pick a real indexed file so the expansion has something to attach to.
fn a_typescript_file(engine: &Engine) -> String {
    engine
        .store()
        .query_nodes(&NodeQuery {
            kinds: vec!["File".into()],
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .filter_map(|n| n.path)
        .find(|p| p.ends_with(".ts") || p.ends_with(".tsx"))
        .expect("the sample workspace has a TypeScript file")
}

fn configure(root: &Path, enabled_github: bool) {
    let path = root.join(".aneural/config.json");
    let mut cfg: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let spores = cfg
        .as_object_mut()
        .unwrap()
        .entry("spores")
        .or_insert_with(|| serde_json::json!({}));
    let spores = spores.as_object_mut().unwrap();
    if enabled_github {
        spores.insert(
            "enabled".into(),
            serde_json::json!(["aneural.comments", "aneural.github"]),
        );
    }
    spores.insert(
        "settings".into(),
        serde_json::json!({ "aneural.github": { "repo": "acme/widget" } }),
    );
    std::fs::write(&path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();
}

#[test]
fn pull_requests_land_in_the_graph_attached_to_the_files_they_change() {
    let (_tmp, root) = sample();
    configure(&root, true);

    let ws = Workspace::at(&root);
    let mut engine = Engine::open_in_memory(ws).unwrap();
    engine.index_full(true, &mut |_| {}).unwrap();

    let touched = a_typescript_file(&engine);
    let pulls = "https://api.github.com/repos/acme/widget/pulls\
                 ?state=open&per_page=100&sort=updated&direction=desc";
    let files = "https://api.github.com/repos/acme/widget/pulls/42/files?per_page=100";

    let api = FakeGitHub {
        routes: BTreeMap::from([
            (
                pulls.to_string(),
                serde_json::json!([{
                    "number": 42,
                    "title": "Tidy the resolver",
                    "draft": false,
                    "html_url": "https://github.com/acme/widget/pull/42",
                    "updated_at": "2026-09-16T10:00:00Z",
                    "user": { "login": "ada" },
                    "head": { "ref": "tidy-resolver" },
                    "base": { "ref": "main" },
                    "labels": [{ "name": "cleanup" }]
                }]),
            ),
            (
                files.to_string(),
                serde_json::json!([
                    { "filename": touched, "status": "modified", "additions": 9, "deletions": 4 },
                    // A path this checkout does not have.
                    { "filename": "not/in/this/repo.ts", "status": "added",
                      "additions": 1, "deletions": 0 },
                ]),
            ),
        ]),
        seen: Mutex::new(Vec::new()),
    };

    engine.set_fetcher(Box::new(api));
    engine.set_secrets(Box::new(MapSecrets::new([("githubToken", "ghp_fake")])));

    let mut events = Vec::new();
    let stats = engine
        .refresh_http(None, true, &mut |e| events.push(e))
        .unwrap();

    assert_eq!(stats.problems, Vec::<String>::new());
    assert_eq!(stats.harvesters, 1);
    assert_eq!(stats.nodes, 1);

    let pr = engine
        .store()
        .get_node(&NodeId::new("pull:42"))
        .unwrap()
        .expect("the pull request node");
    assert_eq!(pr.label, "#42 Tidy the resolver");
    assert_eq!(pr.kind, "PullRequest");
    assert_eq!(pr.props["author"], "ada");
    assert_eq!(pr.props["branch"], "tidy-resolver");
    assert_eq!(pr.props["labels"], "cleanup");
    assert_eq!(pr.source, "spore:aneural.github");
    // Dated, so the Inspector can show how old the reading is.
    assert!(
        pr.props["readAt"].as_str().unwrap().contains('T'),
        "{:?}",
        pr.props["readAt"]
    );

    let edges = engine
        .store()
        .get_edges(&EdgeQuery {
            src: Some(NodeId::new("pull:42")),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(edges.len(), 1, "only the file we actually have: {edges:?}");
    assert_eq!(edges[0].kind, "TOUCHES");
    assert_eq!(edges[0].dst.as_str(), format!("file:{touched}"));
    assert_eq!(edges[0].props["additions"], 9);
}

#[test]
fn without_a_fetcher_the_spore_says_so_instead_of_reporting_an_empty_repo() {
    let (_tmp, root) = sample();
    configure(&root, true);

    let ws = Workspace::at(&root);
    let mut engine = Engine::open_in_memory(ws).unwrap();
    engine.index_full(true, &mut |_| {}).unwrap();
    engine.set_secrets(Box::new(MapSecrets::new([("githubToken", "ghp_fake")])));

    // No `set_fetcher`: this is what the MCP server and a one-shot CLI query
    // look like, and neither should silently pretend there are no pull requests.
    let stats = engine.refresh_http(None, true, &mut |_| {}).unwrap();
    assert!(
        stats
            .problems
            .iter()
            .any(|p| p.contains("cannot make web requests")),
        "{:?}",
        stats.problems
    );
    assert_eq!(stats.nodes, 0);
    assert!(
        engine
            .store()
            .get_node(&NodeId::new("pull:42"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_disabled_http_spore_is_never_fetched() {
    let (_tmp, root) = sample();
    configure(&root, false);

    let ws = Workspace::at(&root);
    let mut engine = Engine::open_in_memory(ws).unwrap();
    assert!(
        !engine.has_http_spores(),
        "github ships disabled; nothing should reach for the network on open"
    );

    let api = FakeGitHub::default();
    engine.set_fetcher(Box::new(api));
    let stats = engine.refresh_http(None, true, &mut |_| {}).unwrap();
    assert_eq!(stats.harvesters, 0);
    assert_eq!(stats.problems, Vec::<String>::new());
}
