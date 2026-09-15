//! Dependency manifests → external `Package` nodes and `DEPENDS_ON` edges.

use aneural_core::kinds::{EdgeKind, NodeKind, Source};
use aneural_core::{Edge, Node, NodeId};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub struct Dependency {
    pub ecosystem: &'static str,
    pub name: String,
    pub range: String,
    pub dev: bool,
}

pub fn parse(file_name: &str, text: &str) -> Vec<Dependency> {
    match file_name {
        "package.json" => package_json(text),
        "Cargo.toml" => cargo_toml(text),
        "pyproject.toml" => pyproject(text),
        "requirements.txt" => requirements(text),
        "go.mod" => go_mod(text),
        "pom.xml" => pom(text),
        "build.gradle" | "build.gradle.kts" => gradle(text),
        "composer.json" => composer(text),
        "Gemfile" => gemfile(text),
        _ => Vec::new(),
    }
}

/// Node for an external package.
pub fn package_node(ecosystem: &str, name: &str) -> Node {
    Node::new(
        NodeId::package(ecosystem, name),
        NodeKind::PACKAGE,
        name,
        Source::MANIFEST,
    )
    .with_prop("ecosystem", ecosystem)
}

/// Nodes/edges for a manifest at `rel` (origin = rel).
pub fn graph_for(rel: &str, deps: &[Dependency]) -> (Vec<Node>, Vec<Edge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for d in deps {
        nodes.push(package_node(d.ecosystem, &d.name));
        edges.push(
            Edge::new(
                EdgeKind::DEPENDS_ON,
                NodeId::file(rel),
                NodeId::package(d.ecosystem, &d.name),
                Source::MANIFEST,
            )
            .with_origin(rel)
            .with_prop("range", d.range.clone())
            .with_prop("dev", d.dev),
        );
    }
    (nodes, edges)
}

fn package_json(text: &str) -> Vec<Dependency> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (key, dev) in [
        ("dependencies", false),
        ("devDependencies", true),
        ("peerDependencies", false),
        ("optionalDependencies", false),
    ] {
        if let Some(map) = v.get(key).and_then(|m| m.as_object()) {
            for (name, range) in map {
                let range = range.as_str().unwrap_or("").to_string();
                if range.starts_with("workspace:")
                    || range.starts_with("file:")
                    || range.starts_with("link:")
                {
                    continue;
                }
                out.push(Dependency {
                    ecosystem: "npm",
                    name: name.clone(),
                    range,
                    dev,
                });
            }
        }
    }
    out
}

fn cargo_toml(text: &str) -> Vec<Dependency> {
    let Ok(v) = text.parse::<toml::Table>() else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut tables: Vec<(&toml::Table, bool)> = Vec::new();
    for (key, dev) in [
        ("dependencies", false),
        ("dev-dependencies", true),
        ("build-dependencies", true),
    ] {
        if let Some(t) = v.get(key).and_then(|t| t.as_table()) {
            tables.push((t, dev));
        }
    }
    if let Some(t) = v
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|t| t.as_table())
    {
        tables.push((t, false));
    }
    if let Some(targets) = v.get("target").and_then(|t| t.as_table()) {
        for (_, cfg) in targets {
            if let Some(t) = cfg.get("dependencies").and_then(|t| t.as_table()) {
                tables.push((t, false));
            }
        }
    }
    for (t, dev) in tables {
        for (name, spec) in t {
            let (range, is_path) = match spec {
                toml::Value::String(s) => (s.clone(), false),
                toml::Value::Table(t) => (
                    t.get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    t.contains_key("path")
                        || t.contains_key("workspace")
                            && t.get("workspace").and_then(|w| w.as_bool()) == Some(true)
                            && false,
                ),
                _ => (String::new(), false),
            };
            if is_path {
                continue;
            }
            let name = spec
                .as_table()
                .and_then(|t| t.get("package"))
                .and_then(|p| p.as_str())
                .unwrap_or(name)
                .to_string();
            out.push(Dependency {
                ecosystem: "cargo",
                name,
                range,
                dev,
            });
        }
    }
    dedupe(out)
}

fn pep508_name(spec: &str) -> Option<String> {
    let re = name_re();
    re.find(spec.trim())
        .map(|m| m.as_str().to_lowercase().replace('_', "-"))
}

fn name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*").unwrap())
}

