#!/usr/bin/env node
import { resolveWorkspaceRoot, serveAneuralStdio } from './index.js';

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(name);
  if (i >= 0) return process.argv[i + 1];
  const eq = process.argv.find((a) => a.startsWith(`${name}=`));
  return eq?.slice(name.length + 1);
}

const root = resolveWorkspaceRoot(arg('--root'));
if (!root) {
  process.stderr.write(
    'aneural-mcp: no .aneural workspace found (pass --root <dir> or run `aneural init`)\n',
  );
  process.exit(2);
}
const handle = await serveAneuralStdio({ root });
const shutdown = (): void => {
  handle.close().finally(() => process.exit(0));
};
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
