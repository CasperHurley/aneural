//! End-to-end: index the sample workspace, check the graph, then change a file live.

use aneural_core::kinds::{EdgeKind, NodeKind};
use aneural_core::{GraphDelta, NodeId, Workspace};
use aneural_engine::{Engine, EngineEvent};
use aneural_store::{EdgeQuery, NodeQuery};
use std::path::{Path, PathBuf};

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
    // never inherit an index cache or focus left behind by a local GUI run
    for local in [".aneural/cache", ".aneural/state"] {
        let _ = std::fs::remove_dir_all(root.join(local));
    }
    let root = root.canonicalize().unwrap();
    for repo in ["apps/web", "services/api", "tools/cli"] {
        std::fs::create_dir_all(root.join(repo).join(".git")).unwrap();
    }
    (tmp, root)
}

fn collect(events: &[EngineEvent]) -> GraphDelta {
    let mut all = GraphDelta::default();
    for ev in events {
        if let EngineEvent::Delta(d) = ev {
            all.merge(d.clone());
        }
    }
    all
}

#[test]
fn indexes_sample_workspace_and_reacts_live() {
    let (_tmp, root) = sample();
    let ws = Workspace::at(&root);
    let mut engine = Engine::open(ws.clone()).unwrap();
    let mut events = Vec::new();
    let stats = engine.index_full(false, &mut |e| events.push(e)).unwrap();
    assert!(stats.files_indexed > 20, "{stats:?}");
    assert!(matches!(events.last(), Some(EngineEvent::IndexComplete(_))));
    let all = collect(&events);
    assert!(all.initial_complete);
    let store = engine.store();

    // structure
    let repos = store
        .query_nodes(&NodeQuery {
            kinds: vec![NodeKind::REPO.into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(repos.len(), 3, "{repos:?}");
    let index_ts = store
        .get_node(&NodeId::file("apps/web/src/index.ts"))
        .unwrap()
        .unwrap();
    assert_eq!(index_ts.kind, "File");
    assert_eq!(index_ts.repo_id, Some(NodeId::dir("apps/web")));
    assert_eq!(index_ts.props["lang"], "typescript");
    assert_eq!(
        store
            .get_node(&NodeId::file("apps/web/package.json"))
            .unwrap()
            .unwrap()
            .kind,
        "Manifest"
    );
    assert!(
        store
            .get_node(&NodeId::file("apps/web/node_modules/react/index.js"))
            .unwrap()
            .is_none(),
        "node_modules ignored"
    );
    assert!(
        store
            .get_node(&NodeId::dir(".aneural/cache"))
            .unwrap()
            .is_none()
    );

    // imports
    let out = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("apps/web/src/index.ts")),
            ..Default::default()
        })
        .unwrap();
    let dsts: Vec<&str> = out.iter().map(|e| e.dst.as_str()).collect();
    assert!(
        dsts.contains(&"file:apps/web/src/app.ts"),
        "tsconfig paths alias: {dsts:?} unresolved={:?}",
        store.list_unresolved(None).unwrap()
    );
    assert!(dsts.contains(&"pkg:npm/react"), "{dsts:?}");
    assert!(dsts.contains(&"file:apps/web/src/styles.css"), "{dsts:?}");
    let app = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("apps/web/src/app.ts")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        app.iter()
            .any(|e| e.kind == EdgeKind::RE_EXPORTS
                && e.dst == NodeId::file("apps/web/src/lib/util.ts")),
        "{app:?}"
    );
    assert!(
        app.iter().any(|e| e.kind == EdgeKind::IMPORTS
            && e.dst == NodeId::file("apps/web/src/lib/util.ts")
            && e.props["specifier"] == "./lib/util.js"),
        ".js→.ts: {app:?}"
    );
    let py = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("services/api/api/main.py")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        py.iter()
            .any(|e| e.dst == NodeId::file("services/api/api/routes.py")),
        "{py:?}"
    );
    assert!(
        py.iter()
            .any(|e| e.dst == NodeId::package("pypi", "fastapi")),
        "{py:?}"
    );
    let rs = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("tools/cli/src/main.rs")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        rs.iter()
            .any(|e| e.dst == NodeId::file("tools/cli/src/config.rs")),
        "{rs:?}"
    );
    assert!(
        rs.iter()
            .any(|e| e.dst == NodeId::package("cargo", "serde")),
        "{rs:?}"
    );
    let go = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("lib/go-svc/main.go")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        go.iter()
            .any(|e| e.dst == NodeId::dir("lib/go-svc/internal/handler")),
        "{go:?}"
    );
    let java = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("lib/jvm/src/main/java/com/example/App.java")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        java.iter()
            .any(|e| e.dst == NodeId::file("lib/jvm/src/main/java/com/example/util/Strings.java")),
        "{java:?}"
    );
    let php = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("lib/php-app/src/Kernel.php")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        php.iter()
            .any(|e| e.dst == NodeId::file("lib/php-app/src/Http/Router.php")),
        "{php:?}"
    );
    let rb = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("lib/ruby-app/lib/app.rb")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        rb.iter()
            .any(|e| e.dst == NodeId::file("lib/ruby-app/lib/app/config.rb")),
        "{rb:?}"
    );

    // manifests
    let pj = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("apps/web/package.json")),
            kinds: vec![EdgeKind::DEPENDS_ON.into()],
            ..Default::default()
        })
        .unwrap();
    assert!(pj.iter().any(|e| e.dst == NodeId::package("npm", "react")));
    assert!(
        store
            .get_node(&NodeId::package("cargo", "serde"))
            .unwrap()
            .is_some()
    );

    // spores
    let comments = store
        .query_nodes(&NodeQuery {
            kinds: vec!["Comment".into()],
            ..Default::default()
        })
        .unwrap();
    assert!(comments.len() >= 9, "{}", comments.len());
    assert!(
        comments
            .iter()
            .any(|c| c.label == "hydrate from server state" && c.props["tag"] == "TODO")
    );
    let plans = store
        .query_nodes(&NodeQuery {
            kinds: vec!["Plan".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(plans.len(), 1);
    let plan_edges = store
        .get_edges(&EdgeQuery {
            src: Some(plans[0].id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        plan_edges.iter().any(
            |e| e.kind == EdgeKind::ANNOTATES && e.dst == NodeId::file("apps/web/src/index.ts")
        )
    );
    assert!(plan_edges.iter().any(|e| e.kind == EdgeKind::RELATES_TO
        && e.dst == NodeId::file(".aneural/notes/Architecture.md")));
    let ideas = store
        .query_nodes(&NodeQuery {
            kinds: vec!["Idea".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ideas.len(), 2);
    let notes = store
        .query_nodes(&NodeQuery {
            kinds: vec!["Note".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(notes.len(), 1);
    let readme_links = store
        .get_edges(&EdgeQuery {
            src: Some(NodeId::file("README.md")),
            kinds: vec![EdgeKind::RELATES_TO.into()],
            ..Default::default()
        })
        .unwrap();
    assert!(
        readme_links
            .iter()
            .any(|e| e.dst == NodeId::file(".aneural/notes/Architecture.md")),
        "{readme_links:?}"
    );

    // node types include spore kinds + config overrides
    let types = engine.node_types().unwrap();
    let idea = types.iter().find(|t| t.kind == "Idea").unwrap();
    assert_eq!(idea.color, "#f5c542");
    assert!(idea.provider.starts_with("spore:"));
    assert_eq!(
        engine.spores().len(),
        aneural_engine::spores::BUILTIN_SPORES.len()
    );
    // `database` ships but is not enabled by default: it would walk every `.db`
    // in the workspace, which is worth opting into.
    let database = engine
        .spores()
        .into_iter()
        .find(|s| s.id == "aneural.database")
        .unwrap();
    assert!(!database.enabled);

    // second run: everything skipped, still fully emitted
    let mut events2 = Vec::new();
    let stats2 = engine.index_full(false, &mut |e| events2.push(e)).unwrap();
    assert_eq!(stats2.files_indexed, 0);
    assert_eq!(stats2.files_skipped, stats.files_indexed);
    let all2 = collect(&events2);
    assert!(
        all2.nodes
            .iter()
            .any(|n| n.id == NodeId::file("apps/web/src/index.ts"))
    );
    assert!(
        all2.edges
            .iter()
            .any(|e| e.dst == NodeId::file("apps/web/src/app.ts"))
    );
    assert!(
        all2.nodes
            .iter()
            .any(|n| n.id == NodeId::package("npm", "react"))
    );

    // live: add a TODO, add a new file importing util, delete a file
    let util = root.join("apps/web/src/lib/util.ts");
    std::fs::write(&util, "// FIXME: harden parsing of unicode input\n// TODO: sprout a new node\nexport function format(s: string): string { return s.trim(); }\n").unwrap();
    let newfile = root.join("apps/web/src/extra.ts");
    std::fs::write(
        &newfile,
        "import { format } from './lib/util';\nexport const x = format('a');\n",
    )
    .unwrap();
    std::fs::remove_file(root.join("apps/web/src/components/Button.tsx")).unwrap();
    let mut live = Vec::new();
    engine
        .index_paths(
            &[
                util.clone(),
                newfile.clone(),
                root.join("apps/web/src/components/Button.tsx"),
            ],
            &mut |e| live.push(e),
        )
        .unwrap();
    let d = collect(&live);
    assert!(
        d.nodes
            .iter()
            .any(|n| n.kind == "Comment" && n.label == "sprout a new node"),
        "{:?}",
        d.nodes
    );
    assert!(
        d.nodes
            .iter()
            .any(|n| n.id == NodeId::file("apps/web/src/extra.ts"))
    );
    assert!(d.edges.iter().any(|e| e.kind == EdgeKind::CONTAINS && e.dst == NodeId::file("apps/web/src/extra.ts")));
    assert!(
        d.edges
            .iter()
            .any(|e| e.src == NodeId::file("apps/web/src/extra.ts")
                && e.dst == NodeId::file("apps/web/src/lib/util.ts"))
    );
    assert!(
        d.removed_node_ids
            .contains(&NodeId::file("apps/web/src/components/Button.tsx"))
    );
    assert!(
        engine
            .store()
            .get_node(&NodeId::file("apps/web/src/components/Button.tsx"))
            .unwrap()
            .is_none()
    );
    assert!(
        engine
            .store()
            .file_record("apps/web/src/components/Button.tsx")
            .unwrap()
            .is_none()
    );

    // live: new directory with a file
    std::fs::create_dir_all(root.join("apps/web/src/feature")).unwrap();
    std::fs::write(
        root.join("apps/web/src/feature/a.ts"),
        "export const a = 1; // TODO: in feature\n",
    )
    .unwrap();
    let mut live2 = Vec::new();
    engine
        .index_paths(
            &[
                root.join("apps/web/src/feature"),
                root.join("apps/web/src/feature/a.ts"),
            ],
            &mut |e| live2.push(e),
        )
        .unwrap();
    let d2 = collect(&live2);
    assert!(
        d2.nodes
            .iter()
            .any(|n| n.id == NodeId::dir("apps/web/src/feature"))
    );
    assert!(d2.nodes.iter().any(|n| n.label == "in feature"));
    assert_eq!(
        engine
            .store()
            .get_node(&NodeId::file("apps/web/src/feature/a.ts"))
            .unwrap()
            .unwrap()
            .repo_id,
        Some(NodeId::dir("apps/web"))
    );

    // live: delete a directory
    std::fs::remove_dir_all(root.join("apps/web/src/feature")).unwrap();
    let mut live3 = Vec::new();
    engine
        .index_paths(&[root.join("apps/web/src/feature")], &mut |e| live3.push(e))
        .unwrap();
    let d3 = collect(&live3);
    assert!(
        d3.removed_node_ids
            .contains(&NodeId::dir("apps/web/src/feature"))
    );
    assert!(
        d3.removed_node_ids
            .contains(&NodeId::file("apps/web/src/feature/a.ts"))
    );
    assert!(
        engine
            .store()
            .query_nodes(&NodeQuery {
                text: Some("in feature".into()),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );

    // doctor: no errors on the sample
    let diags = engine.doctor().unwrap();
    assert!(diags.iter().all(|d| d.level != "error"), "{diags:?}");
}

#[test]
fn ignored_paths_and_workspace_discovery() {
    let (_tmp, root) = sample();
    let engine = Engine::open_in_memory(Workspace::at(&root)).unwrap();
    assert!(engine.is_ignored("apps/web/node_modules/react/index.js"));
    assert!(engine.is_ignored(".aneural/cache/index.db"));
    assert!(engine.is_ignored("apps/web/.git/HEAD"));
    assert!(!engine.is_ignored("apps/web/src/index.ts"));
    assert_eq!(
        Workspace::find(root.join("apps/web/src")).unwrap().root(),
        root.as_path()
    );
}
