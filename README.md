# Aneural

**A living map of everything you're building.** Aneural renders any directory — one repo or a whole
ecosystem of them — as a growing mycelium of typed nodes and relationships, keeps it current as you
vibe-code with Claude Code or Codex, and hands the part you're looking at to your assistant as context.

- **GUI** (`aneural-gui`, Rust + Bevy): 2D fungal graph of folders, files, repos, manifests, packages and
  spore-derived nodes (comments, plans, ideas, notes). Typed edges: `CONTAINS`, `IMPORTS`, `RE_EXPORTS`,
  `REFERENCES`, `DEPENDS_ON`, `ANNOTATES`, `RELATES_TO`. Grows live as files change.
- **CLI** (`aneural`, npm): `aneural init` creates an Obsidian-style `.aneural/` directory; `index`,
  `query`, `focus`, `spores`, `doctor`, `mcp`.
- **Spores**: lightweight, *inferred* alternatives to JIRA/Obsidian. Nothing to maintain — TODOs, plan
  docs, an icebox of ideas and wiki-links are harvested from files you already have. Declarative
  manifests, installable from the Open Spores Marketplace.
- **Context loader**: the GUI's filters + selection are written to `.aneural/state/focus.json`; the MCP
  server (`aneural mcp`) serves exactly that neighbourhood — nodes, edges, file contents — to any MCP client.

Languages analysed for imports: TypeScript/JavaScript (full resolution via tsconfig paths, `exports`
maps, `.js`→`.ts`), Python, Rust, Go, Java, PHP, Ruby (best-effort resolution). Dependency manifests:
package.json, Cargo.toml, pyproject.toml, requirements.txt, go.mod, pom.xml, build.gradle(.kts),
composer.json, Gemfile.

## Layout

```
crates/aneural-core     graph model, ids, .aneural config / focus / spore schemas
crates/aneural-store    SQLite index (.aneural/cache/index.db)
crates/aneural-lang     tree-sitter import extraction + module resolution
crates/aneural-engine   walker, watcher, manifests, spore harvesters → GraphDelta stream
crates/aneural-icons    curated icondata registry + rasterizer
crates/aneural-napi     Node addon → packages/core (@aneural/core)
crates/aneural-gui      Bevy desktop app
packages/core           @aneural/core (napi bindings)
packages/cli            aneural (CLI)
packages/mcp            @aneural/mcp (MCP server)
spores/                 first-party spores: comments, plans, icebox, wiki-links
fixtures/sample-workspace   multi-language demo/test workspace
```

## Develop

```sh
# prerequisites: rustup (stable), Node ≥ 22.12, pnpm 12
pnpm install
cargo test --workspace --exclude aneural-gui        # Rust crates
pnpm build:native                                   # builds @aneural/core (.node) for this machine
pnpm -r build && pnpm -r test                       # CLI + MCP
cargo run -p aneural-gui --features dev -- fixtures/sample-workspace   # the app
```

Try it on the sample workspace:

```sh
cd fixtures/sample-workspace
node ../../packages/cli/dist/cli.js index
node ../../packages/cli/dist/cli.js query util
```

Hook the MCP server into Claude Code from any workspace with a `.aneural/` directory:

```sh
aneural init --claude          # writes .mcp.json → { "aneural": { "command": "npx", "args": ["-y", "aneural", "mcp"] } }
```

See `docs/` for the architecture, graph schema, spore format, addon API and GUI notes.
