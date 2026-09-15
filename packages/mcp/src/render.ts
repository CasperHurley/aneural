import type { Edge, Node } from '@aneural/core';
import type { FocusBundle } from './types.js';

const FENCE_LANG: Record<string, string> = {
  ts: 'ts',
  tsx: 'tsx',
  mts: 'ts',
  cts: 'ts',
  js: 'js',
  jsx: 'jsx',
  mjs: 'js',
  cjs: 'js',
  py: 'python',
  rs: 'rust',
  go: 'go',
  java: 'java',
  php: 'php',
  rb: 'ruby',
  md: 'markdown',
  json: 'json',
  toml: 'toml',
  yaml: 'yaml',
  yml: 'yaml',
  html: 'html',
  css: 'css',
  sh: 'bash',
  sql: 'sql',
};

export function fenceLang(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return FENCE_LANG[ext] ?? '';
}

export function describeNode(n: Node): string {
  const loc = n.path ? ` \`${n.path}\`` : '';
  return `**${n.id}** — ${n.label} (${n.kind})${loc}`;
}

export function describeEdge(e: Edge, byId: Map<string, Node>): string {
  const label = (id: string): string => byId.get(id)?.label ?? id;
  const line =
    e.props && typeof e.props === 'object' && 'line' in e.props
      ? ` @${(e.props as { line: unknown }).line}`
      : '';
  return `${label(e.src)} -[${e.kind}]-> ${label(e.dst)}${line}  (${e.src} → ${e.dst})`;
}

/** Markdown rendering of a focus bundle — what the assistant actually reads. */
export function renderFocusMarkdown(b: FocusBundle): string {
  const out: string[] = [];
  out.push(`# Aneural focus — ${b.workspaceName}`);
  out.push('');
  if (!b.focus) {
    out.push('No focus has been set for this workspace yet.');
    out.push('');
    out.push(
      'Open the Aneural GUI and select a node (or run `aneural focus set <node-id>`) to define the context.',
    );
    out.push(`Workspace root: \`${b.root}\``);
    return out.join('\n');
  }
  out.push(`Workspace root: \`${b.root}\`  `);
  if (b.focus.updatedAt) out.push(`Updated: ${b.focus.updatedAt}  `);
  out.push(`Neighborhood: depth ${b.depth}, direction ${b.direction}`);
  if (b.focus.notes?.trim()) {
    out.push('');
    out.push('## Notes from the user');
    out.push('');
    out.push(b.focus.notes.trim());
  }
  const f = b.focus.filters ?? {};
  const filterBits: string[] = [];
  if (f.kinds?.length) filterBits.push(`kinds: ${f.kinds.join(', ')}`);
  if (f.edgeKinds?.length) filterBits.push(`edges: ${f.edgeKinds.join(', ')}`);
  if (f.repos?.length) filterBits.push(`repos: ${f.repos.join(', ')}`);
  if (f.query?.trim()) filterBits.push(`query: "${f.query.trim()}"`);
  if (filterBits.length) {
    out.push('');
    out.push(`Active filters — ${filterBits.join('; ')}`);
  }

  out.push('');
  out.push('## Selection');
  out.push('');
  if (b.anchors.length === 0) {
    out.push('_Nothing selected._');
  } else {
    const primary = b.focus.selection?.primary;
    for (const n of b.anchors) {
      out.push(`- ${describeNode(n)}${n.id === primary ? ' [primary]' : ' [pinned]'}`);
    }
  }

  const anchorIds = new Set(b.anchors.map((n) => n.id));
  const others = b.nodes.filter((n) => !anchorIds.has(n.id));
  if (others.length) {
    out.push('');
    out.push(`## Connected nodes (${others.length})`);
    out.push('');
    for (const n of others) out.push(`- ${describeNode(n)}`);
  }

  if (b.edges.length) {
    const byId = new Map(b.nodes.map((n) => [n.id, n]));
    out.push('');
    out.push(`## Edges (${b.edges.length})`);
    out.push('');
    for (const e of b.edges) out.push(`- ${describeEdge(e, byId)}`);
  }

  const withContent = b.files.filter((f) => f.content !== undefined || f.skipped);
  if (withContent.length) {
    out.push('');
    out.push('## Files');
    for (const file of withContent) {
      out.push('');
      out.push(`### ${file.path}`);
      if (file.skipped) {
        out.push(`_content skipped (${file.skipped})_`);
        continue;
      }
      out.push('');
      out.push(`\`\`\`${fenceLang(file.path)}`);
      out.push(file.content ?? '');
      out.push('```');
      if (file.truncated) out.push('_(truncated to fit the byte budget)_');
    }
  }
  return out.join('\n');
}

export function renderNodeMarkdown(node: Node, edges: Edge[]): string {
  const out = [describeNode(node), ''];
  const props =
    node.props && typeof node.props === 'object' ? (node.props as Record<string, unknown>) : {};
  const keys = Object.keys(props);
  if (keys.length) {
    out.push('Props:');
    for (const k of keys) out.push(`- ${k}: ${JSON.stringify(props[k])}`);
    out.push('');
  }
  if (edges.length) {
    out.push('Edges:');
    for (const e of edges) out.push(`- ${e.src} -[${e.kind}]-> ${e.dst}`);
  }
  return out.join('\n');
}

export function renderNodeList(nodes: Node[]): string {
  if (!nodes.length) return '_No nodes._';
  return nodes.map((n) => `- ${describeNode(n)}`).join('\n');
}
