//! Client-side search over downloaded indexes.
//!
//! Ranking is a pure function so it can be asserted exactly. The tie-breaks make
//! the ordering total, which is the only reason a ranking test is meaningful.

use crate::index::Entry;
use aneural_core::spore::Tier;

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// Which configured registry this came from.
    pub registry: String,
    pub entry: Entry,
    pub score: u32,
    /// Other registries that also list this id, in config order.
    pub also_in: Vec<String>,
}

/// One index and the registry name it came from.
pub struct Source<'a> {
    pub registry: &'a str,
    pub entries: &'a [Entry],
}

/// Rank every entry matching *all* query tokens. An empty query lists
/// everything, which is what opening the marketplace should show.
pub fn search(sources: &[Source<'_>], query: &str) -> Vec<Hit> {
    let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();

    let mut hits: Vec<Hit> = Vec::new();
    for source in sources {
        for entry in source.entries {
            // First registry in config order wins; later ones are noted as
            // alternatives rather than hidden, so a shadow is never invisible.
            if let Some(existing) = hits.iter_mut().find(|h| h.entry.id == entry.id) {
                existing.also_in.push(source.registry.to_string());
                continue;
            }
            let Some(score) = score(entry, &tokens) else {
                continue;
            };
            hits.push(Hit {
                registry: source.registry.to_string(),
                entry: entry.clone(),
                score,
                also_in: Vec::new(),
            });
        }
    }

    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            // Safer things surface first, consistently with the rest of the posture.
            .then(a.entry.tier().cmp(&b.entry.tier()))
            .then(b.entry.first_party.cmp(&a.entry.first_party))
            .then(a.entry.id.cmp(&b.entry.id))
    });
    hits
}

/// `None` when any token matches nothing; tokens are ANDed.
fn score(entry: &Entry, tokens: &[String]) -> Option<u32> {
    if tokens.is_empty() {
        return Some(0);
    }
    let mut total = 0u32;
    for token in tokens {
        let s = token_score(entry, token);
        if s == 0 {
            return None;
        }
        total = total.saturating_add(s);
    }
    Some(total)
}

fn token_score(entry: &Entry, token: &str) -> u32 {
    let id = entry.id.to_lowercase();
    let name = entry.name().to_lowercase();
    let publisher = entry.publisher().to_lowercase();
    let display = entry.display_name.to_lowercase();
    let description = entry.description.to_lowercase();

    let mut score = 0;
    if id == token {
        score += 1000;
    }
    if name == token {
        score += 500;
    }
    if publisher == token {
        score += 200;
    }
    if name.starts_with(token) {
        score += 120;
    }
    if name.contains(token) {
        score += 80;
    }
    if display.split_whitespace().any(|w| w.starts_with(token)) {
        score += 60;
    }
    if display.contains(token) {
        score += 30;
    }
    score += 50
        * entry
            .keywords
            .iter()
            .filter(|k| k.to_lowercase() == token)
            .take(3)
            .count() as u32;
    if entry.categories.iter().any(|c| c.to_lowercase() == token) {
        score += 25;
    }
    if description.contains(token) {
        score += 10;
    }
    if entry.node_kinds.iter().any(|k| k.to_lowercase() == token) {
        score += 40;
    }
    score
}

/// Tier ordering is relied on by the sort above; assert it here so a reorder of
/// the enum cannot silently invert search results.
const _: () = assert!((Tier::Declarative as u8) < (Tier::Native as u8));

#[cfg(test)]
mod tests {
    use super::*;
    use aneural_core::spore::Capability;
    use std::collections::BTreeMap;

    fn entry(id: &str, display: &str, description: &str, keywords: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            version: "1.0.0".into(),
            display_name: display.into(),
            description: description.into(),
            repo: "github:x/y".into(),
            files: BTreeMap::from([(crate::MANIFEST_FILE.into(), "a".repeat(64))]),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn corpus() -> Vec<Entry> {
        vec![
            entry(
                "aneural.comments",
                "Code Comments",
                "TODO and FIXME markers",
                &["todo"],
            ),
            entry(
                "acme.adr",
                "Architecture Decision Records",
                "ADR docs",
                &["adr", "docs"],
            ),
            entry(
                "acme.jira",
                "JIRA Tickets",
                "Tickets near the files they touch",
                &["jira", "tickets"],
            ),
            entry(
                "bob.todo",
                "Bob's TODOs",
                "Another take on todo comments",
                &["todo"],
            ),
        ]
    }

    fn ids(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.entry.id.as_str()).collect()
    }

    #[test]
    fn exact_id_beats_everything_else() {
        let c = corpus();
        let hits = search(
            &[Source {
                registry: "official",
                entries: &c,
            }],
            "acme.adr",
        );
        assert_eq!(ids(&hits), vec!["acme.adr"]);
    }

    #[test]
    fn name_match_outranks_keyword_match() {
        let c = corpus();
        let hits = search(
            &[Source {
                registry: "official",
                entries: &c,
            }],
            "todo",
        );
        // bob.todo matches on name (500+120+80) and keyword; comments only on keyword.
        assert_eq!(ids(&hits), vec!["bob.todo", "aneural.comments"]);
    }

    #[test]
    fn tokens_are_anded() {
        let c = corpus();
        assert!(
            search(
                &[Source {
                    registry: "official",
                    entries: &c
                }],
                "adr nonexistent"
            )
            .is_empty()
        );
        let hits = search(
            &[Source {
                registry: "official",
                entries: &c,
            }],
            "acme tickets",
        );
        assert_eq!(ids(&hits), vec!["acme.jira"]);
    }

    #[test]
    fn empty_query_lists_everything_in_a_total_order() {
        let c = corpus();
        let hits = search(
            &[Source {
                registry: "official",
                entries: &c,
            }],
            "",
        );
        assert_eq!(hits.len(), 4);
        // All scores tie, so the order is tier, then first-party, then id.
        assert_eq!(
            ids(&hits),
            vec!["acme.adr", "acme.jira", "aneural.comments", "bob.todo"]
        );
    }

    #[test]
    fn safer_tiers_surface_first_on_a_tie() {
        let mut c = corpus();
        c[1].capabilities.push(Capability::Subprocess {
            commands: vec!["sh".into()],
        });
        let hits = search(
            &[Source {
                registry: "official",
                entries: &c,
            }],
            "",
        );
        // acme.adr is now Native and drops below every declarative entry.
        assert_eq!(*ids(&hits).last().unwrap(), "acme.adr");
    }

    #[test]
    fn first_registry_wins_and_the_shadow_is_recorded() {
        let official = corpus();
        let team = vec![entry("acme.adr", "Team ADR", "our fork", &[])];
        let hits = search(
            &[
                Source {
                    registry: "team",
                    entries: &team,
                },
                Source {
                    registry: "official",
                    entries: &official,
                },
            ],
            "adr",
        );
        assert_eq!(hits[0].registry, "team");
        assert_eq!(hits[0].entry.display_name, "Team ADR");
        assert_eq!(hits[0].also_in, vec!["official"]);
    }
}
