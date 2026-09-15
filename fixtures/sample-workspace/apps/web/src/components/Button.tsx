import type { Props } from '../types';

export function Button(props: Props) {
  const load = () => import('../lib/util');
  void load;
  return props.title;
}
