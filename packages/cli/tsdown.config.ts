import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/cli.ts'],
  format: 'esm',
  platform: 'node',
  target: 'node22',
  dts: false,
  fixedExtension: false,
  clean: true,
  external: ['@aneural/core', '@aneural/mcp'],
});
