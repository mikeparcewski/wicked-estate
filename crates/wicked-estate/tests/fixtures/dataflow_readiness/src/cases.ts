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
  const { a: alias, c = 'd', ...rest } = obj;
  const [first, ...others] = arr;
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
