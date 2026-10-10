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
