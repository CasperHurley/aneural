import { describe, expect, it } from 'vitest';
import { type Fetcher, fetchRegistry, fetchSpore, manifestUrl, sha256 } from '../src/registry.js';

const manifest = JSON.stringify({ name: 'comments', version: '0.1.0', harvesters: [] });

function fetcher(routes: Record<string, string | number>): Fetcher {
  return async (url: string) => {
    const hit = routes[url];
    if (hit === undefined) return { ok: false, status: 404, text: async () => '' };
    if (typeof hit === 'number') return { ok: false, status: hit, text: async () => '' };
    return { ok: true, status: 200, text: async () => hit };
  };
}

describe('registry', () => {
  it('builds raw github urls', () => {
    expect(
      manifestUrl({ name: 'x', version: '1', repo: 'github:aneural/spores', path: 'comments' }),
    ).toBe('https://raw.githubusercontent.com/aneural/spores/main/comments/spore.json');
    expect(manifestUrl({ name: 'x', version: '1', repo: 'github:o/r#dev', path: '/deep/x/' })).toBe(
      'https://raw.githubusercontent.com/o/r/dev/deep/x/spore.json',
    );
    expect(
      manifestUrl({ name: 'x', version: '1', repo: 'https://example.com/spores/', path: 'x' }),
    ).toBe('https://example.com/spores/x/spore.json');
  });

  it('fetches and verifies a spore', async () => {
    const entry = {
      name: 'comments',
      version: '0.1.0',
      repo: 'github:a/b',
      path: 'comments',
      sha256: sha256(manifest),
    };
    const reg = { version: 1, spores: [entry] };
    const f = fetcher({
      'https://reg/registry.json': JSON.stringify(reg),
      'https://raw.githubusercontent.com/a/b/main/comments/spore.json': manifest,
    });
    const got = await fetchRegistry('https://reg/registry.json', f);
    expect(got.spores[0]?.name).toBe('comments');
    const spore = await fetchSpore(entry, f);
    expect(spore.manifest.version).toBe('0.1.0');
  });

  it('rejects hash mismatches, bad json, wrong names and http errors', async () => {
    const base = { name: 'comments', version: '0.1.0', repo: 'github:a/b', path: 'comments' };
    const url = 'https://raw.githubusercontent.com/a/b/main/comments/spore.json';
    await expect(
      fetchSpore({ ...base, sha256: 'deadbeef' }, fetcher({ [url]: manifest })),
    ).rejects.toThrow(/sha256 mismatch/);
    await expect(fetchSpore(base, fetcher({ [url]: '{not json' }))).rejects.toThrow(
      /not valid JSON/,
    );
    await expect(
      fetchSpore(base, fetcher({ [url]: JSON.stringify({ name: 'other', version: '1' }) })),
    ).rejects.toThrow(/does not match/);
    await expect(fetchSpore(base, fetcher({ [url]: 500 }))).rejects.toThrow(/responded 500/);
    await expect(fetchRegistry('https://reg/r.json', fetcher({}))).rejects.toThrow(/responded 404/);
  });
});
