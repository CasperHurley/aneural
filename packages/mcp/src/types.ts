import type { Edge, Focus, Node } from '@aneural/core';

/** The shape of `.aneural/state/focus.json` (camelCase, see aneural-core focus.rs). */
export interface FocusFilters {
  kinds?: string[];
  edgeKinds?: string[];
  repos?: string[];
  query?: string;
}

export interface FocusSelection {
  primary?: string | null;
  pinned?: string[];
}

export interface FocusNeighborhood {
  depth?: number;
  direction?: 'in' | 'out' | 'both';
}

export interface FocusShape extends Focus {
  filters: FocusFilters;
  selection: FocusSelection;
  neighborhood: FocusNeighborhood;
}

export interface FocusFile {
  path: string;
  node: Node;
  content?: string;
  truncated?: boolean;
  skipped?: string;
}

export interface FocusBundle {
  root: string;
  workspaceName: string;
  focus: FocusShape | null;
  /** Nodes the user explicitly pointed at (primary first, then pinned). */
  anchors: Node[];
  /** Anchors plus their neighbourhood. */
  nodes: Node[];
  edges: Edge[];
  files: FocusFile[];
  depth: number;
  direction: string;
}

export interface BuildFocusOptions {
  includeContent?: boolean;
  maxBytes?: number;
  depth?: number;
}
