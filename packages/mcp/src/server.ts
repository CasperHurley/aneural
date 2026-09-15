import * as fs from 'node:fs';
import * as path from 'node:path';
import * as core from '@aneural/core';
import { McpServer, ResourceTemplate } from '@modelcontextprotocol/server';
import { z } from 'zod';
import { buildFocusBundle, type FocusApi } from './focus.js';
import { renderFocusMarkdown, renderNodeList, renderNodeMarkdown } from './render.js';
import { VERSION } from './version.js';

export interface AneuralServerOptions {
  /** Absolute workspace root (a directory containing `.aneural/`). */
  root: string;
  /** Override the core bindings (tests). */
  api?: FocusApi & Partial<typeof core>;
  /** Watch focus.json and emit resource-updated notifications (default true). */
  watchFocus?: boolean;
}

export interface AneuralServer {
  server: McpServer;
  root: string;
  close(): Promise<void>;
}

const FOCUS_URI = 'aneural://focus';
const CONTEXT_URI = 'aneural://focus/context.md';

function log(msg: string): void {
  process.stderr.write(`[aneural-mcp] ${msg}\n`);
}

function text(
  t: string,
  structured?: unknown,
): {
  content: { type: 'text'; text: string }[];
  structuredContent?: Record<string, unknown>;
} {
  const result: {
    content: { type: 'text'; text: string }[];
    structuredContent?: Record<string, unknown>;
  } = {
    content: [{ type: 'text', text: t }],
  };
  if (structured !== undefined) result.structuredContent = structured as Record<string, unknown>;
  return result;
}

function json(v: unknown): string {
  return JSON.stringify(v, null, 2);
}

