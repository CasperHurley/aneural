# @aneural/core

Native (napi-rs) bindings to the Aneural indexing engine. See `docs/napi-api.md` in the repository.

```ts
import * as core from '@aneural/core';
const root = core.findWorkspace(process.cwd());
await core.indexWorkspace(root);
core.neighborhood(root, ['file:src/index.ts'], { depth: 1 });
```
