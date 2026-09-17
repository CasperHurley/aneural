import * as fs from 'node:fs';
import * as path from 'node:path';
import * as core from '@aneural/core';
import { Client, InMemoryTransport } from '@modelcontextprotocol/client';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createAneuralServer } from '../src/server.js';
import { copyFixture } from './helpers.js';

function resourceText(content: unknown): string {
  return content && typeof content === 'object' && 'text' in content
    ? String((content as { text: unknown }).text)
    : '';
}

const TOOLS = [
  'aneural_focus',
  'aneural_search_nodes',
  'aneural_get_node',
  'aneural_neighborhood',
  'aneural_get_edges',
  'aneural_read_file',
  'aneural_list_node_types',
  'aneural_list_spores',
  'aneural_counts',
  'aneural_doctor',
  'aneural_index',
  'aneural_add_idea',
];

function textOf(result: { content?: unknown }): string {
  const content = (result.content ?? []) as { type: string; text?: string }[];
  return content
    .filter((c) => c.type === 'text')
    .map((c) => c.text ?? '')
    .join('\n');
}

describe('aneural MCP server', () => {
  let root: string;
  let client: Client;
  let handle: ReturnType<typeof createAneuralServer>;

  beforeAll(async () => {
    root = copyFixture();
    core.indexWorkspaceSync(root);
    handle = createAneuralServer({ root, watchFocus: false });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    client = new Client({ name: 'test', version: '0.0.0' });
    await handle.server.connect(serverTransport);
    await client.connect(clientTransport);
  });

  afterAll(async () => {
    await client.close();
    await handle.close();
  });

  it('lists every tool', async () => {
    const { tools } = await client.listTools();
    const names = tools.map((t) => t.name).sort();
    expect(names).toEqual([...TOOLS].sort());
  });

  it('reports a missing focus', async () => {
    const r = await client.callTool({ name: 'aneural_focus', arguments: {} });
    expect(textOf(r)).toContain('No focus has been set');
  });

  it('serves the focused context once a focus is written', async () => {
    core.writeFocus(root, {
      version: 1,
      updatedAt: new Date().toISOString(),
      workspace: root,
      filters: { kinds: [], edgeKinds: [], repos: [], query: '' },
      selection: { primary: 'file:apps/web/src/index.ts', pinned: [] },
      neighborhood: { depth: 1, direction: 'out' },
      visibleNodeIds: [],
      notes: 'hydration matters',
    });
    const r = await client.callTool({ name: 'aneural_focus', arguments: {} });
    const text = textOf(r);
    expect(text).toContain('hydration matters');
    expect(text).toContain("import { App } from '@/app';");
    const structured = r.structuredContent as { nodes: { id: string }[] };
    expect(structured.nodes.map((n) => n.id)).toContain('file:apps/web/src/app.ts');

    const ctx = await client.readResource({ uri: 'aneural://focus/context.md' });
    expect(ctx.contents[0]?.mimeType).toBe('text/markdown');
    const prompt = await client.getPrompt({ name: 'aneural_context' });
    expect(JSON.stringify(prompt.messages)).toContain('hydration matters');
  });

  it('searches, reads nodes/files, and lists schema', async () => {
    const s = await client.callTool({ name: 'aneural_search_nodes', arguments: { text: 'util' } });
    expect(textOf(s)).toContain('file:apps/web/src/lib/util.ts');
    const n = await client.callTool({
      name: 'aneural_get_node',
      arguments: { id: 'file:apps/web/src/app.ts' },
    });
    expect(textOf(n)).toContain('IMPORTS');
    const f = await client.callTool({
      name: 'aneural_read_file',
      arguments: { path: 'apps/web/src/types.ts', startLine: 1, endLine: 1 },
    });
    expect(textOf(f)).toBe('export interface Props {');
    const nb = await client.callTool({
      name: 'aneural_neighborhood',
      arguments: { ids: ['file:apps/web/src/index.ts'], depth: 2 },
    });
    expect(textOf(nb)).toContain('util.ts');
    const types = await client.callTool({ name: 'aneural_list_node_types', arguments: {} });
    expect(textOf(types)).toContain('Idea');
    const spores = await client.callTool({ name: 'aneural_list_spores', arguments: {} });
    expect(textOf(spores)).toContain('comments@');
    const counts = await client.callTool({ name: 'aneural_counts', arguments: {} });
    expect(textOf(counts)).toMatch(/\d+ nodes/);
    const doc = await client.callTool({ name: 'aneural_doctor', arguments: {} });
    expect(textOf(doc)).not.toContain('[error]');
    const node = await client.readResource({ uri: 'aneural://node/file:apps/web/src/app.ts' });
    expect(resourceText(node.contents[0])).toContain('"id": "file:apps/web/src/app.ts"');
    const file = await client.readResource({ uri: 'aneural://file/apps/web/src/types.ts' });
    expect(resourceText(file.contents[0])).toContain('interface Props');
  });

  it('adds ideas and re-indexes', async () => {
    await client.callTool({
      name: 'aneural_add_idea',
      arguments: { title: 'Ship it', body: 'soon' },
    });
    const md = fs.readFileSync(path.join(root, '.aneural/icebox/ideas.md'), 'utf8');
    expect(md).toContain('## Ship it');
    const r = await client.callTool({ name: 'aneural_index', arguments: {} });
    expect(textOf(r)).toMatch(/Indexed \d+ files/);
    const s = await client.callTool({
      name: 'aneural_search_nodes',
      arguments: { text: 'Ship it', kinds: ['Idea'] },
    });
    expect(textOf(s)).toContain('idea:.aneural/icebox/ideas.md#ship-it');
  });
});
