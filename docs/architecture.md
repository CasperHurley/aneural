# Architecture

```
            ┌──────────────┐   GraphDelta stream   ┌──────────────┐
  files ──▶ │ aneural-engine│ ────────────────────▶ │ aneural-gui  │──▶ .aneural/state/focus.json
            │ walk/watch    │                       │ (Bevy)       │             │
            │ manifests     │──▶ aneural-store      └──────────────┘             ▼
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
`BELONGS_TO` edges; folding them into the file/dir nodes halves the node count and avoids hub hairballs.
`BELONGS_TO` remains a reserved edge kind.)

**Deltas, not snapshots.** The engine streams `GraphDelta { removedNodeIds, removedEdges, nodes, edges,
phase, initialComplete }`. A full index re-emits everything it knows (including unchanged files, from
the cache) so a fresh consumer builds the whole graph, then prunes what vanished. Live changes come
from a debounced `notify` watcher and go through the same per-file pipeline.

**Per-file pipeline** (`Engine::process_file`): fingerprint check → manifest parse → language analysis
(`extract_imports` + `Resolver`) → spore harvesters → `Store::replace_origin` → delta.

**Focus handoff.** The GUI derives a `Focus` (filters, selection, neighbourhood depth, visible ids,
free-text notes) and writes it atomically, debounced, to `.aneural/state/focus.json`. The MCP server
reads it on every `aneural_focus` call and pushes `resources/updated` for `aneural://focus` when the
file changes. Transport is stdio; an HTTP transport (for claude.ai connectors) is a later addition.

**Spores never execute code.** A spore is a manifest: node/edge type definitions plus declarative
harvesters (`regex`, `tree-sitter`, `markdown`) run by built-in Rust runners. A `wasm` harvester kind
is reserved for a sandboxed runtime later.
