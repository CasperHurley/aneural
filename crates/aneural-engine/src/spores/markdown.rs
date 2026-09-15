//! Minimal markdown structure: frontmatter, title, level-2 sections, wiki-links.

use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frontmatter {
    /// Scalar values.
    pub scalars: BTreeMap<String, String>,
    /// List values (`key:` followed by `- item` lines, or `key: [a, b]`).
    pub lists: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub heading: String,
    /// 1-based line of the heading.
    pub line: u32,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WikiLink {
    pub target: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub frontmatter: Frontmatter,
    pub title: String,
    /// Body without frontmatter.
    pub body: String,
    /// Line offset of `body` within the file (0-based).
    pub body_offset: u32,
    pub sections: Vec<Section>,
    pub links: Vec<WikiLink>,
}

fn wikilink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[([^\]\|#]+)(?:#[^\]\|]*)?(?:\|[^\]]*)?\]\]").unwrap())
}

pub fn parse(text: &str, fallback_title: &str) -> Document {
    let (frontmatter, body, body_offset) = split_frontmatter(text);
    let mut doc = Document {
        frontmatter,
        body: body.to_string(),
        body_offset,
        ..Default::default()
    };

    // title: frontmatter > first H1 > fallback
    doc.title = doc
        .frontmatter
        .scalars
        .get("title")
        .cloned()
        .or_else(|| {
            body.lines()
                .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        })
        .unwrap_or_else(|| fallback_title.to_string());

    // level-2 sections
    let mut current: Option<Section> = None;
    let mut in_fence = false;
    for (i, line) in body.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && let Some(h) = line.strip_prefix("## ") {
            if let Some(s) = current.take() {
                doc.sections.push(finish(s));
            }
            current = Some(Section {
                heading: h.trim().to_string(),
                line: body_offset + i as u32 + 1,
                body: String::new(),
            });
            continue;
        }
        if let Some(s) = &mut current {
            s.body.push_str(line);
            s.body.push('\n');
        }
    }
    if let Some(s) = current.take() {
        doc.sections.push(finish(s));
    }

    // wiki links
    for (i, line) in body.lines().enumerate() {
        for cap in wikilink_re().captures_iter(line) {
            doc.links.push(WikiLink {
                target: cap[1].trim().to_string(),
                line: body_offset + i as u32 + 1,
            });
        }
    }
    doc
}

fn finish(mut s: Section) -> Section {
    s.body = s.body.trim().to_string();
    s
}

/// Wiki links inside a section's line range.
pub fn links_in(doc: &Document, section: &Section, next_line: Option<u32>) -> Vec<WikiLink> {
    doc.links
        .iter()
        .filter(|l| l.line > section.line && next_line.is_none_or(|n| l.line < n))
        .cloned()
        .collect()
}

fn split_frontmatter(text: &str) -> (Frontmatter, &str, u32) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (Frontmatter::default(), text, 0);
    }
    let mut fm = Frontmatter::default();
    let mut current_list: Option<String> = None;
    let mut consumed = 1u32;
    for line in lines {
        consumed += 1;
        if line.trim() == "---" {
            let offset: usize = text
                .lines()
                .take(consumed as usize)
                .map(|l| l.len() + 1)
                .sum();
            let body = text.get(offset.min(text.len())..).unwrap_or("");
            return (fm, body, consumed);
        }
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if let Some(key) = &current_list {
                fm.lists
                    .entry(key.clone())
                    .or_default()
                    .push(unquote(item.trim()));
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_string();
            let value = v.trim();
            if value.is_empty() {
                current_list = Some(key.clone());
                fm.lists.entry(key).or_default();
            } else if value.starts_with('[') && value.ends_with(']') {
                let items = value[1..value.len() - 1]
                    .split(',')
                    .map(|s| unquote(s.trim()))
                    .filter(|s| !s.is_empty())
                    .collect();
                fm.lists.insert(key, items);
                current_list = None;
            } else {
                fm.scalars.insert(key, unquote(value));
                current_list = None;
            }
        }
    }
    // unterminated frontmatter: treat as body
    (Frontmatter::default(), text, 0)
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if (t.starts_with('"') && t.ends_with('"') || t.starts_with('\'') && t.ends_with('\''))
        && t.len() >= 2
    {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\ntitle: Focus loader\nstatus: in-progress\ntargets:\n  - apps/web/src/index.ts\n  - services/api/api/main.py\ntags: [a, \"b\"]\n---\n\n# Ignored H1\n\nIntro [[Architecture]].\n\n## First idea\nBody with [[README|alias]] and [[Notes#sec]].\n\n## Second\n```\n## not a heading\n```\ntext\n";

    #[test]
    fn parses_frontmatter_sections_links() {
        let d = parse(DOC, "fallback");
        assert_eq!(d.title, "Focus loader");
        assert_eq!(d.frontmatter.scalars["status"], "in-progress");
        assert_eq!(
            d.frontmatter.lists["targets"],
            vec!["apps/web/src/index.ts", "services/api/api/main.py"]
        );
        assert_eq!(d.frontmatter.lists["tags"], vec!["a", "b"]);
        assert_eq!(d.sections.len(), 2);
        assert_eq!(d.sections[0].heading, "First idea");
        assert_eq!(d.sections[0].line, 14);
        assert_eq!(d.sections[1].body, "```\n## not a heading\n```\ntext");
        let targets: Vec<_> = d.links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, vec!["Architecture", "README", "Notes"]);
        let first = links_in(&d, &d.sections[0], Some(d.sections[1].line));
        assert_eq!(first.len(), 2);
    }

    #[test]
    fn no_frontmatter_uses_h1_then_fallback() {
        let d = parse("# Hello\n\ntext", "x.md");
        assert_eq!(d.title, "Hello");
        assert_eq!(d.body_offset, 0);
        assert_eq!(parse("just text", "x").title, "x");
    }
}
