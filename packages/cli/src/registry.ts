import { createHash } from 'node:crypto';

export interface RegistryEntry {
  name: string;
  description?: string;
  version: string;
  /** `github:owner/repo[#ref]` or an https base URL. */
  repo: string;
  /** Path inside the repo containing `spore.json`. */
  path: string;
  sha256?: string;
}

export interface Registry {
  version: number;
  spores: RegistryEntry[];
}

export type Fetcher = (
  url: string,
) => Promise<{ ok: boolean; status: number; text(): Promise<string> }>;

export function manifestUrl(entry: RegistryEntry): string {
  const gh = entry.repo.match(/^github:([^/]+)\/([^/#]+)(?:#(.+))?$/);
  const base = gh
    ? `https://raw.githubusercontent.com/${gh[1]}/${gh[2]}/${gh[3] ?? 'main'}`
    : entry.repo.replace(/\/$/, '');
  return `${base}/${entry.path.replace(/^\/|\/$/g, '')}/spore.json`;
}

export function sha256(text: string): string {
  return createHash('sha256').update(text).digest('hex');
}

export async function fetchRegistry(url: string, fetcher: Fetcher = fetch): Promise<Registry> {
  const res = await fetcher(url);
  if (!res.ok) throw new Error(`registry ${url} responded ${res.status}`);
  const reg = JSON.parse(await res.text()) as Registry;
  if (!Array.isArray(reg.spores)) throw new Error(`registry ${url} has no "spores" array`);
  return reg;
}

export interface FetchedSpore {
  entry: RegistryEntry;
  text: string;
  manifest: { name: string; version: string };
}

export async function fetchSpore(
  entry: RegistryEntry,
  fetcher: Fetcher = fetch,
): Promise<FetchedSpore> {
  const url = manifestUrl(entry);
  const res = await fetcher(url);
  if (!res.ok) throw new Error(`spore manifest ${url} responded ${res.status}`);
  const text = await res.text();
  if (entry.sha256) {
    const actual = sha256(text);
    if (actual !== entry.sha256.toLowerCase()) {
      throw new Error(`sha256 mismatch for ${entry.name}: expected ${entry.sha256}, got ${actual}`);
    }
  }
  let manifest: { name: string; version: string };
  try {
    manifest = JSON.parse(text) as { name: string; version: string };
  } catch (e) {
    throw new Error(`spore manifest ${url} is not valid JSON: ${(e as Error).message}`);
  }
  if (manifest.name !== entry.name) {
    throw new Error(
      `spore manifest name "${manifest.name}" does not match registry entry "${entry.name}"`,
    );
  }
  return { entry, text, manifest };
}
