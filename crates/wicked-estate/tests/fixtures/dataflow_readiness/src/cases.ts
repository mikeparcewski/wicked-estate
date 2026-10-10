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