export function createAneuralServer(opts: AneuralServerOptions): AneuralServer {
  const root = path.resolve(opts.root);
  const api: FocusApi & typeof core = { ...core, ...(opts.api ?? {}) } as FocusApi & typeof core;
  const server = new McpServer(
    { name: 'aneural', version: VERSION, title: 'Aneural' },
    { capabilities: { tools: {}, resources: { subscribe: true, listChanged: true }, prompts: {} } },
  );

  const focusBundle = (o: { includeContent?: boolean; maxBytes?: number; depth?: number }) =>
    buildFocusBundle(api, root, o);

  // ---- tools --------------------------------------------------------------

  server.registerTool(
    'aneural_focus',
    {
      title: 'Current focus',
      description:
        'The context the user is currently looking at in the Aneural graph: selected + pinned nodes, their neighbourhood, edges, and file contents. Call this first.',
      inputSchema: z.object({
        includeContent: z.boolean().optional().describe('Include file contents (default true)'),
        maxBytes: z
          .number()
          .int()
          .positive()
          .optional()
          .describe('Total byte budget for file contents (default 60000)'),
        depth: z
          .number()
          .int()
          .min(0)
          .max(5)
          .optional()
          .describe('Override the neighbourhood depth'),
      }),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async (args) => {
      const b = focusBundle(args);
      return text(renderFocusMarkdown(b), {
        focus: b.focus,
        nodes: b.nodes,
        edges: b.edges,
        files: b.files.map((f) => ({
          path: f.path,
          node: f.node,
          content: f.content,
          truncated: f.truncated,
          skipped: f.skipped,
        })),
      });
    },
  );

  server.registerTool(
    'aneural_search_nodes',
    {
      title: 'Search nodes',
      description:
        'Search graph nodes by text (label/path/id substring), kind, repo or path prefix.',
      inputSchema: z.object({
        text: z.string().optional(),
        kinds: z
          .array(z.string())
          .optional()
          .describe(
            'e.g. ["File","Directory","Comment","Plan","Idea","Note","Package","Repo","Manifest"]',
          ),
        repo: z.string().optional().describe('Repo node id, e.g. dir:apps/web'),
        pathPrefix: z.string().optional(),
        limit: z.number().int().positive().optional(),
      }),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async (args) => {
      const nodes = api.queryNodes(root, { ...args, limit: args.limit ?? 50 });
      return text(renderNodeList(nodes), { nodes });
    },
  );

  server.registerTool(
    'aneural_get_node',
    {
      title: 'Get node',
      description: 'One node by id with its edges.',
      inputSchema: z.object({ id: z.string() }),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async ({ id }) => {
      const node = api.getNode(root, id);
      if (!node) return text(`No node with id ${id}`, { node: null, edges: [] });
      const edges = [...api.getEdges(root, { src: id }), ...api.getEdges(root, { dst: id })];
      return text(renderNodeMarkdown(node, edges), { node, edges });
    },
  );

  server.registerTool(
    'aneural_neighborhood',
    {
      title: 'Neighborhood',
      description: 'BFS neighbourhood of one or more nodes.',
      inputSchema: z.object({
        ids: z.array(z.string()).min(1),
        depth: z.number().int().min(0).max(6).optional(),
        direction: z.enum(['in', 'out', 'both']).optional(),
        edgeKinds: z.array(z.string()).optional(),
        limit: z.number().int().positive().optional(),
      }),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async ({ ids, depth, direction, edgeKinds, limit }) => {
      const sub = api.neighborhood(root, ids, {
        depth: depth ?? 1,
        direction: direction ?? 'both',
        edgeKinds,
        limit: limit ?? 200,
      });
      const byId = new Map(sub.nodes.map((n) => [n.id, n]));
      const lines = [
        renderNodeList(sub.nodes),
        '',
        ...sub.edges.map(
          (e) =>
            `- ${byId.get(e.src)?.label ?? e.src} -[${e.kind}]-> ${byId.get(e.dst)?.label ?? e.dst}`,
        ),
      ];
      if (sub.truncated) lines.push('', '_(truncated)_');
      return text(lines.join('\n'), sub as unknown as Record<string, unknown>);
    },
  );

  server.registerTool(
    'aneural_get_edges',
    {
      title: 'Get edges',
      description: 'Edges by source, destination and/or kind.',
      inputSchema: z.object({
        src: z.string().optional(),
        dst: z.string().optional(),
        kinds: z.array(z.string()).optional(),
        limit: z.number().int().positive().optional(),
      }),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async (args) => {
      const edges = api.getEdges(root, { ...args, limit: args.limit ?? 200 });
      return text(
        edges.length
          ? edges.map((e) => `- ${e.src} -[${e.kind}]-> ${e.dst}`).join('\n')
          : '_No edges._',
        { edges },
      );
    },
  );

  server.registerTool(
    'aneural_read_file',
    {
      title: 'Read file',
      description:
        'Read a workspace file (workspace-relative path), optionally a 1-based inclusive line range. Capped at 100 KB.',
      inputSchema: z.object({
        path: z.string(),
        startLine: z.number().int().positive().optional(),
        endLine: z.number().int().positive().optional(),
      }),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async ({ path: rel, startLine, endLine }) => {
      let content = api.readFile(root, rel, startLine, endLine);
      let truncated = false;
      if (content.length > 100_000) {
        content = content.slice(0, 100_000);
        truncated = true;
      }
      return text(content + (truncated ? '\n…(truncated)' : ''), { path: rel, content, truncated });
    },
  );

  server.registerTool(
    'aneural_list_node_types',
    {
      title: 'Node types',
      description: 'All node kinds (builtin, spore-provided, workspace-defined) with icon/colour.',
      inputSchema: z.object({}),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async () => {
      const types = api.listNodeTypes(root);
      return text(
        types.map((t) => `- ${t.kind} (${t.provider}) — ${t.description || t.label}`).join('\n'),
        { nodeTypes: types },
      );
    },
  );

  server.registerTool(
    'aneural_list_spores',
    {
      title: 'Spores',
      description:
        'Installed spores (lightweight harvesters that add node kinds) and whether they are enabled.',
      inputSchema: z.object({}),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async () => {
      const spores = api.listSpores(root);
      return text(
        spores
          .map((s) => `- ${s.name}@${s.version} ${s.enabled ? '[on]' : '[off]'} — ${s.description}`)
          .join('\n'),
        { spores },
      );
    },
  );

  server.registerTool(
    'aneural_counts',
    {
      title: 'Counts',
      description: 'Node/edge/file counts, per kind.',
      inputSchema: z.object({}),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async () => {
      const c = api.counts(root);
      return text(
        `${c.nodes} nodes, ${c.edges} edges, ${c.files} files, ${c.unresolved} unresolved imports\n${c.byKind.map((k) => `- ${k.kind}: ${k.count}`).join('\n')}`,
        c as unknown as Record<string, unknown>,
      );
    },
  );

  server.registerTool(
    'aneural_doctor',
    {
      title: 'Doctor',
      description: 'Health diagnostics: unresolved imports, bad spores/icons, stale cache.',
      inputSchema: z.object({}),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
    },
    async () => {
      const diags = api.doctor(root);
      return text(
        diags.length
          ? diags
              .map(
                (d) => `- [${d.level}] ${d.category}: ${d.message}${d.path ? ` (${d.path})` : ''}`,
              )
              .join('\n')
          : 'All clear.',
        { diagnostics: diags },
      );
    },
  );

  server.registerTool(
    'aneural_index',
    {
      title: 'Index workspace',
      description: 'Re-index the workspace (incremental unless full=true).',
      inputSchema: z.object({ full: z.boolean().optional() }),
      annotations: {
        readOnlyHint: false,
        idempotentHint: true,
        destructiveHint: false,
        openWorldHint: false,
      },
    },
    async ({ full }) => {
      const stats = await api.indexWorkspace(root, { full: full ?? false });
      return text(
        `Indexed ${stats.filesIndexed} files (${stats.filesSkipped} unchanged) → ${stats.nodes} nodes, ${stats.edges} edges in ${stats.durationMs} ms`,
        stats as unknown as Record<string, unknown>,
      );
    },
  );

  server.registerTool(
    'aneural_add_idea',
    {
      title: 'Add idea to icebox',
      description:
        'Append an idea (a `## heading` + body) to .aneural/icebox/ideas.md; it becomes an Idea node.',
      inputSchema: z.object({ title: z.string().min(1), body: z.string().optional() }),
      annotations: {
        readOnlyHint: false,
        idempotentHint: false,
        destructiveHint: false,
        openWorldHint: false,
      },
    },
    async ({ title, body }) => {
      const file = path.join(root, '.aneural', 'icebox', 'ideas.md');
      fs.mkdirSync(path.dirname(file), { recursive: true });
      if (!fs.existsSync(file)) fs.writeFileSync(file, '# Icebox\n');
      fs.appendFileSync(file, `\n## ${title.trim()}\n${(body ?? '').trim()}\n`);
      return text(`Added "${title.trim()}" to ${path.relative(root, file)}`, {
        path: path.relative(root, file),
        title,
      });
    },
  );

  // ---- resources ----------------------------------------------------------

  server.registerResource(
    'focus',
    FOCUS_URI,
    {
      title: 'Aneural focus',
      description: 'focus.json plus resolved nodes and edges',
      mimeType: 'application/json',
    },
    async (uri) => {
      const b = focusBundle({ includeContent: false });
      return {
        contents: [
          {
            uri: uri.href,
            mimeType: 'application/json',
            text: json({ focus: b.focus, nodes: b.nodes, edges: b.edges }),
          },
        ],
      };
    },
  );

  server.registerResource(
    'focus-context',
    CONTEXT_URI,
    {
      title: 'Aneural focus context',
      description: 'Markdown rendering of the current focus with file contents',
      mimeType: 'text/markdown',
    },
    async (uri) => ({
      contents: [
        { uri: uri.href, mimeType: 'text/markdown', text: renderFocusMarkdown(focusBundle({})) },
      ],
    }),
  );

  server.registerResource(
    'node-types',
    'aneural://node-types',
    { title: 'Node types', mimeType: 'application/json' },
    async (uri) => ({
      contents: [
        { uri: uri.href, mimeType: 'application/json', text: json(api.listNodeTypes(root)) },
      ],
    }),
  );

  server.registerResource(
    'spores',
    'aneural://spores',
    { title: 'Spores', mimeType: 'application/json' },
    async (uri) => ({
      contents: [{ uri: uri.href, mimeType: 'application/json', text: json(api.listSpores(root)) }],
    }),
  );

  server.registerResource(
    'node',
    new ResourceTemplate('aneural://node/{+id}', { list: undefined }),
    { title: 'Graph node', description: 'One node and its edges', mimeType: 'application/json' },
    async (uri, vars) => {
      const id = decodeURIComponent(String(vars.id ?? ''));
      const node = api.getNode(root, id);
      const edges = node
        ? [...api.getEdges(root, { src: id }), ...api.getEdges(root, { dst: id })]
        : [];
      return {
        contents: [{ uri: uri.href, mimeType: 'application/json', text: json({ node, edges }) }],
      };
    },
  );

  server.registerResource(
    'file',
    new ResourceTemplate('aneural://file/{+path}', { list: undefined }),
    { title: 'Workspace file', mimeType: 'text/plain' },
    async (uri, vars) => {
      const rel = decodeURIComponent(String(vars.path ?? ''));
      return {
        contents: [{ uri: uri.href, mimeType: 'text/plain', text: api.readFile(root, rel) }],
      };
    },
  );

  // ---- prompt -------------------------------------------------------------

  server.registerPrompt(
    'aneural_context',
    {
      title: 'Aneural context',
      description: 'Load the currently focused part of the workspace as context.',
    },
    async () => ({
      messages: [
        {
          role: 'user' as const,
          content: { type: 'text' as const, text: renderFocusMarkdown(focusBundle({})) },
        },
      ],
    }),
  );

  // ---- focus.json watcher -------------------------------------------------

  let watcher: fs.FSWatcher | null = null;
  let timer: NodeJS.Timeout | null = null;
  if (opts.watchFocus ?? true) {
    const stateDir = path.join(root, '.aneural', 'state');
    try {
      fs.mkdirSync(stateDir, { recursive: true });
      watcher = fs.watch(stateDir, (_event, file) => {
        if (
          file &&
          !String(file).startsWith('focus.json') &&
          !String(file).startsWith('.focus.json')
        )
          return;
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => {
          timer = null;
          if (!server.isConnected()) return;
          for (const uri of [FOCUS_URI, CONTEXT_URI]) {
            server.server.sendResourceUpdated({ uri }).catch(() => {});
          }
        }, 200);
      });
      watcher.on('error', (e) => log(`focus watcher error: ${e.message}`));
    } catch (e) {
      log(`focus watcher unavailable: ${(e as Error).message}`);
    }
  }

  return {
    server,
    root,
    async close(): Promise<void> {
      if (timer) clearTimeout(timer);
      watcher?.close();
      try {
        await server.close();
      } catch {
        // already closed
      }
    },
  };
}
