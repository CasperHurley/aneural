import React from 'react';
import { App } from '@/app';
import './styles.css';

// TODO: hydrate from server state
export function main(): void {
  App.mount(React);
}
