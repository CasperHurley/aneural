import { describe, expect, it } from 'vitest';
import { buildFocusBundle, type FocusApi } from '../src/focus.js';
import { renderFocusMarkdown } from '../src/render.js';
import type { FocusShape } from '../src/types.js';

const nodes = {
  'file:src/index.ts': {
    id: 'file:src/index.ts',
    kind: 'File',
    label: 'index.ts',
    path: 'src/index.ts',
    props: {},
    source: 'walker',
  },
  'file:src/app.ts': {
    id: 'file:src/app.ts',
    kind: 'File',
    label: 'app.ts',
    path: 'src/app.ts',
    props: {},
    source: 'walker',
  },
  'pkg:npm/react': {
    id: 'pkg:npm/react',
    kind: 'Package',
    label: 'react',
    props: {},
    source: 'lang',
  },
  'file:logo.png': {
    id: 'file:logo.png',
    kind: 'File',
    label: 'logo.png',
    path: 'logo.png',
    props: {},
    source: 'walker',
  },
};

function fakeApi(focus: FocusShape | null): FocusApi {
  return {
    readFocus: () => focus,
    getNodes: (_root, ids) => ids.map((id) => nodes[id as keyof typeof nodes]).filter(Boolean),
    neighborhood: () => ({
      nodes: Object.values(nodes),
      edges: [
        {
          kind: 'IMPORTS',
          src: 'file:src/index.ts',
          dst: 'file:src/app.ts',
          props: { line: 2 },
          source: 'lang',
        },
        {
          kind: 'IMPORTS',
          src: 'file:src/index.ts',
          dst: 'pkg:npm/react',
          props: {},
          source: 'lang',
        },
      ],
      truncated: false,
    }),
    readFile: (_root, p) => `// contents of ${p}\n`.repeat(3),
    loadConfig: () => ({ name: 'demo' }),
  };
}

const focus: FocusShape = {
  version: 1,
  updatedAt: '2026-09-15T00:00:00Z',
  workspace: '/tmp/demo',
  filters: { kinds: [], edgeKinds: [], repos: [], query: '' },
  selection: { primary: 'file:src/index.ts', pinned: ['file:src/app.ts'] },
  neighborhood: { depth: 1, direction: 'both' },
  visibleNodeIds: [],
  notes: 'please be careful with hydration',
};

describe('renderFocusMarkdown', () => {
  it('explains when no focus exists', () => {
    const b = buildFocusBundle(fakeApi(null), '/tmp/demo');
    const md = renderFocusMarkdown(b);
    expect(md).toContain('No focus has been set');
    expect(md).toContain('/tmp/demo');
  });

  it('renders selection, edges and file contents', () => {
    const b = buildFocusBundle(fakeApi(focus), '/tmp/demo', { maxBytes: 10_000 });
    expect(b.anchors.map((n) => n.id)).toEqual(['file:src/index.ts', 'file:src/app.ts']);
    const md = renderFocusMarkdown(b);
    expect(md).toContain('# Aneural focus — demo');
    expect(md).toContain('please be careful with hydration');
    expect(md).toContain('**file:src/index.ts** — index.ts (File) `src/index.ts` [primary]');
    expect(md).toContain('[pinned]');
    expect(md).toContain('index.ts -[IMPORTS]-> app.ts @2');
    expect(md).toContain('### src/index.ts');
    expect(md).toContain('```ts');
    expect(md).toContain('// contents of src/index.ts');
    expect(md).toContain('_content skipped (binary)_');
    expect(md).not.toContain('### pkg');
  });

  it('respects the byte budget, anchors first', () => {
    const b = buildFocusBundle(fakeApi(focus), '/tmp/demo', { maxBytes: 30 });
    const index = b.files.find((f) => f.path === 'src/index.ts');
    expect(index?.truncated).toBe(true);
    expect(index?.content?.length).toBe(30);
    const app = b.files.find((f) => f.path === 'src/app.ts');
    expect(app?.skipped).toBe('budget');
    expect(renderFocusMarkdown(b)).toContain('truncated to fit');
  });

  it('can omit content and honours kind filters', () => {
    const filtered: FocusShape = { ...focus, filters: { kinds: ['File'] } };
    const b = buildFocusBundle(fakeApi(filtered), '/tmp/demo', { includeContent: false });
    expect(b.nodes.some((n) => n.kind === 'Package')).toBe(false);
    expect(b.edges.every((e) => e.dst !== 'pkg:npm/react')).toBe(true);
    expect(b.files.every((f) => f.content === undefined)).toBe(true);
    expect(renderFocusMarkdown(b)).toContain('Active filters — kinds: File');
  });
});
