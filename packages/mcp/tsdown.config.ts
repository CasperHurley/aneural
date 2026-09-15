import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/index.ts', 'src/bin.ts'],
  format: 'esm',
  platform: 'node',
  target: 'node22',
  dts: true,
  fixedExtension: false,
  clean: true,
  sourcemap: false,
});
