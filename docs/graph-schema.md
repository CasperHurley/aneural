# Graph schema

## Node kinds

| Kind | Id | Producer | Notable props |
|---|---|---|---|
| `Directory` | `dir:<path>` (`dir:.` is the root) | walker | |
| `Repo` | `dir:<path>` | walker (dir containing `.git`) | `repo: true` |
| `File` | `file:<path>` | walker | `lang`, `ext`, `size`; `fingerprint` column |
| `Manifest` | `file:<path>` | walker (package.json, Cargo.toml, …) | as File |
| `Package` | `pkg:<ecosystem>/<name>` | imports of external packages | `ecosystem` (npm, cargo, pypi, go, maven, packagist, rubygems) |
| `Comment` | `comment:<file>#<hash(text)>` | spore `comments` | `tag`, `line`, `file` |
| `Plan` | `plan:<file>` | spore `plans` | `file`, `status` |
| `Idea` | `idea:<file>#<slug(heading)>` | spore `icebox` | `file`, `line`, `body` |
| `Note` | `note:<file>` | spore `wiki-links` | `file` |
| `Symbol` | `sym:…` | reserved | |

Kinds are open strings. Custom kinds come from `.aneural/nodes/<kind>.json`
(`{ kind, label, icon, color, shape, description }`) or spore manifests. Every node has `repoId` (nearest
enclosing repo) when inside one.

## Edge kinds

| Kind | src → dst | Props |
|---|---|---|
| `CONTAINS` | Directory/Repo → Directory/File | |
| `IMPORTS` | File → File / Directory (Go packages, Java wildcards) / Package | `specifier`, `line`, `lines[]`, `importKind`, `importKinds[]`, `symbols[]` |
| `REFERENCES` | File → File (require(), `mod`, include) · spore nodes (e.g. foreign keys) | same |
| `ANNOTATES` | Comment/Plan → File (`line` / `via: frontmatter`) | |
| `RELATES_TO` | Note/Idea/Plan/File → File (`via: wikilink|source`, `line`) | |

`importKind` is how the file pulled the target in (`static`, `dynamic`, `type-only`, `re-export`, …).
One file importing and re-exporting the same module is a single edge; `importKinds` then lists both.

In the GUI, `CONTAINS` is the folder tree and is always drawn, and `RELATES_TO` is never drawn as a
strand: a node with no place in the tree that relates to others (a note, idea or plan) floats near
what it relates to, and the thread only appears faintly while one end is selected or hovered. Only
`IMPORTS`, `REFERENCES`, `ANNOTATES` and spore-declared kinds can be switched off.

Edges are unique per `(kind, src, dst, source)`. Standard-library imports produce nothing; other
unresolved imports land in the `unresolved` table and surface via `aneural doctor`.

## SQLite (`.aneural/cache/index.db`, `user_version = 2`)

`nodes(id, kind, label, path, repo_id, props, fingerprint, source, origin, created_at, updated_at)`,
`edges(id, kind, src, dst, props, source, origin)`, `files(path, mtime, size, fingerprint, lang, indexed_at)`,
`unresolved(origin, specifier, line, reason)`, `meta(key, value)`. Foreign keys are not enforced;
dangling edges are filtered at query time and cascaded on delete.

## `.aneural/`

```
config.json          workspace config (see aneural-core::config::Config)
nodes/*.json         custom node types
spores/<name>/       installed spores (spore.json)
notes/ plans/ icebox/   markdown harvested by first-party spores
state/focus.json     GUI → MCP handoff (local; state/ is gitignored)
cache/index.db       derived, gitignored
```

`state/focus.json`:

```json
{ "version": 1, "updatedAt": "…", "workspace": "/abs/root",
  "filters": { "kinds": [], "edgeKinds": [], "repos": [], "query": "" },
  "selection": { "primary": "file:apps/web/src/index.ts", "pinned": [] },
  "neighborhood": { "depth": 1, "direction": "both" },
  "visibleNodeIds": ["…"], "notes": "" }
```
