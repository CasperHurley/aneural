# Spores

A spore is a `spore.json` manifest. It declares node types, edge types, and *harvesters* that infer
nodes from files. Harvested nodes are always derived — Aneural never writes them back.

```json
{
  "$schema": "https://aneural.dev/schema/spore-v1.json",
  "name": "comments", "version": "0.1.0", "displayName": "Code Comments",
  "description": "…", "license": "MIT", "aneural": ">=0.1",
  "nodeTypes": [{ "kind": "Comment", "label": "Comment", "icon": "LuMessageSquare", "color": "#e8c170", "shape": "pill" }],
  "edgeTypes":  [{ "kind": "ANNOTATES", "label": "annotates", "style": "dotted" }],
  "harvesters": [ … ],
  "panel": { "title": "Comments", "columns": ["tag", "file", "line"] }
}
```

## Harvester kinds

- **`regex`** — `include`/`exclude` globs, a Rust `pattern` applied per line with named captures.
- **`tree-sitter`** — `language` + `query` (tree-sitter S-expression); each match's captures become
  template variables.
- **`markdown`** — `granularity: document | heading`; optional `wikilinks` (`[[Name]]` → edges, resolved
  by file stem, preferring the same directory) and `annotate` (a frontmatter list of paths → edges).
  Variables: `{file} {title} {heading} {line} {body} {fm.<key>}`.
- **`wasm`** — reserved.

## `emit`

```json
"emit": {
  "node": { "kind": "Comment", "id": "comment:{file}#{hash(text)}", "label": "{text}", "props": { "tag": "{upper(tag)}", "line": "{line}" } },
  "edges": [ { "kind": "ANNOTATES", "src": "$node", "dst": "file:{file}", "props": { "line": "{line}" } } ]
}
```

Template functions: `hash`, `slug`, `upper`, `lower`, `trim`, `basename`, `stem`. `$node` refers to the
emitted node. Numeric-looking props become numbers.

## Icons

`icon` must be a name from the curated icondata registry (`aneural spores validate` / `aneural doctor`
flag unknown names; the GUI falls back to `LuCircleDot`). List them with `@aneural/core`'s `listIcons()`.

## Marketplace

`config.spores.registry` points at a JSON index:

```json
{ "version": 1, "spores": [ { "name": "comments", "description": "…", "version": "0.1.0",
                              "repo": "github:aneural/spores", "path": "comments", "sha256": "…" } ] }
```

`aneural spores add <name>` fetches `<path>/spore.json`, verifies the hash, validates, installs to
`.aneural/spores/<name>/` and enables it. Workspace spores shadow builtins of the same name.
