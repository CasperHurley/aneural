# `@aneural/core`

Native bindings (napi-rs) over `aneural-engine` and `aneural-store`. Stateless: every call opens the
workspace (SQLite open is cheap). Types are in the generated `index.d.ts`.

| Function | Purpose |
|---|---|
| `version()` | crate version |
| `findWorkspace(startDir)` | nearest ancestor with `.aneural/` |
| `initWorkspace(root, { name?, force? })` / `workspaceInfo(root)` | create / describe |
| `loadConfig(root)` | effective config with defaults |
| `indexWorkspace(root, { full? })` (Promise) / `indexWorkspaceSync` | index; returns `IndexStats` |
| `queryNodes(root, { kinds?, repo?, pathPrefix?, text?, limit? })` | search |
| `getNode`, `getNodes`, `getEdges(root, { src?, dst?, kinds?, limit? })` | reads |
| `neighborhood(root, ids, { depth?, direction?, edgeKinds?, limit? })` | BFS subgraph |
| `snapshot(root)`, `counts(root)`, `listUnresolved(root, limit?)` | whole graph / stats |
| `readFocus(root)`, `writeFocus(root, focus)` | `.aneural/state/focus.json` |
| `listNodeTypes(root)`, `listSpores(root)`, `validateSpore(path)`, `harvestSpore(root, name)` | schema |
| `doctor(root)` | diagnostics |
| `listIcons()` | valid icon names |
| `readFile(root, relPath, startLine?, endLine?)` | bounded, workspace-jailed read |
| `watch(root, cb)` → `WatchHandle` | full index then live deltas (`{type: "delta"|"progress"|"indexComplete"|"watching"|"error"}`) |

Build locally: `pnpm --filter @aneural/core build:debug` (debug) or `build` (release). Prebuilt binaries
for the five targets in `package.json#napi.targets` are published by `.github/workflows/napi-release.yml`.
