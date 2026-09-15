import type { Edge, Node, Subgraph } from '@aneural/core';
import type { BuildFocusOptions, FocusBundle, FocusFile, FocusShape } from './types.js';

/** The slice of `@aneural/core` the focus bundle needs (injectable for tests). */
export interface FocusApi {
  readFocus(root: string): FocusShape | null;
  getNodes(root: string, ids: string[]): Node[];
  neighborhood(
    root: string,
    ids: string[],
    opts: { depth: number; direction: string; edgeKinds?: string[]; limit?: number },
  ): Subgraph;
  readFile(root: string, relPath: string): string;
  loadConfig(root: string): { name?: string } | null;
}

const BINARY_EXT = new Set([
  'png',
  'jpg',
  'jpeg',
  'gif',
  'webp',
  'ico',
  'bmp',
  'pdf',
  'zip',
  'gz',
  'tgz',
  'tar',
  'bz2',
  'xz',
  '7z',
  'node',
  'wasm',
  'woff',
  'woff2',
  'ttf',
  'otf',
  'eot',
  'mp3',
  'mp4',
  'mov',
  'avi',
  'wav',
  'ogg',
  'sqlite',
  'db',
  'so',
  'dylib',
  'dll',
  'exe',
  'bin',
  'class',
  'jar',
  'pyc',
  'o',
  'a',
]);

export const DEFAULT_MAX_BYTES: number = 60_000;
export const NEIGHBORHOOD_LIMIT: number = 400;

export function isBinaryPath(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return path.includes('.') && BINARY_EXT.has(ext);
}

export function workspaceName(api: FocusApi, root: string): string {
  try {
    const cfg = api.loadConfig(root);
    if (cfg?.name) return cfg.name;
  } catch {
    // fall through
  }
  return root.split(/[\\/]/).filter(Boolean).pop() ?? root;
}

/** Resolve the current focus into nodes, edges and (optionally) file contents. */
export function buildFocusBundle(
  api: FocusApi,
  root: string,
  opts: BuildFocusOptions = {},
): FocusBundle {
  const includeContent = opts.includeContent ?? true;
  const maxBytes = opts.maxBytes ?? DEFAULT_MAX_BYTES;
  const name = workspaceName(api, root);
  const focus = api.readFocus(root);
  if (!focus) {
    return {
      root,
      workspaceName: name,
      focus: null,
      anchors: [],
      nodes: [],
      edges: [],
      files: [],
      depth: 0,
      direction: 'both',
    };
  }
  const anchorIds: string[] = [];
  const primary = focus.selection?.primary;
  if (primary) anchorIds.push(primary);
  for (const id of focus.selection?.pinned ?? []) {
    if (!anchorIds.includes(id)) anchorIds.push(id);
  }
  const depth = opts.depth ?? focus.neighborhood?.depth ?? 1;
  const direction = focus.neighborhood?.direction ?? 'both';
  const anchors = anchorIds.length ? api.getNodes(root, anchorIds) : [];

  let nodes: Node[] = anchors;
  let edges: Edge[] = [];
  if (anchorIds.length) {
    const sub = api.neighborhood(root, anchorIds, {
      depth,
      direction,
      edgeKinds: focus.filters?.edgeKinds?.length ? focus.filters.edgeKinds : undefined,
      limit: NEIGHBORHOOD_LIMIT,
    });
    nodes = sub.nodes;
    edges = sub.edges;
  }
  const allowedKinds = focus.filters?.kinds ?? [];
  if (allowedKinds.length) {
    const keep = new Set(
      nodes
        .filter((n) => allowedKinds.includes(n.kind) || anchorIds.includes(n.id))
        .map((n) => n.id),
    );
    nodes = nodes.filter((n) => keep.has(n.id));
    edges = edges.filter((e) => keep.has(e.src) && keep.has(e.dst));
  }

  const files: FocusFile[] = [];
  let budget = maxBytes;
  // anchors first, then the rest, so the selected file is never the one that gets cut
  const ordered = [...nodes].sort(
    (a, b) => Number(anchorIds.includes(b.id)) - Number(anchorIds.includes(a.id)),
  );
  for (const node of ordered) {
    if (!(node.kind === 'File' || node.kind === 'Manifest') || !node.path) continue;
    const file: FocusFile = { path: node.path, node };
    if (includeContent) {
      if (isBinaryPath(node.path)) {
        file.skipped = 'binary';
      } else if (budget <= 0) {
        file.skipped = 'budget';
      } else {
        try {
          let content = api.readFile(root, node.path);
          if (content.length > budget) {
            content = content.slice(0, budget);
            file.truncated = true;
          }
          budget -= content.length;
          file.content = content;
        } catch (e) {
          file.skipped = `unreadable: ${(e as Error).message}`;
        }
      }
    }
    files.push(file);
  }
  return { root, workspaceName: name, focus, anchors, nodes, edges, files, depth, direction };
}