fn pyproject(text: &str) -> Vec<Dependency> {
    let Ok(v) = text.parse::<toml::Table>() else {
        return vec![];
    };
    let mut out = Vec::new();
    let push = |out: &mut Vec<Dependency>, spec: &str, dev: bool| {
        if let Some(name) = pep508_name(spec) {
            let range = spec.trim()[name.len().min(spec.trim().len())..]
                .trim()
                .to_string();
            out.push(Dependency {
                ecosystem: "pypi",
                name,
                range,
                dev,
            });
        }
    };
    if let Some(project) = v.get("project").and_then(|p| p.as_table()) {
        for spec in project
            .get("dependencies")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(s) = spec.as_str() {
                push(&mut out, s, false);
            }
        }
        if let Some(opt) = project
            .get("optional-dependencies")
            .and_then(|o| o.as_table())
        {
            for (_, arr) in opt {
                for spec in arr.as_array().into_iter().flatten() {
                    if let Some(s) = spec.as_str() {
                        push(&mut out, s, true);
                    }
                }
            }
        }
    }
    if let Some(groups) = v.get("dependency-groups").and_then(|g| g.as_table()) {
        for (_, arr) in groups {
            for spec in arr.as_array().into_iter().flatten() {
                if let Some(s) = spec.as_str() {
                    push(&mut out, s, true);
                }
            }
        }
    }
    if let Some(poetry) = v
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.as_table())
    {
        for (key, dev) in [("dependencies", false), ("dev-dependencies", true)] {
            if let Some(t) = poetry.get(key).and_then(|t| t.as_table()) {
                for (name, spec) in t {
                    if name == "python" {
                        continue;
                    }
                    let range = match spec {
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Table(t) => t
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        _ => String::new(),
                    };
                    out.push(Dependency {
                        ecosystem: "pypi",
                        name: name.to_lowercase().replace('_', "-"),
                        range,
                        dev,
                    });
                }
            }
        }
    }
    dedupe(out)
}

fn requirements(text: &str) -> Vec<Dependency> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('-') {
            continue;
        }
        if let Some(name) = pep508_name(line) {
            out.push(Dependency {
                ecosystem: "pypi",
                name: name.clone(),
                range: line[name.len().min(line.len())..].trim().to_string(),
                dev: false,
            });
        }
    }
    dedupe(out)
}

fn go_mod(text: &str) -> Vec<Dependency> {
    let mut out = Vec::new();
    let mut in_block = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("require (") {
            in_block = true;
            continue;
        }
        if in_block && line.starts_with(')') {
            in_block = false;
            continue;
        }
        let spec = if in_block {
            Some(line)
        } else {
            line.strip_prefix("require ").map(str::trim)
        };
        if let Some(spec) = spec {
            let mut parts = spec.split_whitespace();
            if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
                if name.starts_with("//") {
                    continue;
                }
                let indirect = spec.contains("// indirect");
                out.push(Dependency {
                    ecosystem: "go",
                    name: name.to_string(),
                    range: ver.to_string(),
                    dev: indirect,
                });
            }
        }
    }
    dedupe(out)
}

fn pom(text: &str) -> Vec<Dependency> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?s)<dependency>\s*(?:<groupId>([^<]+)</groupId>\s*<artifactId>([^<]+)</artifactId>|<artifactId>([^<]+)</artifactId>\s*<groupId>([^<]+)</groupId>)(?:\s*<version>([^<]+)</version>)?(.*?)</dependency>").unwrap()
    });
    let mut out = Vec::new();
    for c in re.captures_iter(text) {
        let (g, a) = match (c.get(1), c.get(2), c.get(3), c.get(4)) {
            (Some(g), Some(a), _, _) => (g.as_str(), a.as_str()),
            (_, _, Some(a), Some(g)) => (g.as_str(), a.as_str()),
            _ => continue,
        };
        let dev = c
            .get(6)
            .map(|m| m.as_str().contains("<scope>test</scope>"))
            .unwrap_or(false);
        out.push(Dependency {
            ecosystem: "maven",
            name: format!("{}:{}", g.trim(), a.trim()),
            range: c
                .get(5)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default(),
            dev,
        });
    }
    dedupe(out)
}

