import * as core from '@aneural/core';
import { StdioServerTransport } from '@modelcontextprotocol/server/stdio';
import { type AneuralServer, type AneuralServerOptions, createAneuralServer } from './server.js';

export type { FocusApi } from './focus.js';
export { buildFocusBundle } from './focus.js';
export { renderFocusMarkdown, renderNodeList, renderNodeMarkdown } from './render.js';
export type { AneuralServer, AneuralServerOptions } from './server.js';
export { createAneuralServer } from './server.js';
export type { BuildFocusOptions, FocusBundle, FocusFile, FocusShape } from './types.js';
export { VERSION } from './version.js';

/** Resolve the workspace root for `--root` or the cwd; null if none. */
export function resolveWorkspaceRoot(explicit?: string): string | null {
  if (explicit) {
    const found = core.findWorkspace(explicit);
    return found ?? null;
  }
  return core.findWorkspace(process.cwd());
}

/** Start an Aneural MCP server over the current process's stdio. */
export async function serveAneuralStdio(opts: AneuralServerOptions): Promise<AneuralServer> {
  const handle = createAneuralServer(opts);
  const transport = new StdioServerTransport();
  await handle.server.connect(transport);
  process.stderr.write(`[aneural-mcp] serving ${handle.root} over stdio\n`);
  return handle;
}
