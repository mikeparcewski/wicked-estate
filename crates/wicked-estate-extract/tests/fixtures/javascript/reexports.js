// The three import forms the original queries missed (review D01-7).
export * from './y';
const z = require('./z');
const dyn = import('./dyn');
export function useAll() {
  return [z, dyn];
}
