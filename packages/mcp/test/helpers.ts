import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

export function copyFixture(): string {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const src = path.resolve(here, '../../../fixtures/sample-workspace');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'aneural-mcp-'));
  const root = path.join(tmp, 'sample-workspace');
  fs.cpSync(src, root, { recursive: true });
  fs.rmSync(path.join(root, '.aneural/cache'), { recursive: true, force: true });
  fs.rmSync(path.join(root, '.aneural/state'), { recursive: true, force: true });
  fs.mkdirSync(path.join(root, 'apps/web/.git'), { recursive: true });
  return fs.realpathSync(root);
}
