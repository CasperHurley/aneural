# Architecture

```
            ┌──────────────┐   GraphDelta stream   ┌──────────────┐
  files ──▶ │ aneural-engine│ ────────────────────▶ │ aneural-gui  │──▶ .aneural/state/focus.json
            │ walk/watch    │                       │ (Bevy)       │             │
            │               │──▶ aneural-store      └──────────────┘             ▼
            │ lang (tree-   │    (SQLite cache)  ◀── @aneural/core ◀── aneural CLI / @aneural/mcp ──▶ Claude Code, Codex
            │  sitter+oxc)  │                        (napi-rs)
            │ spores        │
            └──────────────┘
```

**One indexer.** All parsing lives in Rust (`aneural-lang`, `aneural-engine`). The GUI links the crates
directly and runs the engine on a background thread; the TypeScript CLI and MCP server call the same
code through the `@aneural/core` napi addon. Nothing is implemented twice.

**The cache is disposable.** `.aneural/cache/index.db` is derived state. Every node and edge carries a
`source` (which producer made it) and an `origin` (which file's processing produced it). Reindexing a
file = delete everything with that origin, insert the new set, emit the difference. Deleting the cache
just costs a full reindex.

**Stable ids.** Node ids are `<prefix>:<workspace-relative path>[#fragment]` and never depend on
content, so GUI layout and focus selections survive edits. Files that are manifests keep the `file:`
id but get kind `Manifest`; a directory containing `.git` keeps its `dir:` id but gets kind `Repo`, and
every node under it carries `repoId`. (The plan sketched separate `manifest:`/`repo:` nodes and
`BELONGS_TO` edges; folding them into the file/dir nodes halves the node count and avoids hub hairballs.)
Manifests are not parsed for dependencies: a `Package` node exists because some file imports it.

**Deltas, not snapshots.** The engine streams `GraphDelta { removedNodeIds, removedEdges, nodes, edges,
phase, initialComplete }`. A full index re-emits everything it knows (including unchanged files, from
the cache) so a fresh consumer builds the whole graph, then prunes what vanished. Live changes come
from a debounced `notify` watcher and go through the same per-file pipeline.

**Per-file pipeline** (`Engine::process_file`): fingerprint check → language analysis
(`extract_imports` + `Resolver`) → spore harvesters → `Store::replace_origin` → delta.

**Focus handoff.** The GUI derives a `Focus` (filters, selection, neighbourhood depth, visible ids,
free-text notes) and writes it atomically, debounced, to `.aneural/state/focus.json`. The MCP server
reads it on every `aneural_focus` call and pushes `resources/updated` for `aneural://focus` when the
file changes. Transport is stdio; an HTTP transport (for claude.ai connectors) is a later addition.

**Spores declare what they need, and a spore still ships no code.** A spore is a manifest: node/edge
type definitions plus declarative harvesters (`regex`, `tree-sitter`, `markdown`, `sqlite`, `http`)
run by built-in Rust runners. Nothing a spore ships is executed.

What separates the tiers is therefore not code, it is *reach*. A manifest declares `capabilities`, and
its tier is derived from them — never declared, so it cannot understate itself:

| Tier | Capabilities | Runner |
|---|---|---|
| `declarative` | none | ships |
| `http` | outbound HTTP to named hosts, named secrets | ships |
| `sandboxed` | sockets, graph reads; never the filesystem | not yet |
| `native` | filesystem writes, subprocesses — the user's own privileges | not yet |

A spore whose tier has no runner still lists and still validates, and refuses to install. See
`docs/marketplace.md`.

**Only a host that opts in can make a web request.** `aneural-core` defines `net::Fetcher`; the sole
implementation lives in `aneural-registry`, which already links TLS. The engine holds an
`Option<Box<dyn Fetcher>>` and the host binary supplies it — the GUI and `aneural spores refresh` do,
the MCP server and one-shot CLI queries do not. So indexing a repository never links a TLS stack, an
agent reading the graph over MCP can never cause an outbound request, and every test of the `http`
harvester injects a fake instead of touching the network.

**Tier-1 harvesters are scheduled, not walked.** `http` is the one harvester with no file behind it.
Its nodes hang off a synthetic origin (`spore://<id>/<harvester>`) that no path can collide with, and
`watch_loop` sleeps until the next one is due rather than polling — a workspace with no HTTP spores
never wakes up for them at all.

**One harvester is given a path, not bytes.** `sqlite` opens the database itself, read-only, so a dev
database past `MAX_PARSE_BYTES` is still indexed for the cost of a few catalogue queries.

**Networking is quarantined.** `aneural-registry` is the only crate that reaches the network, and it
depends on `aneural-core` alone — the engine, and therefore `aneural index` and the MCP server, never
link a TLS stack.
