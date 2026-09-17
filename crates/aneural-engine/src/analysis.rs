//! Language analysis → IMPORTS / REFERENCES edges.

use aneural_core::kinds::{EdgeKind, NodeKind, Source};
use aneural_core::{Edge, Node, NodeId, Workspace};
use aneural_lang::{ImportKind, Resolved, Resolver};
use aneural_store::Unresolved;
use std::path::Path;

#[derive(Default, Debug)]
pub struct Analysis {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub unresolved: Vec<Unresolved>,
}

/// Analyse one source file. `rel` is workspace-relative; `abs` is its absolute path.
pub fn analyze(
    ws: &Workspace,
    resolver: &Resolver,
    rel: &str,
    abs: &Path,
    source: &[u8],
) -> Analysis {
    let mut out = Analysis::default();
    let Ok(fa) = aneural_lang::analyze_file(resolver, abs, source) else {
        return out;
    };
    let src_id = NodeId::file(rel);
    for (import, resolved) in fa.imports {
        let kind = match import.kind {
            ImportKind::Require | ImportKind::Include | ImportKind::Mod => EdgeKind::REFERENCES,
            _ => EdgeKind::IMPORTS,
        };
        let import_kind = serde_json::to_value(import.kind)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let base = |dst: NodeId, kind: &str| {
            Edge::new(kind, src_id.clone(), dst, Source::LANG)
                .with_origin(rel)
                .with_prop("specifier", import.specifier.clone())
                .with_prop("line", import.line as i64)
                .with_prop("importKind", import_kind.clone())
                .with_prop(
                    "symbols",
                    serde_json::Value::Array(
                        import
                            .symbols
                            .iter()
                            .map(|s| serde_json::Value::String(s.clone()))
                            .collect(),
                    ),
                )
        };
        match resolved {
            Resolved::File { path } => match ws.rel(&path) {
                Some(target) if target != rel => out.edges.push(base(NodeId::file(&target), kind)),
                Some(_) => {}
                None => out.unresolved.push(Unresolved {
                    origin: rel.into(),
                    specifier: import.specifier.clone(),
                    line: import.line,
                    reason: "outside workspace".into(),
                }),
            },
            Resolved::Directory { path } => match ws.rel(&path) {
                Some(target) => out.edges.push(base(NodeId::dir(&target), kind)),
                None => out.unresolved.push(Unresolved {
                    origin: rel.into(),
                    specifier: import.specifier.clone(),
                    line: import.line,
                    reason: "outside workspace".into(),
                }),
            },
            Resolved::External { ecosystem, name } => {
                out.nodes.push(
                    Node::new(
                        NodeId::package(&ecosystem, &name),
                        NodeKind::PACKAGE,
                        &name,
                        Source::LANG,
                    )
                    .with_prop("ecosystem", ecosystem.clone()),
                );
                out.edges
                    .push(base(NodeId::package(&ecosystem, &name), kind));
            }
            Resolved::Unresolved { reason } => {
                if reason != "stdlib" {
                    out.unresolved.push(Unresolved {
                        origin: rel.into(),
                        specifier: import.specifier.clone(),
                        line: import.line,
                        reason,
                    });
                }
            }
        }
    }
    // dedupe edges with the same (kind, dst): keep the first, merge lines,
    // and remember every way the file pulled it in (an import and a re-export
    // of the same module are one edge)
    let mut seen = std::collections::HashMap::<(String, NodeId), usize>::new();
    let mut merged: Vec<Edge> = Vec::new();
    for e in out.edges.drain(..) {
        let key = (e.kind.clone(), e.dst.clone());
        if let Some(&i) = seen.get(&key) {
            if let (serde_json::Value::Object(a), serde_json::Value::Object(b)) =
                (&mut merged[i].props, &e.props)
            {
                let first_line = a.get("line").cloned().unwrap_or_default();
                let lines = a
                    .entry("lines")
                    .or_insert_with(|| serde_json::Value::Array(vec![first_line]));
                if let (serde_json::Value::Array(arr), Some(l)) = (lines, b.get("line")) {
                    arr.push(l.clone());
                }
                let first_kind = a.get("importKind").cloned().unwrap_or_default();
                let kinds = a
                    .entry("importKinds")
                    .or_insert_with(|| serde_json::Value::Array(vec![first_kind]));
                if let (serde_json::Value::Array(arr), Some(k)) = (kinds, b.get("importKind"))
                    && !arr.contains(k)
                {
                    arr.push(k.clone());
                }
            }
            continue;
        }
        seen.insert(key, merged.len());
        merged.push(e);
    }
    out.edges = merged;
    let mut seen_n = std::collections::HashSet::new();
    out.nodes.retain(|n| seen_n.insert(n.id.clone()));
    out
}
