# GUI (`aneural-gui`)

```sh
cargo run -p aneural-gui --features dev -- <directory>     # omit the path to get a folder picker
```

- **Engine thread** runs `aneural_engine::run` (full index, then watch) and streams `EngineEvent`s over a
  channel. A `PreUpdate` system applies deltas with a per-frame node budget (`gui.growthBudgetPerFrame`)
  so the graph visibly grows.
- **Nodes** are `Mesh2d` circles coloured by kind with a rasterised icondata icon sprite and a label
  shown when zoomed in past `gui.labelZoomThreshold`. New nodes sprout from their parent and scale in.
  Each carries a halo sprite behind it, unlit by day (see **Circadian** below).
- **Edges** are gizmo cubic Béziers with per-edge deterministic curvature (hyphae). Retained lyon meshes
  and a WGSL pulse material are the upgrade path.
- **Circadian** (`circadian.rs`): one number, `Vibe::night`, read off the local clock — 0 through
  working hours, rising through the evening, 1 around 2:30am — drives the whole look. At 0 the app is
  pixel-for-pixel the tool it has always been: nothing glows, nothing moves. As it rises the palette
  crossfades to a cooler, bioluminescent one (`theme::Palette`), node halos light and breathe, nodes
  wander a couple of pixels off their layout positions (`Drift`, never fed back into the forces or
  picking), pulses of light travel the hyphae, and spore motes drift across the canvas. `gui.circadian`
  (`auto` | `day` | `night`) pins it, and the dial in the top bar cycles the same three for the session.
- **Layout**: force-directed on `FixedUpdate` — repulsion, springs per edge kind, gravity toward the
  CONTAINS parent, damping, energy-based freeze. Drag pins a node.
- **Panels** (egui): filters (kinds, edge kinds, repos, search, focus mode + depth), inspector (props,
  edges, pins, notes for the assistant, open in editor), status bar. The
  **marketplace** is an `egui::Modal` opened from the top bar, not a docked panel: `ee00a1c` removed
  the bottom spores panel because a permanent strip obstructed the graph, and a modal costs nothing
  when closed. Its detail pane also carries a spore's declared `settings` as editable fields and, for
  anything above the declarative tier, a **Refresh now** button. Saving a setting goes through the
  registry worker like every other config write, then reloads spores and re-fetches, so the graph
  reflects the new value without the user hunting for a refresh.
- **Focus writer**: `Filters + Selection + neighbourhood + visible ids + notes` → debounced atomic write
  of `.aneural/state/focus.json` (also on exit). This is what `aneural mcp` serves.
- **Camera**: the view follows the graph's bounds (centred on the canvas, i.e. the window minus the
  egui panels) while it first grows, and stops following on the first manual pan/zoom or once the
  index is complete and the layout has settled. Left-drag on empty canvas pans; left-drag on a node
  moves it; right/middle-drag pans anywhere; wheel/pinch zooms to the cursor.
- Keys: `F` frame all · `Space` re-run layout · `R` reindex · arrows/WASD pan.
