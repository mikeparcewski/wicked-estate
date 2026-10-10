// TS-S5 adversarial readiness fixture: each function isolates one construct a data-flow or taint
// layer would have to model. `crates/wicked-estate/tests/dataflow_readiness.rs` pins what the
// graph says about each today (ADR-014's readiness matrix cites it).

export function assignment(src: string): string {
  const a = src;
  const b = a;
  return b;
}

export function destructuring(obj: { k: string }): string {
  const { k } = obj;
  return k;
}

export function branch(src: string, flag: boolean): string {
  let out = 'safe';
  if (flag) {
    out = src;
  }
  return out;
}

export function loop(items: string[]): string {
  let acc = '';
  for (const it of items) {
    acc = acc + it;
  }
  return acc;
}

export function closure(src: string): () => string {
  const captured = src;
  return () => captured;
}

export function promise(src: string): Promise<string> {
  return Promise.resolve(src).then((v) => v);
}

export function property(src: string): string {
  const o = { field: src };
  return o.field;
}

export function sanitize(raw: string): string {
  return raw.replace(/</g, '&lt;');
}

export function sanitized(raw: string): string {
  const clean = sanitize(raw);
  return clean;
}

// ADR-014 S5b expected NON-flows. An unrelated parameter does not reach a call's result.
export function returnCallUnrelated(raw: string, unrelated: string): string {
  const unused = unrelated;
  return raw.trim();
}

// A `return` inside a callback is the callback's, never the enclosing function's.
export function callbackReturn(src: string, items: string[]): string {
  items.forEach((it) => {
    return src.trim();
  });
  return 'done';
}

// Out of S5b's scope: a chained receiver (the outer call's receiver is itself a call).
export function chainedReceiver(raw: string): string {
  return raw.trim().toLowerCase();
}

// A returned call's identifier arguments influence its result; a literal argument does not.
export function returnCallArgs(raw: string, other: string): string {
  return combine(raw, 'x', other);
}

export function combine(a: string, sep: string, b: string): string {
  return a + sep + b;
}

// A returned arrow's own parameter is a future call's argument, not a contributor.
export function returnParamArrow(src: string): (x: string) => string {
  const unused = src;
  return (x) => x;
}

// A destructured arrow and a private arrow field mint no definition, so their `return`s belong to
// no slot: never to the enclosing function or class.
export function destructuredArrow(raw: string): string {
  const { length } = () => {
    return raw;
  };
  return 'safe';
}

export class PrivateArrow {
  #handle = (raw: string) => {
    return raw;
  };
}

// ADR-014 S5c destructuring shapes and expected NON-flows.
export function destructuringShapes(obj: { a: string; b: string; c?: string }, arr: string[]): string {
  const { a: alias, b: bb = 'd', c = 'd', ...rest } = obj;
  const [first, second = 'd', ...others] = arr;
  return alias;
}

export function destructuringOther(obj: { k: string }, other: { j: string }): string {
  const { k } = obj;
  const { j } = other;
  return j;
}

export function destructuringShadow(obj: { k: string }): string {
  const { k } = obj;
  {
    const k = 'x';
    return k;
  }
}

// Out of S5c's scope: a nested pattern binds nothing from `obj`.
export function destructuringNested(obj: { a: { b: string } }): string {
  const {
    a: { b },
  } = obj;
  return b;
}

// ADR-014 S5d loop shapes and expected NON-flows.
export function loopAugmented(items: string[]): string {
  let total = '';
  for (const s of items) {
    total += s;
  }
  return total;
}

export function loopPattern(pairs: [string, string][]): string {
  for (const [k, v] of pairs) {
    return v;
  }
  return '';
}

// `for…in` binds KEYS, not elements: no flow from the object.
export function loopKeys(obj: { [k: string]: string }): string {
  let last = '';
  for (const key in obj) {
    last = key;
  }
  return last;
}

// The loop binding shadows the parameter of the same name; the return reads the parameter.
export function loopShadow(items: string[], it: string): string {
  for (const it of items) {
  }
  return it;
}

// ADR-014 S6a inline array callbacks and expected NON-flows.
export function cbMap(items: string[]): string[] {
  const out = items.map((x) => x.trim());
  return out;
}

export function cbMapFn(items: string[]): string[] {
  const out = items.map(function (x) {
    return x;
  });
  return out;
}

export function cbFlatMapBlock(items: string[]): string[] {
  const out = items.flatMap((x) => {
    const y = x;
    return y;
  });
  return out;
}

export function cbFilter(items: string[], needle: string): string[] {
  const kept = items.filter((x) => x === needle);
  return kept;
}

export function cbFind(items: string[]): string | undefined {
  return items.find((x) => x.length > 0);
}

export function cbForEach(items: string[]): string {
  let last = '';
  items.forEach((x) => {
    last = x;
  });
  return last;
}

export function cbReduce(items: string[], seed: string): string {
  return items.reduce((acc, x) => acc + x, seed);
}

// A named callback is out of scope: binding its parameter would be interprocedural.
export function cbNamed(items: string[]): string[] {
  const out = items.map(fmt);
  return out;
}

export function fmt(v: string): string {
  return v;
}

// `some` / `every` return a boolean: neither the receiver nor a captured value reaches it.
export function cbSome(items: string[], needle: string): boolean {
  const ok = items.some((x) => x === needle);
  return ok;
}

// S6a review cases: a named function expression's own name, a commented return, a single
// unparenthesized `reduce` parameter, and a TypeScript `this` parameter (erased at runtime).
export function cbNamedExpr(items: string[], x: string): string[] {
  const out = items.map(function x(v) {
    return x;
  });
  return out;
}

export function cbCommented(items: string[]): string[] {
  const out = items.map((x) => {
    return /* the element */ x;
  });
  return out;
}

export function cbReduceSingle(items: string[], seed: string): string {
  const out = items.reduce(acc => acc, seed);
  return out;
}

export function cbReduceThis(items: string[], seed: string): string {
  const out = items.reduce(function (this: void, acc: string, x: string) {
    return acc;
  }, seed);
  return out;
}

// The callback parameter shadows the returned parameter of the same name.
export function cbShadow(items: string[], x: string): string {
  const out = items.map((x) => x);
  return x;
}