fn gradle(text: &str) -> Vec<Dependency> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?m)^\s*(\w+)\s*\(?\s*['"]([\w.-]+):([\w.-]+)(?::([^'"]+))?['"]"#).unwrap()
    });
    let mut out = Vec::new();
    for c in re.captures_iter(text) {
        let conf = &c[1];
        let dev = conf.starts_with("test");
        out.push(Dependency {
            ecosystem: "maven",
            name: format!("{}:{}", &c[2], &c[3]),
            range: c.get(4).map(|m| m.as_str().to_string()).unwrap_or_default(),
            dev,
        });
    }
    dedupe(out)
}

fn composer(text: &str) -> Vec<Dependency> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (key, dev) in [("require", false), ("require-dev", true)] {
        if let Some(map) = v.get(key).and_then(|m| m.as_object()) {
            for (name, range) in map {
                if name == "php"
                    || name.starts_with("ext-")
                    || name.starts_with("lib-")
                    || !name.contains('/')
                {
                    continue;
                }
                out.push(Dependency {
                    ecosystem: "packagist",
                    name: name.to_lowercase(),
                    range: range.as_str().unwrap_or("").to_string(),
                    dev,
                });
            }
        }
    }
    out
}

fn gemfile(text: &str) -> Vec<Dependency> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"^\s*gem\s+['"]([^'"]+)['"](?:\s*,\s*['"]([^'"]+)['"])?"#).unwrap()
    });
    let mut out = Vec::new();
    let mut depth_dev = 0i32;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("group") && (t.contains(":development") || t.contains(":test")) {
            depth_dev += 1;
        } else if t == "end" && depth_dev > 0 {
            depth_dev -= 1;
        }
        if let Some(c) = re.captures(line) {
            out.push(Dependency {
                ecosystem: "rubygems",
                name: c[1].to_string(),
                range: c.get(2).map(|m| m.as_str().to_string()).unwrap_or_default(),
                dev: depth_dev > 0,
            });
        }
    }
    dedupe(out)
}

fn dedupe(v: Vec<Dependency>) -> Vec<Dependency> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter()
        .filter(|d| seen.insert((d.ecosystem, d.name.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_manifest() {
        let pj = parse(
            "package.json",
            r#"{"dependencies":{"react":"^19","local":"workspace:*"},"devDependencies":{"vitest":"5"}}"#,
        );
        assert_eq!(pj.len(), 2);
        assert!(pj.iter().any(|d| d.name == "vitest" && d.dev));

        let ct = parse(
            "Cargo.toml",
            "[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nlocal = { path = \"../x\" }\n[dev-dependencies]\ntempfile = \"3\"\n",
        );
        assert_eq!(
            ct.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            vec!["serde", "tempfile"]
        );

        let py = parse(
            "pyproject.toml",
            "[project]\ndependencies = [\"fastapi>=0.115\", \"Pydantic_Core\"]\n",
        );
        assert_eq!(py[0].name, "fastapi");
        assert_eq!(py[0].range, ">=0.115");
        assert_eq!(py[1].name, "pydantic-core");

        let go = parse(
            "go.mod",
            "module x\n\nrequire github.com/a/b v1.2.3\nrequire (\n\tgolang.org/x/net v0.1.0 // indirect\n)\n",
        );
        assert_eq!(go.len(), 2);
        assert!(go[1].dev);

        let pom = parse(
            "pom.xml",
            "<project><dependencies><dependency><groupId>g</groupId><artifactId>a</artifactId><version>1</version><scope>test</scope></dependency></dependencies></project>",
        );
        assert_eq!(pom[0].name, "g:a");
        assert!(pom[0].dev);

        let gr = parse(
            "build.gradle.kts",
            "dependencies {\n    implementation(\"com.x:y:1.0\")\n    testImplementation(\"junit:junit:4\")\n}\n",
        );
        assert_eq!(gr.len(), 2);

        let cj = parse(
            "composer.json",
            r#"{"require":{"php":">=8","monolog/monolog":"^3"}}"#,
        );
        assert_eq!(cj.len(), 1);

        let gf = parse(
            "Gemfile",
            "gem 'sinatra', '~> 4'\ngroup :test do\n  gem 'rspec'\nend\n",
        );
        assert_eq!(gf.len(), 2);
        assert!(gf[1].dev);

        let (nodes, edges) = graph_for("apps/web/package.json", &pj);
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges[0].src, NodeId::file("apps/web/package.json"));
        assert_eq!(edges[0].origin.as_deref(), Some("apps/web/package.json"));
    }
}
