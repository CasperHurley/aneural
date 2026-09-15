# GUI (`aneural-gui`)

```sh
cargo run -p aneural-gui --features dev -- <directory>     # omit the path to get a folder picker
```

- **Engine thread** runs `aneural_engine::run` (full index, then watch) and streams `EngineEvent`s over a
  channel. A `PreUpdate` system applies deltas with a per-frame node budget (`gui.growthBudgetPerFrame`)
  so the graph visibly grows.
- **Nodes** are `Mesh2d` circles coloured by kind with a rasterised icondata icon sprite and a label
  shown when zoomed in past `gui.labelZoomThreshold`. New nodes sprout from their parent and scale in.
- **Edges** are gizmo cubic Béziers with per-edge deterministic curvature (hyphae). Retained lyon meshes
  and a WGSL pulse material are the upgrade path.
- **Layout**: force-directed on `FixedUpdate` — repulsion, springs per edge kind, gravity toward the
  CONTAINS parent, damping, energy-based freeze. Drag pins a node.
- **Panels** (egui): filters (kinds, edge kinds, repos, search, focus mode + depth), inspector (props,
  edges, pins, notes for the assistant, open in editor), spores (comments/plans/ideas/notes), status bar.
- **Focus writer**: `Filters + Selection + neighbourhood + visible ids + notes` → debounced atomic write
  of `.aneural/state/focus.json` (also on exit). This is what `aneural mcp` serves.
- **Camera**: the view follows the graph's bounds (centred on the canvas, i.e. the window minus the
  egui panels) while it first grows, and stops following on the first manual pan/zoom or once the
  index is complete and the layout has settled. Left-drag on empty canvas pans; left-drag on a node
  moves it; right/middle-drag pans anywhere; wheel/pinch zooms to the cursor.
- Keys: `F` frame all · `Space` re-run layout · `R` reindex · arrows/WASD pan.
