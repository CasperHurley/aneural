import { format } from './lib/util.js';
import type { Props } from './types';
export * from './lib/util';

export const App = {
  mount(_react: unknown): void {
    const p: Props = { title: format('aneural') };
    void p;
  },
};
