import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import * as core from '../index.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = path.resolve(here, '../../../fixtures/sample-workspace');
let root = '';

beforeAll(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), 'aneural-core-'));
  fs.cpSync(fixture, root, { recursive: true });
  // never inherit an index cache or focus left behind by a local GUI run
  fs.rmSync(path.join(root, '.aneural/cache'), { recursive: true, force: true });
  fs.rmSync(path.join(root, '.aneural/state'), { recursive: true, force: true });
  fs.mkdirSync(path.join(root, 'apps/web/.git'), { recursive: true });
  root = fs.realpathSync(root);
});

afterAll(() => {
  fs.rmSync(root, { recursive: true, force: true });
});

describe('@aneural/core', () => {
  it('reports a version and finds the workspace', () => {
    expect(typeof core.version()).toBe('string');
    expect(core.findWorkspace(path.join(root, 'apps/web/src'))).toBe(root);
    expect(core.findWorkspace(os.tmpdir())).toBeNull();
  });

  it('indexes and answers graph queries', async () => {
    const stats = await core.indexWorkspace(root);
    expect(stats.filesIndexed).toBeGreaterThan(20);
    expect(core.counts(root).nodes).toBeGreaterThan(50);
    const nb = core.neighborhood(root, ['file:apps/web/src/index.ts'], {
      depth: 1,
      direction: 'out',
    });
    expect(nb.nodes.map((n) => n.id)).toContain('file:apps/web/src/app.ts');
    expect(core.queryNodes(root, { text: 'util', kinds: ['File'] }).map((n) => n.id)).toContain(
      'file:apps/web/src/lib/util.ts',
    );
    expect(core.getEdges(root, { src: 'file:apps/web/src/index.ts' }).length).toBeGreaterThan(1);
    expect(core.listNodeTypes(root).map((t) => t.kind)).toContain('Idea');
    const spores = core.listSpores(root);
    // Four enabled by default, plus `database`, which ships off because it
    // would walk every .db in the workspace.
    expect(spores.map((s) => s.id)).toContain('aneural.comments');
    expect(spores.find((s) => s.id === 'aneural.database')?.enabled).toBe(false);
    expect(spores.filter((s) => s.enabled)).toHaveLength(4);
    expect(core.listIcons()).toContain('LuLeaf');
    expect(core.readFile(root, 'apps/web/src/types.ts', 1, 1)).toContain('interface Props');
    expect(() => core.readFile(root, '../outside')).toThrow();
  });

  it('round-trips focus.json', () => {
    expect(core.readFocus(root)).toBeNull();
    core.writeFocus(root, {
      version: 1,
      updatedAt: '',
      workspace: root,
      filters: {},
      selection: { primary: 'file:apps/web/src/index.ts', pinned: [] },
      neighborhood: { depth: 1, direction: 'both' },
      visibleNodeIds: [],
      notes: 'hello',
    });
    const f = core.readFocus(root);
    expect(f?.selection.primary).toBe('file:apps/web/src/index.ts');
    expect(f?.notes).toBe('hello');
  });
});
