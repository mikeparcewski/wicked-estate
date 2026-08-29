// The three import forms the original queries missed (review D01-7) + TS import-equals.
export * from './y';
const z = require('./z');
const dyn = import('./dyn');
import req = require('./req');
export function UseAll() {
  return <div>{[z, dyn, req]}</div>;
}

// Satisfy the extends/implements fixture gate for this language.
interface Shape { area(): number; }
class Base {}
export class Sub extends Base implements Shape {
  area(): number { return 0; }
}
